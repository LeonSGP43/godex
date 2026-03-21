use crate::agent::AgentStatus;
use crate::agent::backend::ClaudeCodeResumeState;
use crate::agent::backend::SpawnedAgentBackendKind;
use crate::agent::backend::SpawnedAgentHandle;
use crate::agent::guards::Guards;
use crate::agent::role::DEFAULT_ROLE_NAME;
use crate::agent::role::resolve_role_config;
use crate::agent::status::is_final;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::find_archived_thread_path_by_id_str;
use crate::find_thread_path_by_id_str;
use crate::rollout::RolloutRecorder;
use crate::session_prefix::format_subagent_context_line;
use crate::session_prefix::format_subagent_notification_message;
use crate::shell_snapshot::ShellSnapshot;
use crate::state_db;
use crate::thread_manager::ThreadManagerState;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::user_input::UserInput;
use codex_state::DirectionalThreadSpawnEdgeStatus;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::RwLock;
use tokio::sync::watch;
use tracing::warn;

const AGENT_NAMES: &str = include_str!("agent_names.txt");
const FORKED_SPAWN_AGENT_OUTPUT_MESSAGE: &str = "You are the newly spawned agent. The prior conversation history was forked from your parent agent. Treat the next user message as your new task, and use the forked history only as background context.";

#[derive(Clone, Debug, Default)]
pub(crate) struct SpawnAgentOptions {
    pub(crate) fork_parent_spawn_call_id: Option<String>,
    pub(crate) backend_kind: SpawnedAgentBackendKind,
}

fn default_agent_nickname_list() -> Vec<&'static str> {
    AGENT_NAMES
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

fn agent_nickname_candidates(
    config: &crate::config::Config,
    role_name: Option<&str>,
) -> Vec<String> {
    let role_name = role_name.unwrap_or(DEFAULT_ROLE_NAME);
    if let Some(candidates) =
        resolve_role_config(config, role_name).and_then(|role| role.nickname_candidates.clone())
    {
        return candidates;
    }

    default_agent_nickname_list()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

/// Control-plane handle for multi-agent operations.
/// `AgentControl` is held by each session (via `SessionServices`). It provides capability to
/// spawn new agents and the inter-agent communication layer.
/// An `AgentControl` instance is shared per "user session" which means the same `AgentControl`
/// is used for every sub-agent spawned by Codex. By doing so, we make sure the guards are
/// scoped to a user session.
#[derive(Clone, Default)]
pub(crate) struct AgentControl {
    /// Weak handle back to the global thread registry/state.
    /// This is `Weak` to avoid reference cycles and shadow persistence of the form
    /// `ThreadManagerState -> CodexThread -> Session -> SessionServices -> ThreadManagerState`.
    manager: Weak<ThreadManagerState>,
    state: Arc<Guards>,
    spawned_agents: Arc<RwLock<HashMap<ThreadId, SpawnedAgentHandle>>>,
    closed_claude_code_agents: Arc<RwLock<HashMap<ThreadId, ClaudeCodeResumeState>>>,
}

impl AgentControl {
    /// Construct a new `AgentControl` that can spawn/message agents via the given manager state.
    pub(crate) fn new(manager: Weak<ThreadManagerState>) -> Self {
        Self {
            manager,
            ..Default::default()
        }
    }

    /// Spawn a new agent thread and submit the initial prompt.
    pub(crate) async fn spawn_agent(
        &self,
        config: crate::config::Config,
        items: Vec<UserInput>,
        session_source: Option<SessionSource>,
    ) -> CodexResult<ThreadId> {
        self.spawn_agent_with_options(config, items, session_source, SpawnAgentOptions::default())
            .await
    }

    pub(crate) async fn spawn_agent_with_options(
        &self,
        config: crate::config::Config,
        items: Vec<UserInput>,
        session_source: Option<SessionSource>,
        options: SpawnAgentOptions,
    ) -> CodexResult<ThreadId> {
        let state = self.upgrade()?;
        let mut reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;
        let inherited_shell_snapshot = self
            .inherited_shell_snapshot_for_source(&state, session_source.as_ref())
            .await;
        let inherited_exec_policy = self
            .inherited_exec_policy_for_source(&state, session_source.as_ref(), &config)
            .await;
        let session_source = match session_source {
            Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth,
                agent_role,
                ..
            })) => {
                let candidate_names = agent_nickname_candidates(&config, agent_role.as_deref());
                let candidate_name_refs: Vec<&str> =
                    candidate_names.iter().map(String::as_str).collect();
                let agent_nickname = reservation.reserve_agent_nickname(&candidate_name_refs)?;
                Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id,
                    depth,
                    agent_nickname: Some(agent_nickname),
                    agent_role,
                }))
            }
            other => other,
        };
        let notification_source = session_source.clone();

        if matches!(options.backend_kind, SpawnedAgentBackendKind::ClaudeCode) {
            let agent_id = ThreadId::new();
            let launch_items = if let Some(call_id) = options.fork_parent_spawn_call_id.as_ref() {
                build_external_backend_fork_items(
                    &state,
                    &config,
                    session_source.as_ref(),
                    call_id,
                    items,
                )
                .await?
            } else {
                items
            };
            let config_snapshot = external_backend_config_snapshot(
                &config,
                session_source.clone().unwrap_or(SessionSource::Exec),
            );
            let handle = SpawnedAgentHandle::claude_code(
                agent_id,
                config_snapshot,
                config.developer_instructions.clone(),
                launch_items,
            )
            .await?;
            reservation.commit(agent_id);
            self.register_spawned_agent(handle).await;
            self.closed_claude_code_agents
                .write()
                .await
                .remove(&agent_id);
            self.persist_thread_spawn_edge_for_source(agent_id, notification_source.as_ref())
                .await;
            self.maybe_start_completion_watcher(agent_id, notification_source);
            return Ok(agent_id);
        }

        // The same `AgentControl` is sent to spawn the thread.
        let new_thread = match session_source {
            Some(session_source) => {
                if let Some(call_id) = options.fork_parent_spawn_call_id.as_ref() {
                    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                        parent_thread_id,
                        ..
                    }) = session_source.clone()
                    else {
                        return Err(CodexErr::Fatal(
                            "spawn_agent fork requires a thread-spawn session source".to_string(),
                        ));
                    };
                    let parent_thread = state.get_thread(parent_thread_id).await.ok();
                    if let Some(parent_thread) = parent_thread.as_ref() {
                        // `record_conversation_items` only queues rollout writes asynchronously.
                        // Flush/materialize the live parent before snapshotting JSONL for a fork.
                        parent_thread
                            .codex
                            .session
                            .ensure_rollout_materialized()
                            .await;
                        parent_thread.codex.session.flush_rollout().await;
                    }
                    let rollout_path = parent_thread
                        .as_ref()
                        .and_then(|parent_thread| parent_thread.rollout_path())
                        .or(find_thread_path_by_id_str(
                            config.codex_home.as_path(),
                            &parent_thread_id.to_string(),
                        )
                        .await?)
                        .ok_or_else(|| {
                            CodexErr::Fatal(format!(
                                "parent thread rollout unavailable for fork: {parent_thread_id}"
                            ))
                        })?;
                    let mut forked_rollout_items: Vec<RolloutItem> =
                        RolloutRecorder::get_rollout_history(&rollout_path)
                            .await?
                            .get_rollout_items();
                    let mut output = FunctionCallOutputPayload::from_text(
                        FORKED_SPAWN_AGENT_OUTPUT_MESSAGE.to_string(),
                    );
                    output.success = Some(true);
                    forked_rollout_items.push(RolloutItem::ResponseItem(
                        ResponseItem::FunctionCallOutput {
                            call_id: call_id.clone(),
                            output,
                        },
                    ));
                    let initial_history = InitialHistory::Forked(forked_rollout_items);
                    state
                        .fork_thread_with_source(
                            config,
                            initial_history,
                            self.clone(),
                            session_source,
                            /*persist_extended_history*/ false,
                            inherited_shell_snapshot,
                            inherited_exec_policy,
                        )
                        .await?
                } else {
                    state
                        .spawn_new_thread_with_source(
                            config,
                            self.clone(),
                            session_source,
                            /*persist_extended_history*/ false,
                            /*metrics_service_name*/ None,
                            inherited_shell_snapshot,
                            inherited_exec_policy,
                        )
                        .await?
                }
            }
            None => state.spawn_new_thread(config, self.clone()).await?,
        };
        reservation.commit(new_thread.thread_id);
        self.register_spawned_agent(SpawnedAgentHandle::codex(
            Arc::downgrade(&state),
            new_thread.thread_id,
        ))
        .await;

        // Notify a new thread has been created. This notification will be processed by clients
        // to subscribe or drain this newly created thread.
        // TODO(jif) add helper for drain
        state.notify_thread_created(new_thread.thread_id);

        self.persist_thread_spawn_edge_for_source(
            new_thread.thread_id,
            notification_source.as_ref(),
        )
        .await;

        self.send_input(new_thread.thread_id, items).await?;
        self.maybe_start_completion_watcher(new_thread.thread_id, notification_source);

        Ok(new_thread.thread_id)
    }

    /// Resume an existing agent thread from a recorded rollout file.
    pub(crate) async fn resume_agent_from_rollout(
        &self,
        config: crate::config::Config,
        thread_id: ThreadId,
        session_source: SessionSource,
    ) -> CodexResult<ThreadId> {
        if self
            .resume_closed_claude_code_agent(thread_id, &session_source, &config)
            .await?
        {
            return Ok(thread_id);
        }

        let root_depth = thread_spawn_depth(&session_source).unwrap_or(0);
        let resumed_thread_id = self
            .resume_single_agent_from_rollout(config.clone(), thread_id, session_source)
            .await?;
        let state = self.upgrade()?;
        let Ok(resumed_thread) = state.get_thread(resumed_thread_id).await else {
            return Ok(resumed_thread_id);
        };
        let Some(state_db_ctx) = resumed_thread.state_db() else {
            return Ok(resumed_thread_id);
        };

        let mut resume_queue = VecDeque::from([(thread_id, root_depth)]);
        while let Some((parent_thread_id, parent_depth)) = resume_queue.pop_front() {
            let child_ids = match state_db_ctx
                .list_thread_spawn_children_with_status(
                    parent_thread_id,
                    DirectionalThreadSpawnEdgeStatus::Open,
                )
                .await
            {
                Ok(child_ids) => child_ids,
                Err(err) => {
                    warn!(
                        "failed to load persisted thread-spawn children for {parent_thread_id}: {err}"
                    );
                    continue;
                }
            };

            for child_thread_id in child_ids {
                let child_depth = parent_depth + 1;
                let child_resumed = if state.get_thread(child_thread_id).await.is_ok() {
                    true
                } else {
                    let child_session_source =
                        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                            parent_thread_id,
                            depth: child_depth,
                            agent_nickname: None,
                            agent_role: None,
                        });
                    match self
                        .resume_single_agent_from_rollout(
                            config.clone(),
                            child_thread_id,
                            child_session_source,
                        )
                        .await
                    {
                        Ok(_) => true,
                        Err(err) => {
                            warn!("failed to resume descendant thread {child_thread_id}: {err}");
                            false
                        }
                    }
                };
                if child_resumed {
                    resume_queue.push_back((child_thread_id, child_depth));
                }
            }
        }

        Ok(resumed_thread_id)
    }

    async fn resume_single_agent_from_rollout(
        &self,
        mut config: crate::config::Config,
        thread_id: ThreadId,
        session_source: SessionSource,
    ) -> CodexResult<ThreadId> {
        if let SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) = &session_source
            && *depth >= config.agent_max_depth
        {
            let _ = config.features.disable(Feature::SpawnCsv);
            let _ = config.features.disable(Feature::Collab);
        }
        let state = self.upgrade()?;
        let mut reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;
        let session_source = match session_source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth,
                ..
            }) => {
                // Collab resume callers rebuild a placeholder ThreadSpawn source. Rehydrate the
                // stored nickname/role from sqlite when available; otherwise leave both unset.
                let (resumed_agent_nickname, resumed_agent_role) =
                    if let Some(state_db_ctx) = state_db::get_state_db(&config).await {
                        match state_db_ctx.get_thread(thread_id).await {
                            Ok(Some(metadata)) => (metadata.agent_nickname, metadata.agent_role),
                            Ok(None) | Err(_) => (None, None),
                        }
                    } else {
                        (None, None)
                    };
                let reserved_agent_nickname = resumed_agent_nickname
                    .as_deref()
                    .map(|agent_nickname| {
                        let candidate_names =
                            agent_nickname_candidates(&config, resumed_agent_role.as_deref());
                        let candidate_name_refs: Vec<&str> =
                            candidate_names.iter().map(String::as_str).collect();
                        reservation.reserve_agent_nickname_with_preference(
                            &candidate_name_refs,
                            Some(agent_nickname),
                        )
                    })
                    .transpose()?;
                SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id,
                    depth,
                    agent_nickname: reserved_agent_nickname,
                    agent_role: resumed_agent_role,
                })
            }
            other => other,
        };
        let notification_source = session_source.clone();
        let inherited_shell_snapshot = self
            .inherited_shell_snapshot_for_source(&state, Some(&session_source))
            .await;
        let inherited_exec_policy = self
            .inherited_exec_policy_for_source(&state, Some(&session_source), &config)
            .await;
        let rollout_path =
            match find_thread_path_by_id_str(config.codex_home.as_path(), &thread_id.to_string())
                .await?
            {
                Some(rollout_path) => rollout_path,
                None => find_archived_thread_path_by_id_str(
                    config.codex_home.as_path(),
                    &thread_id.to_string(),
                )
                .await?
                .ok_or_else(|| CodexErr::ThreadNotFound(thread_id))?,
            };

        let resumed_thread = state
            .resume_thread_from_rollout_with_source(
                config,
                rollout_path,
                self.clone(),
                session_source,
                inherited_shell_snapshot,
                inherited_exec_policy,
            )
            .await?;
        reservation.commit(resumed_thread.thread_id);
        self.register_spawned_agent(SpawnedAgentHandle::codex(
            Arc::downgrade(&state),
            resumed_thread.thread_id,
        ))
        .await;
        // Resumed threads are re-registered in-memory and need the same listener
        // attachment path as freshly spawned threads.
        state.notify_thread_created(resumed_thread.thread_id);
        self.maybe_start_completion_watcher(
            resumed_thread.thread_id,
            Some(notification_source.clone()),
        );
        self.persist_thread_spawn_edge_for_source(
            resumed_thread.thread_id,
            Some(&notification_source),
        )
        .await;

        Ok(resumed_thread.thread_id)
    }

    /// Send rich user input items to an existing agent thread.
    pub(crate) async fn send_input(
        &self,
        agent_id: ThreadId,
        items: Vec<UserInput>,
    ) -> CodexResult<String> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            let result = handle.send_input(items).await;
            if matches!(result, Err(CodexErr::InternalAgentDied)) {
                handle.cleanup_after_internal_death().await;
                self.unregister_spawned_agent(agent_id).await;
                self.state.release_spawned_thread(agent_id);
            }
            return result;
        }
        let state = self.upgrade()?;
        let result = state
            .send_op(
                agent_id,
                Op::UserInput {
                    items,
                    final_output_json_schema: None,
                },
            )
            .await;
        if matches!(result, Err(CodexErr::InternalAgentDied)) {
            let _ = state.remove_thread(&agent_id).await;
            self.unregister_spawned_agent(agent_id).await;
            self.state.release_spawned_thread(agent_id);
        }
        result
    }

    /// Interrupt the current task for an existing agent thread.
    pub(crate) async fn interrupt_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            return handle.interrupt().await;
        }
        let state = self.upgrade()?;
        state.send_op(agent_id, Op::Interrupt).await
    }

    /// Submit a shutdown request for a live agent without marking it explicitly closed in
    /// persisted spawn-edge state.
    pub(crate) async fn shutdown_live_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            if let Some(state) = handle.claude_code_resume_state().await {
                self.closed_claude_code_agents
                    .write()
                    .await
                    .insert(agent_id, state);
            }
            let result = handle.shutdown_live().await;
            self.unregister_spawned_agent(agent_id).await;
            self.state.release_spawned_thread(agent_id);
            return result;
        }
        let state = self.upgrade()?;
        let result = if let Ok(thread) = state.get_thread(agent_id).await {
            thread.codex.session.ensure_rollout_materialized().await;
            thread.codex.session.flush_rollout().await;
            if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state.send_op(agent_id, Op::Shutdown {}).await
            }
        } else {
            state.send_op(agent_id, Op::Shutdown {}).await
        };
        let _ = state.remove_thread(&agent_id).await;
        self.unregister_spawned_agent(agent_id).await;
        self.state.release_spawned_thread(agent_id);
        result
    }

    /// Mark `agent_id` as explicitly closed in persisted spawn-edge state, then shut down the
    /// agent and any live descendants reached from the in-memory tree.
    pub(crate) async fn close_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let session_source = self
            .get_agent_config_snapshot(agent_id)
            .await
            .map(|snapshot| snapshot.session_source);
        self.persist_thread_spawn_edge_status_for_source(
            agent_id,
            session_source.as_ref(),
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await;
        self.shutdown_agent_tree(agent_id).await
    }

    /// Shut down `agent_id` and any live descendants reachable from the in-memory spawn tree.
    async fn shutdown_agent_tree(&self, agent_id: ThreadId) -> CodexResult<String> {
        let descendant_ids = self.live_thread_spawn_descendants(agent_id).await?;
        let result = self.shutdown_live_agent(agent_id).await;
        for descendant_id in descendant_ids {
            match self.shutdown_live_agent(descendant_id).await {
                Ok(_) | Err(CodexErr::ThreadNotFound(_)) | Err(CodexErr::InternalAgentDied) => {}
                Err(err) => return Err(err),
            }
        }
        result
    }

    /// Fetch the last known status for `agent_id`, returning `NotFound` when unavailable.
    pub(crate) async fn get_status(&self, agent_id: ThreadId) -> AgentStatus {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            return handle.status().await;
        }
        let Ok(state) = self.upgrade() else {
            // No agent available if upgrade fails.
            return AgentStatus::NotFound;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return AgentStatus::NotFound;
        };
        thread.agent_status().await
    }

    pub(crate) async fn get_agent_nickname_and_role(
        &self,
        agent_id: ThreadId,
    ) -> Option<(Option<String>, Option<String>)> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await
            && let Some(snapshot) = handle.config_snapshot().await
        {
            let session_source = snapshot.session_source;
            return Some((
                session_source.get_nickname(),
                session_source.get_agent_role(),
            ));
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return None;
        };
        let session_source = thread.config_snapshot().await.session_source;
        Some((
            session_source.get_nickname(),
            session_source.get_agent_role(),
        ))
    }

    pub(crate) async fn get_agent_config_snapshot(
        &self,
        agent_id: ThreadId,
    ) -> Option<ThreadConfigSnapshot> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            return handle.config_snapshot().await;
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return None;
        };
        Some(thread.config_snapshot().await)
    }

    /// Subscribe to status updates for `agent_id`, yielding the latest value and changes.
    pub(crate) async fn subscribe_status(
        &self,
        agent_id: ThreadId,
    ) -> CodexResult<watch::Receiver<AgentStatus>> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            return handle.subscribe_status().await;
        }
        let state = self.upgrade()?;
        let thread = state.get_thread(agent_id).await?;
        Ok(thread.subscribe_status())
    }

    pub(crate) async fn get_total_token_usage(&self, agent_id: ThreadId) -> Option<TokenUsage> {
        if let Some(handle) = self.spawned_agent_handle(agent_id).await {
            return handle.total_token_usage().await;
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return None;
        };
        thread.total_token_usage().await
    }

    pub(crate) async fn format_environment_context_subagents(
        &self,
        parent_thread_id: ThreadId,
    ) -> String {
        let Ok(agents) = self.open_thread_spawn_children(parent_thread_id).await else {
            return String::new();
        };

        agents
            .into_iter()
            .map(|(thread_id, nickname)| {
                format_subagent_context_line(&thread_id.to_string(), nickname.as_deref())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Starts a detached watcher for sub-agents spawned from another thread.
    ///
    /// This is only enabled for `SubAgentSource::ThreadSpawn`, where a parent thread exists and
    /// can receive completion notifications.
    fn maybe_start_completion_watcher(
        &self,
        child_thread_id: ThreadId,
        session_source: Option<SessionSource>,
    ) {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return;
        };
        let control = self.clone();
        tokio::spawn(async move {
            let status = match control.subscribe_status(child_thread_id).await {
                Ok(mut status_rx) => {
                    let mut status = status_rx.borrow().clone();
                    while !is_final(&status) {
                        if status_rx.changed().await.is_err() {
                            status = control.get_status(child_thread_id).await;
                            break;
                        }
                        status = status_rx.borrow().clone();
                    }
                    status
                }
                Err(_) => control.get_status(child_thread_id).await,
            };
            if !is_final(&status) {
                return;
            }

            let Ok(state) = control.upgrade() else {
                return;
            };
            let Ok(parent_thread) = state.get_thread(parent_thread_id).await else {
                return;
            };
            parent_thread
                .inject_user_message_without_turn(format_subagent_notification_message(
                    &child_thread_id.to_string(),
                    &status,
                ))
                .await;
        });
    }

    fn upgrade(&self) -> CodexResult<Arc<ThreadManagerState>> {
        self.manager
            .upgrade()
            .ok_or_else(|| CodexErr::UnsupportedOperation("thread manager dropped".to_string()))
    }

    async fn inherited_shell_snapshot_for_source(
        &self,
        state: &Arc<ThreadManagerState>,
        session_source: Option<&SessionSource>,
    ) -> Option<Arc<ShellSnapshot>> {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return None;
        };

        let parent_thread = state.get_thread(*parent_thread_id).await.ok()?;
        parent_thread.codex.session.user_shell().shell_snapshot()
    }

    async fn inherited_exec_policy_for_source(
        &self,
        state: &Arc<ThreadManagerState>,
        session_source: Option<&SessionSource>,
        child_config: &crate::config::Config,
    ) -> Option<Arc<crate::exec_policy::ExecPolicyManager>> {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return None;
        };

        let parent_thread = state.get_thread(*parent_thread_id).await.ok()?;
        let parent_config = parent_thread.codex.session.get_config().await;
        if !crate::exec_policy::child_uses_parent_exec_policy(&parent_config, child_config) {
            return None;
        }

        Some(Arc::clone(
            &parent_thread.codex.session.services.exec_policy,
        ))
    }

    async fn open_thread_spawn_children(
        &self,
        parent_thread_id: ThreadId,
    ) -> CodexResult<Vec<(ThreadId, Option<String>)>> {
        let mut children_by_parent = self.live_thread_spawn_children().await?;
        Ok(children_by_parent
            .remove(&parent_thread_id)
            .unwrap_or_default())
    }

    async fn live_thread_spawn_children(
        &self,
    ) -> CodexResult<HashMap<ThreadId, Vec<(ThreadId, Option<String>)>>> {
        let mut children_by_parent = HashMap::<ThreadId, Vec<(ThreadId, Option<String>)>>::new();
        let mut seen_thread_ids = std::collections::HashSet::new();

        for agent_id in self.spawned_agent_ids().await {
            let Some(snapshot) = self.get_agent_config_snapshot(agent_id).await else {
                continue;
            };
            let Some(parent_thread_id) = thread_spawn_parent_thread_id(&snapshot.session_source)
            else {
                continue;
            };
            seen_thread_ids.insert(agent_id);
            children_by_parent
                .entry(parent_thread_id)
                .or_default()
                .push((agent_id, snapshot.session_source.get_nickname()));
        }

        let state = self.upgrade()?;

        for thread_id in state.list_thread_ids().await {
            if seen_thread_ids.contains(&thread_id) {
                continue;
            }
            let Ok(thread) = state.get_thread(thread_id).await else {
                continue;
            };
            let snapshot = thread.config_snapshot().await;
            let Some(parent_thread_id) = thread_spawn_parent_thread_id(&snapshot.session_source)
            else {
                continue;
            };
            children_by_parent
                .entry(parent_thread_id)
                .or_default()
                .push((thread_id, snapshot.session_source.get_nickname()));
        }

        for children in children_by_parent.values_mut() {
            children.sort_by(|left, right| left.0.to_string().cmp(&right.0.to_string()));
        }

        Ok(children_by_parent)
    }

    async fn register_spawned_agent(&self, handle: SpawnedAgentHandle) {
        self.spawned_agents
            .write()
            .await
            .insert(handle.agent_id(), handle);
    }

    async fn unregister_spawned_agent(&self, agent_id: ThreadId) {
        self.spawned_agents.write().await.remove(&agent_id);
    }

    async fn spawned_agent_handle(&self, agent_id: ThreadId) -> Option<SpawnedAgentHandle> {
        self.spawned_agents.read().await.get(&agent_id).cloned()
    }

    async fn spawned_agent_ids(&self) -> Vec<ThreadId> {
        self.spawned_agents.read().await.keys().copied().collect()
    }

    async fn resume_closed_claude_code_agent(
        &self,
        thread_id: ThreadId,
        session_source: &SessionSource,
        config: &crate::config::Config,
    ) -> CodexResult<bool> {
        let Some(state) = self
            .closed_claude_code_agents
            .read()
            .await
            .get(&thread_id)
            .cloned()
        else {
            return Ok(false);
        };

        let reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;

        self.register_spawned_agent(SpawnedAgentHandle::resumed_claude_code(state))
            .await;
        reservation.commit(thread_id);
        self.closed_claude_code_agents
            .write()
            .await
            .remove(&thread_id);
        self.persist_thread_spawn_edge_for_source(thread_id, Some(session_source))
            .await;
        self.maybe_start_completion_watcher(thread_id, Some(session_source.clone()));
        Ok(true)
    }

    async fn persist_thread_spawn_edge_for_source(
        &self,
        child_thread_id: ThreadId,
        session_source: Option<&SessionSource>,
    ) {
        self.persist_thread_spawn_edge_status_for_source(
            child_thread_id,
            session_source,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await;
    }

    async fn persist_thread_spawn_edge_status_for_source(
        &self,
        child_thread_id: ThreadId,
        session_source: Option<&SessionSource>,
        status: DirectionalThreadSpawnEdgeStatus,
    ) {
        let Some(parent_thread_id) = session_source.and_then(thread_spawn_parent_thread_id) else {
            return;
        };
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(parent_thread) = state.get_thread(parent_thread_id).await else {
            return;
        };
        let Some(state_db_ctx) = parent_thread.state_db() else {
            return;
        };
        if let Err(err) = state_db_ctx
            .upsert_thread_spawn_edge(parent_thread_id, child_thread_id, status)
            .await
        {
            warn!(
                "failed to persist thread-spawn edge status {status} for {child_thread_id}: {err}"
            );
        }
    }

    async fn live_thread_spawn_descendants(
        &self,
        root_thread_id: ThreadId,
    ) -> CodexResult<Vec<ThreadId>> {
        let mut children_by_parent = self.live_thread_spawn_children().await?;
        let mut descendants = Vec::new();
        let mut stack = children_by_parent
            .remove(&root_thread_id)
            .unwrap_or_default()
            .into_iter()
            .map(|(child_thread_id, _)| child_thread_id)
            .rev()
            .collect::<Vec<_>>();

        while let Some(thread_id) = stack.pop() {
            descendants.push(thread_id);
            if let Some(children) = children_by_parent.remove(&thread_id) {
                for (child_thread_id, _) in children.into_iter().rev() {
                    stack.push(child_thread_id);
                }
            }
        }

        Ok(descendants)
    }
}

fn thread_spawn_parent_thread_id(session_source: &SessionSource) -> Option<ThreadId> {
    match session_source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        }) => Some(*parent_thread_id),
        _ => None,
    }
}

fn thread_spawn_depth(session_source: &SessionSource) -> Option<i32> {
    match session_source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) => Some(*depth),
        _ => None,
    }
}

fn external_backend_config_snapshot(
    config: &crate::config::Config,
    session_source: SessionSource,
) -> ThreadConfigSnapshot {
    ThreadConfigSnapshot {
        model: config
            .model
            .clone()
            .unwrap_or_else(|| "claude-code".to_string()),
        model_provider_id: "claude_code".to_string(),
        service_tier: config.service_tier,
        approval_policy: config.permissions.approval_policy.value(),
        approvals_reviewer: config.approvals_reviewer,
        sandbox_policy: config.permissions.sandbox_policy.get().clone(),
        cwd: config.cwd.clone(),
        ephemeral: config.ephemeral,
        reasoning_effort: config.model_reasoning_effort,
        personality: config.personality,
        session_source,
    }
}

async fn build_external_backend_fork_items(
    state: &Arc<ThreadManagerState>,
    config: &crate::config::Config,
    session_source: Option<&SessionSource>,
    call_id: &str,
    items: Vec<UserInput>,
) -> CodexResult<Vec<UserInput>> {
    let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id, ..
    })) = session_source
    else {
        return Err(CodexErr::Fatal(
            "spawn_agent fork requires a thread-spawn session source".to_string(),
        ));
    };

    let parent_thread = state.get_thread(*parent_thread_id).await.ok();
    if let Some(parent_thread) = parent_thread.as_ref() {
        parent_thread
            .codex
            .session
            .ensure_rollout_materialized()
            .await;
        parent_thread.codex.session.flush_rollout().await;
    }
    let rollout_path = parent_thread
        .as_ref()
        .and_then(|parent_thread| parent_thread.rollout_path())
        .or(
            find_thread_path_by_id_str(config.codex_home.as_path(), &parent_thread_id.to_string())
                .await?,
        )
        .ok_or_else(|| {
            CodexErr::Fatal(format!(
                "parent thread rollout unavailable for fork: {parent_thread_id}"
            ))
        })?;
    let mut forked_rollout_items = RolloutRecorder::get_rollout_history(&rollout_path)
        .await?
        .get_rollout_items();
    let mut output =
        FunctionCallOutputPayload::from_text(FORKED_SPAWN_AGENT_OUTPUT_MESSAGE.to_string());
    output.success = Some(true);
    forked_rollout_items.push(RolloutItem::ResponseItem(
        ResponseItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output,
        },
    ));

    let prompt = format!(
        "{FORKED_SPAWN_AGENT_OUTPUT_MESSAGE}\n\nParent conversation transcript:\n{}\n\nNew task:\n{}",
        render_external_backend_fork_history(&forked_rollout_items),
        render_external_backend_user_input(&items),
    );
    Ok(vec![UserInput::Text {
        text: prompt,
        text_elements: Vec::new(),
    }])
}

fn render_external_backend_fork_history(items: &[RolloutItem]) -> String {
    items
        .iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(response_item) => {
                render_external_backend_response_item(response_item)
            }
            RolloutItem::Compacted(compacted) => {
                Some(format!("[assistant]\n{}", compacted.message.trim()))
            }
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_external_backend_response_item(item: &ResponseItem) -> Option<String> {
    match item {
        ResponseItem::Message { role, content, .. } => {
            let text = content
                .iter()
                .map(|content| match content {
                    ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                        text.trim().to_string()
                    }
                    ContentItem::InputImage { image_url } => format!("[image:{image_url}]"),
                })
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then(|| format!("[{role}]\n{text}"))
        }
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => {
            let text = output.to_string();
            (!text.trim().is_empty()).then(|| format!("[tool_output]\n{}", text.trim()))
        }
        ResponseItem::ToolSearchOutput {
            status,
            execution,
            tools,
            ..
        } => Some(format!(
            "[tool_search]\nstatus: {status}\nexecution: {execution}\ntools: {}",
            serde_json::to_string(tools).unwrap_or_default()
        )),
        _ => None,
    }
}

fn render_external_backend_user_input(items: &[UserInput]) -> String {
    items
        .iter()
        .map(|item| match item {
            UserInput::Text { text, .. } => text.clone(),
            UserInput::Image { .. } => "[image]".to_string(),
            UserInput::LocalImage { path } => format!("[local_image:{}]", path.display()),
            UserInput::Skill { name, path } => format!("[skill:${name}]({})", path.display()),
            UserInput::Mention { name, path } => format!("[mention:${name}]({path})"),
            _ => "[input]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
