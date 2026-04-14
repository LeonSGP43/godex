use crate::agent::AgentStatus;
use crate::agent::backend::ArchivedSpawnedAgentHandle;
use crate::agent::backend::SpawnedAgentHandle;
use crate::agent::backend::apply_archived_spawned_agent_config;
use crate::agent::registry::AgentMetadata;
use crate::agent::registry::AgentRegistry;
use crate::agent::role::DEFAULT_ROLE_NAME;
use crate::agent::role::resolve_role_config;
use crate::agent::status::is_final;
use crate::codex::emit_subagent_session_started;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::rollout::RolloutRecorder;
use crate::session_prefix::format_subagent_context_line;
use crate::session_prefix::format_subagent_notification_message;
use crate::shell_snapshot::ShellSnapshot;
use crate::thread_manager::ThreadManagerState;
use crate::thread_rollout_truncation::truncate_rollout_to_last_n_fork_turns;
use codex_features::Feature;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::user_input::UserInput;
use codex_rollout::ARCHIVED_SESSIONS_SUBDIR;
use codex_rollout::SESSIONS_SUBDIR;
use codex_rollout::state_db;
use codex_state::DirectionalThreadSpawnEdgeStatus;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::RwLock;
use tokio::sync::watch;
use tracing::warn;
use uuid::Uuid;

const AGENT_NAMES: &str = include_str!("agent_names.txt");
const ROOT_LAST_TASK_MESSAGE: &str = "Main thread";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpawnAgentForkMode {
    FullHistory,
    LastNTurns(usize),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SpawnAgentOptions {
    pub(crate) fork_parent_spawn_call_id: Option<String>,
    pub(crate) fork_mode: Option<SpawnAgentForkMode>,
}

#[derive(Clone, Debug)]
pub(crate) struct LiveAgent {
    pub(crate) thread_id: ThreadId,
    pub(crate) metadata: AgentMetadata,
    pub(crate) status: AgentStatus,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ListedAgent {
    pub(crate) agent_name: String,
    pub(crate) agent_status: AgentStatus,
    pub(crate) last_task_message: Option<String>,
}

#[derive(Clone, Default)]
struct ExternalAgentHandles {
    live: Arc<RwLock<HashMap<ThreadId, SpawnedAgentHandle>>>,
    archived: Arc<RwLock<HashMap<ThreadId, ArchivedSpawnedAgentHandle>>>,
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

fn keep_forked_rollout_item(item: &RolloutItem) -> bool {
    match item {
        RolloutItem::ResponseItem(ResponseItem::Message { role, phase, .. }) => match role.as_str()
        {
            "system" | "developer" | "user" => true,
            "assistant" => *phase == Some(MessagePhase::FinalAnswer),
            _ => false,
        },
        RolloutItem::ResponseItem(
            ResponseItem::Reasoning { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::FunctionCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::FunctionCallOutput { .. }
            | ResponseItem::CustomToolCall { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::GhostSnapshot { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::Other,
        ) => false,
        RolloutItem::Compacted(_)
        | RolloutItem::EventMsg(_)
        | RolloutItem::SessionMeta(_)
        | RolloutItem::TurnContext(_) => true,
    }
}

/// Control-plane handle for multi-agent operations.
/// `AgentControl` is held by each session (via `SessionServices`). It provides capability to
/// spawn new agents and the inter-agent communication layer.
/// An `AgentControl` instance is intended to be created at most once per root thread/session
/// tree. That same `AgentControl` is then shared with every sub-agent spawned from that root,
/// which keeps the registry scoped to that root thread rather than the entire `ThreadManager`.
#[derive(Clone, Default)]
pub(crate) struct AgentControl {
    /// Weak handle back to the global thread registry/state.
    /// This is `Weak` to avoid reference cycles and shadow persistence of the form
    /// `ThreadManagerState -> CodexThread -> Session -> SessionServices -> ThreadManagerState`.
    manager: Weak<ThreadManagerState>,
    state: Arc<AgentRegistry>,
    external_handles: ExternalAgentHandles,
}

impl AgentControl {
    /// Construct a new `AgentControl` that can spawn/message agents via the given manager state.
    pub(crate) fn new(manager: Weak<ThreadManagerState>) -> Self {
        Self {
            manager,
            ..Default::default()
        }
    }

    async fn live_external_handle(&self, agent_id: ThreadId) -> Option<SpawnedAgentHandle> {
        self.external_handles
            .live
            .read()
            .await
            .get(&agent_id)
            .cloned()
    }

    async fn live_external_handle_owned(self, agent_id: ThreadId) -> Option<SpawnedAgentHandle> {
        self.external_handles
            .live
            .read()
            .await
            .get(&agent_id)
            .cloned()
    }

    async fn register_external_handle(&self, handle: SpawnedAgentHandle) {
        let agent_id = handle.agent_id();
        self.external_handles
            .archived
            .write()
            .await
            .remove(&agent_id);
        self.external_handles
            .live
            .write()
            .await
            .insert(agent_id, handle);
    }

    async fn register_external_handle_owned(self, handle: SpawnedAgentHandle) {
        let agent_id = handle.agent_id();
        self.external_handles
            .archived
            .write()
            .await
            .remove(&agent_id);
        self.external_handles
            .live
            .write()
            .await
            .insert(agent_id, handle);
    }

    async fn take_live_external_handle(&self, agent_id: ThreadId) -> Option<SpawnedAgentHandle> {
        self.external_handles.live.write().await.remove(&agent_id)
    }

    async fn take_live_external_handle_owned(
        self,
        agent_id: ThreadId,
    ) -> Option<SpawnedAgentHandle> {
        self.external_handles.live.write().await.remove(&agent_id)
    }

    async fn take_archived_external_handle(
        &self,
        agent_id: ThreadId,
    ) -> Option<ArchivedSpawnedAgentHandle> {
        self.external_handles
            .archived
            .write()
            .await
            .remove(&agent_id)
    }

    async fn take_archived_external_handle_owned(
        self,
        agent_id: ThreadId,
    ) -> Option<ArchivedSpawnedAgentHandle> {
        self.external_handles
            .archived
            .write()
            .await
            .remove(&agent_id)
    }

    async fn archive_external_handle_if_supported(&self, handle: &SpawnedAgentHandle) {
        let agent_id = handle.agent_id();
        if let Some(state) = handle.archived_state().await {
            self.external_handles
                .archived
                .write()
                .await
                .insert(agent_id, state);
        } else {
            self.external_handles
                .archived
                .write()
                .await
                .remove(&agent_id);
        }
    }

    async fn archive_external_handle_if_supported_owned(self, handle: SpawnedAgentHandle) {
        let agent_id = handle.agent_id();
        if let Some(state) = handle.archived_state().await {
            self.external_handles
                .archived
                .write()
                .await
                .insert(agent_id, state);
        } else {
            self.external_handles
                .archived
                .write()
                .await
                .remove(&agent_id);
        }
    }

    fn sync_external_turn_completion(&self, agent_id: ThreadId, handle: SpawnedAgentHandle) {
        let control = self.clone();
        tokio::spawn(async move {
            let status = match handle.subscribe_status().await {
                Ok(mut rx) => {
                    let mut current = rx.borrow().clone();
                    while matches!(current, AgentStatus::PendingInit | AgentStatus::Running) {
                        if rx.changed().await.is_err() {
                            current = handle.status().await;
                            break;
                        }
                        current = rx.borrow().clone();
                    }
                    current
                }
                Err(_) => handle.status().await,
            };
            control.record_external_completion(agent_id, status).await;
        });
    }

    async fn record_external_input(&self, agent_id: ThreadId, input_items: &[UserInput]) {
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return;
        };

        let user_text = input_items
            .iter()
            .map(render_user_input_for_external_history)
            .collect::<Vec<_>>()
            .join("\n");
        if user_text.trim().is_empty() {
            return;
        }

        let item = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: user_text }],
            end_turn: None,
            phase: None,
        };
        let turn_context = thread.codex.session.new_default_turn().await;
        thread
            .codex
            .session
            .record_into_history(std::slice::from_ref(&item), turn_context.as_ref())
            .await;
        thread
            .codex
            .session
            .persist_rollout_items(&[RolloutItem::ResponseItem(item)])
            .await;
        thread.codex.session.flush_rollout().await;
    }

    async fn record_external_input_owned(self, agent_id: ThreadId, input_items: Vec<UserInput>) {
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return;
        };

        let user_text = input_items
            .iter()
            .map(render_user_input_for_external_history)
            .collect::<Vec<_>>()
            .join("\n");
        if user_text.trim().is_empty() {
            return;
        }

        let item = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: user_text }],
            end_turn: None,
            phase: None,
        };
        let session = thread.codex.session.clone();
        let turn_context = session.new_default_turn().await;
        session
            .clone()
            .record_into_history_owned(vec![item.clone()], turn_context)
            .await;
        session
            .clone()
            .persist_rollout_items_owned(vec![RolloutItem::ResponseItem(item)])
            .await;
        session.flush_rollout().await;
    }

    async fn record_external_completion(&self, agent_id: ThreadId, status: AgentStatus) {
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return;
        };

        let assistant_text = match status {
            AgentStatus::Completed(Some(message)) if !message.trim().is_empty() => Some(message),
            AgentStatus::Completed(None) => None,
            AgentStatus::Errored(message) if !message.trim().is_empty() => {
                Some(format!("[backend error]\n{message}"))
            }
            _ => None,
        };
        if let Some(message) = assistant_text
            && !thread_last_message_matches(
                &thread.codex.session.clone_history().await,
                "assistant",
                &message,
            )
        {
            let item = ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: message }],
                end_turn: None,
                phase: None,
            };
            let session = thread.codex.session.clone();
            let turn_context = session.new_default_turn().await;
            session
                .clone()
                .record_into_history_owned(vec![item.clone()], turn_context)
                .await;
            session
                .clone()
                .persist_rollout_items_owned(vec![RolloutItem::ResponseItem(item)])
                .await;
        }
        thread.codex.session.flush_rollout().await;
    }

    async fn record_external_completion_owned(self, agent_id: ThreadId, status: AgentStatus) {
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return;
        };

        let assistant_text = match status {
            AgentStatus::Completed(Some(message)) if !message.trim().is_empty() => Some(message),
            AgentStatus::Completed(None) => None,
            AgentStatus::Errored(message) if !message.trim().is_empty() => {
                Some(format!("[backend error]\n{message}"))
            }
            _ => None,
        };
        if let Some(message) = assistant_text
            && !thread_last_message_matches(
                &thread.codex.session.clone_history().await,
                "assistant",
                &message,
            )
        {
            let item = ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: message }],
                end_turn: None,
                phase: None,
            };
            let turn_context = thread.codex.session.new_default_turn().await;
            thread
                .codex
                .session
                .record_into_history(std::slice::from_ref(&item), turn_context.as_ref())
                .await;
            thread
                .codex
                .session
                .persist_rollout_items(&[RolloutItem::ResponseItem(item)])
                .await;
        }
        thread.codex.session.flush_rollout().await;
    }

    /// Spawn a new agent thread and submit the initial prompt.
    pub(crate) async fn spawn_agent(
        &self,
        config: crate::config::Config,
        initial_operation: Op,
        session_source: Option<SessionSource>,
    ) -> CodexResult<ThreadId> {
        Ok(self
            .spawn_agent_internal(
                config,
                initial_operation,
                session_source,
                SpawnAgentOptions::default(),
            )
            .await?
            .thread_id)
    }

    pub(crate) async fn spawn_agent_owned(
        self,
        config: crate::config::Config,
        initial_operation: Op,
        session_source: Option<SessionSource>,
    ) -> CodexResult<ThreadId> {
        Ok(self
            .spawn_agent_internal_owned(
                config,
                initial_operation,
                session_source,
                SpawnAgentOptions::default(),
            )
            .await?
            .thread_id)
    }

    /// Spawn an agent thread with some metadata.
    pub(crate) async fn spawn_agent_with_metadata(
        &self,
        config: crate::config::Config,
        initial_operation: Op,
        session_source: Option<SessionSource>,
        options: SpawnAgentOptions, // TODO(jif) drop with new fork.
    ) -> CodexResult<LiveAgent> {
        self.spawn_agent_internal(config, initial_operation, session_source, options)
            .await
    }

    pub(crate) async fn spawn_agent_with_metadata_owned(
        self,
        config: crate::config::Config,
        initial_operation: Op,
        session_source: Option<SessionSource>,
        options: SpawnAgentOptions,
    ) -> CodexResult<LiveAgent> {
        self.spawn_agent_internal_owned(config, initial_operation, session_source, options)
            .await
    }

    async fn spawn_agent_internal(
        &self,
        config: crate::config::Config,
        initial_operation: Op,
        session_source: Option<SessionSource>,
        options: SpawnAgentOptions,
    ) -> CodexResult<LiveAgent> {
        let state = self.upgrade()?;
        let external_backend_config = config.clone();
        let external_developer_instructions = config.developer_instructions.clone();
        let mut reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;
        let inherited_shell_snapshot = self
            .inherited_shell_snapshot_for_source(&state, session_source.as_ref())
            .await;
        let inherited_exec_policy = self
            .inherited_exec_policy_for_source(&state, session_source.as_ref(), &config)
            .await;
        let (session_source, mut agent_metadata) = match session_source {
            Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth,
                agent_path,
                agent_role,
                ..
            })) => {
                let (session_source, agent_metadata) = self.prepare_thread_spawn(
                    &mut reservation,
                    &config,
                    parent_thread_id,
                    depth,
                    agent_path,
                    agent_role,
                    /*preferred_agent_nickname*/ None,
                )?;
                (Some(session_source), agent_metadata)
            }
            other => (other, AgentMetadata::default()),
        };
        let notification_source = session_source.clone();

        // The same `AgentControl` is sent to spawn the thread.
        let new_thread = match (session_source, options.fork_mode.as_ref()) {
            (Some(session_source), Some(_)) => {
                self.spawn_forked_thread(
                    state.clone(),
                    config,
                    session_source,
                    &options,
                    inherited_shell_snapshot,
                    inherited_exec_policy,
                )
                .await?
            }
            (Some(session_source), None) => {
                state
                    .clone()
                    .spawn_new_thread_with_source_owned(
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
            (None, _) => {
                state
                    .clone()
                    .spawn_new_thread_owned(config, self.clone())
                    .await?
            }
        };
        agent_metadata.agent_id = Some(new_thread.thread_id);
        reservation.commit(agent_metadata.clone());

        if let Some(handle) = SpawnedAgentHandle::from_config(
            Arc::downgrade(&state),
            &external_backend_config,
            new_thread.thread_id,
            new_thread.thread.clone().config_snapshot_owned().await,
            external_developer_instructions,
        )? {
            self.clone().register_external_handle_owned(handle).await;
        }

        let analytics_subagent_source = match notification_source.clone() {
            Some(SessionSource::SubAgent(
                subagent_source @ SubAgentSource::ThreadSpawn {
                    parent_thread_id, ..
                },
            )) => Some((subagent_source, parent_thread_id)),
            _ => None,
        };
        if let Some((subagent_source, parent_thread_id)) = analytics_subagent_source
            && new_thread.thread.enabled(Feature::GeneralAnalytics)
        {
            let client_metadata = match state.get_thread(parent_thread_id).await {
                Ok(_parent_thread) => crate::codex::AppServerClientMetadata::default(),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        parent_thread_id = %parent_thread_id,
                        "skipping subagent thread analytics: failed to load parent thread metadata"
                    );
                    crate::codex::AppServerClientMetadata {
                        client_name: None,
                        client_version: None,
                    }
                }
            };
            let thread_config = new_thread.thread.clone().config_snapshot_owned().await;
            emit_subagent_session_started(
                &new_thread
                    .thread
                    .codex
                    .session
                    .services
                    .analytics_events_client,
                client_metadata,
                new_thread.thread_id,
                /*parent_thread_id*/ None,
                thread_config,
                subagent_source,
            );
        }

        // Notify a new thread has been created. This notification will be processed by clients
        // to subscribe or drain this newly created thread.
        // TODO(jif) add helper for drain
        state.notify_thread_created(new_thread.thread_id);

        self.persist_thread_spawn_edge_for_source(
            new_thread.thread.as_ref(),
            new_thread.thread_id,
            notification_source.as_ref(),
        )
        .await;

        if let Err(err) = self
            .clone()
            .send_input_owned(new_thread.thread_id, initial_operation)
            .await
        {
            let _ = self
                .clone()
                .take_live_external_handle_owned(new_thread.thread_id)
                .await;
            let _ = state
                .clone()
                .remove_thread_owned(new_thread.thread_id)
                .await;
            self.state.release_spawned_thread(new_thread.thread_id);
            return Err(err);
        }
        if !new_thread.thread.enabled(Feature::MultiAgentV2) {
            let child_reference = agent_metadata
                .agent_path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| new_thread.thread_id.to_string());
            self.maybe_start_completion_watcher(
                new_thread.thread_id,
                notification_source,
                child_reference,
                agent_metadata.agent_path.clone(),
            );
        }

        Ok(LiveAgent {
            thread_id: new_thread.thread_id,
            metadata: agent_metadata,
            status: self.get_status(new_thread.thread_id).await,
        })
    }

    async fn spawn_agent_internal_owned(
        self,
        config: crate::config::Config,
        initial_operation: Op,
        session_source: Option<SessionSource>,
        options: SpawnAgentOptions,
    ) -> CodexResult<LiveAgent> {
        let state = self.upgrade()?;
        let external_backend_config = config.clone();
        let external_developer_instructions = config.developer_instructions.clone();
        let mut reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;
        let inherited_shell_snapshot =
            Self::inherited_shell_snapshot_for_source_owned(state.clone(), session_source.clone())
                .await;
        let inherited_exec_policy = Self::inherited_exec_policy_for_source_owned(
            state.clone(),
            session_source.clone(),
            config.clone(),
        )
        .await;
        let (session_source, mut agent_metadata) = match session_source {
            Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth,
                agent_path,
                agent_role,
                ..
            })) => {
                let (session_source, agent_metadata) = self.prepare_thread_spawn(
                    &mut reservation,
                    &config,
                    parent_thread_id,
                    depth,
                    agent_path,
                    agent_role,
                    /*preferred_agent_nickname*/ None,
                )?;
                (Some(session_source), agent_metadata)
            }
            other => (other, AgentMetadata::default()),
        };
        let notification_source = session_source.clone();

        let new_thread = match (session_source, options.fork_mode.as_ref()) {
            (Some(session_source), Some(_)) => tokio::task::spawn_blocking({
                let agent_control = self.clone();
                let state = state.clone();
                let options = options.clone();
                move || {
                    let handle = tokio::runtime::Handle::current();
                    handle.block_on(agent_control.spawn_forked_thread_owned(
                        state,
                        config,
                        session_source,
                        options,
                        inherited_shell_snapshot,
                        inherited_exec_policy,
                    ))
                }
            })
            .await
            .map_err(|err| {
                CodexErr::Fatal(format!("failed to join forked thread spawn task: {err}"))
            })??,
            (Some(session_source), None) => {
                tokio::task::spawn_blocking({
                    let state = state.clone();
                    let agent_control = self.clone();
                    move || {
                        let handle = tokio::runtime::Handle::current();
                        handle.block_on(state.spawn_new_thread_with_source_owned(
                            config,
                            agent_control,
                            session_source,
                            /*persist_extended_history*/ false,
                            /*metrics_service_name*/ None,
                            inherited_shell_snapshot,
                            inherited_exec_policy,
                        ))
                    }
                })
                .await
                .map_err(|err| {
                    CodexErr::Fatal(format!("failed to join spawned thread task: {err}"))
                })??
            }
            (None, _) => {
                require_send_owned(state.clone().spawn_new_thread_owned(config, self.clone()))
                    .await?
            }
        };
        agent_metadata.agent_id = Some(new_thread.thread_id);
        reservation.commit(agent_metadata.clone());

        let new_thread_config_snapshot = new_thread.thread.clone().config_snapshot_owned().await;
        if let Some(handle) = SpawnedAgentHandle::from_config(
            Arc::downgrade(&state),
            &external_backend_config,
            new_thread.thread_id,
            new_thread_config_snapshot,
            external_developer_instructions,
        )? {
            require_send_owned(self.clone().register_external_handle_owned(handle)).await;
        }

        let analytics_notification = if new_thread.thread.enabled(Feature::GeneralAnalytics) {
            match notification_source.clone() {
                Some(SessionSource::SubAgent(
                    subagent_source @ SubAgentSource::ThreadSpawn {
                        parent_thread_id, ..
                    },
                )) => Some((subagent_source, parent_thread_id)),
                _ => None,
            }
        } else {
            None
        };
        if let Some((subagent_source, parent_thread_id)) = analytics_notification {
            let client_metadata = match state.clone().get_thread_owned(parent_thread_id).await {
                Ok(_parent_thread) => crate::codex::AppServerClientMetadata::default(),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        parent_thread_id = %parent_thread_id,
                        "skipping subagent thread analytics: failed to load parent thread metadata"
                    );
                    crate::codex::AppServerClientMetadata {
                        client_name: None,
                        client_version: None,
                    }
                }
            };
            let thread_config = new_thread.thread.clone().config_snapshot_owned().await;
            emit_subagent_session_started(
                &new_thread
                    .thread
                    .codex
                    .session
                    .services
                .analytics_events_client,
                client_metadata,
                new_thread.thread_id,
                Some(parent_thread_id),
                thread_config,
                subagent_source,
            );
        }

        state.notify_thread_created(new_thread.thread_id);

        require_send_owned(Self::persist_thread_spawn_edge_for_source_owned(
            new_thread.thread.state_db(),
            new_thread.thread_id,
            notification_source.clone(),
        ))
        .await;

        if let Err(err) = require_send_owned(
            self.clone()
                .send_input_owned(new_thread.thread_id, initial_operation),
        )
        .await
        {
            let _ = self
                .clone()
                .take_live_external_handle_owned(new_thread.thread_id)
                .await;
            let _ = state
                .clone()
                .remove_thread_owned(new_thread.thread_id)
                .await;
            self.state.release_spawned_thread(new_thread.thread_id);
            return Err(err);
        }
        if !new_thread.thread.enabled(Feature::MultiAgentV2) {
            let child_reference = agent_metadata
                .agent_path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| new_thread.thread_id.to_string());
            self.maybe_start_completion_watcher(
                new_thread.thread_id,
                notification_source,
                child_reference,
                agent_metadata.agent_path.clone(),
            );
        }

        Ok(LiveAgent {
            thread_id: new_thread.thread_id,
            metadata: agent_metadata,
            status: require_send_owned(self.clone().get_status_owned(new_thread.thread_id)).await,
        })
    }

    async fn spawn_forked_thread(
        &self,
        state: Arc<ThreadManagerState>,
        config: crate::config::Config,
        session_source: SessionSource,
        options: &SpawnAgentOptions,
        inherited_shell_snapshot: Option<Arc<ShellSnapshot>>,
        inherited_exec_policy: Option<Arc<crate::exec_policy::ExecPolicyManager>>,
    ) -> CodexResult<crate::thread_manager::NewThread> {
        if options.fork_parent_spawn_call_id.is_none() {
            return Err(CodexErr::Fatal(
                "spawn_agent fork requires a parent spawn call id".to_string(),
            ));
        }
        let Some(fork_mode) = options.fork_mode.as_ref() else {
            return Err(CodexErr::Fatal(
                "spawn_agent fork requires a fork mode".to_string(),
            ));
        };
        let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        }) = &session_source
        else {
            return Err(CodexErr::Fatal(
                "spawn_agent fork requires a thread-spawn session source".to_string(),
            ));
        };

        let parent_thread_id = *parent_thread_id;
        let parent_thread = state.clone().get_thread_owned(parent_thread_id).await.ok();
        let live_rollout_path = parent_thread
            .as_ref()
            .and_then(|thread| thread.rollout_path());
        if let Some(parent_thread) = parent_thread {
            // `record_conversation_items` only queues rollout writes asynchronously.
            // Flush/materialize the live parent before snapshotting JSONL for a fork.
            parent_thread
                .codex
                .session
                .ensure_rollout_materialized()
                .await;
            parent_thread.codex.session.flush_rollout().await?;
        }

        let rollout_path = live_rollout_path
            .or(find_thread_path_by_id_owned(
                config.codex_home.to_path_buf(),
                parent_thread_id.to_string(),
            )
            .await?)
            .ok_or_else(|| {
                CodexErr::Fatal(format!(
                    "parent thread rollout unavailable for fork: {parent_thread_id}"
                ))
            })?;

        let mut forked_rollout_items = RolloutRecorder::get_rollout_history(&rollout_path)
            .await?
            .get_rollout_items();
        if let SpawnAgentForkMode::LastNTurns(last_n_turns) = fork_mode {
            forked_rollout_items =
                truncate_rollout_to_last_n_fork_turns(&forked_rollout_items, *last_n_turns);
        }
        forked_rollout_items.retain(keep_forked_rollout_item);

        state
            .fork_thread_with_source(
                config,
                InitialHistory::Forked(forked_rollout_items),
                self.clone(),
                session_source,
                /*persist_extended_history*/ false,
                inherited_shell_snapshot,
                inherited_exec_policy,
            )
            .await
    }

    async fn spawn_forked_thread_owned(
        self,
        state: Arc<ThreadManagerState>,
        config: crate::config::Config,
        session_source: SessionSource,
        options: SpawnAgentOptions,
        inherited_shell_snapshot: Option<Arc<ShellSnapshot>>,
        inherited_exec_policy: Option<Arc<crate::exec_policy::ExecPolicyManager>>,
    ) -> CodexResult<crate::thread_manager::NewThread> {
        if options.fork_parent_spawn_call_id.is_none() {
            return Err(CodexErr::Fatal(
                "spawn_agent fork requires a parent spawn call id".to_string(),
            ));
        }
        let Some(fork_mode) = options.fork_mode else {
            return Err(CodexErr::Fatal(
                "spawn_agent fork requires a fork mode".to_string(),
            ));
        };
        let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        }) = session_source.clone()
        else {
            return Err(CodexErr::Fatal(
                "spawn_agent fork requires a thread-spawn session source".to_string(),
            ));
        };

        let parent_thread = state.clone().get_thread_owned(parent_thread_id).await.ok();
        let live_rollout_path = parent_thread
            .as_ref()
            .and_then(|thread| thread.rollout_path());
        if let Some(parent_thread) = parent_thread {
            let parent_session = parent_thread.codex.session.clone();
            parent_session
                .clone()
                .ensure_rollout_materialized_owned()
                .await;
            parent_session.flush_rollout_owned().await;
        }

        let rollout_path = live_rollout_path
            .or(find_thread_path_by_id_owned(
                config.codex_home.to_path_buf(),
                parent_thread_id.to_string(),
            )
            .await?)
            .ok_or_else(|| {
                CodexErr::Fatal(format!(
                    "parent thread rollout unavailable for fork: {parent_thread_id}"
                ))
            })?;

        let mut forked_rollout_items = RolloutRecorder::get_rollout_history(&rollout_path)
            .await?
            .get_rollout_items();
        if let SpawnAgentForkMode::LastNTurns(last_n_turns) = fork_mode {
            forked_rollout_items =
                truncate_rollout_to_last_n_fork_turns(&forked_rollout_items, last_n_turns);
        }
        forked_rollout_items.retain(keep_forked_rollout_item);

        state
            .clone()
            .fork_thread_with_source_owned(
                config,
                InitialHistory::Forked(forked_rollout_items),
                self,
                session_source,
                /*persist_extended_history*/ false,
                inherited_shell_snapshot,
                inherited_exec_policy,
            )
            .await
    }

    /// Resume an existing agent thread from a recorded rollout file.
    pub(crate) async fn resume_agent_from_rollout(
        &self,
        config: crate::config::Config,
        thread_id: ThreadId,
        session_source: SessionSource,
    ) -> CodexResult<ThreadId> {
        let root_depth = thread_spawn_depth(&session_source).unwrap_or(0);
        let resumed_thread_id = self
            .resume_single_agent_from_rollout(config.clone(), thread_id, session_source)
            .await?;
        let state = self.upgrade()?;
        let Ok(resumed_thread) = state.clone().get_thread_owned(resumed_thread_id).await else {
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
                let child_resumed = if state
                    .clone()
                    .get_thread_owned(child_thread_id)
                    .await
                    .is_ok()
                {
                    true
                } else {
                    let child_session_source =
                        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                            parent_thread_id,
                            depth: child_depth,
                            agent_path: None,
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

    pub(crate) async fn resume_agent_from_rollout_owned(
        self,
        config: crate::config::Config,
        thread_id: ThreadId,
        session_source: SessionSource,
    ) -> CodexResult<ThreadId> {
        let root_depth = thread_spawn_depth(&session_source).unwrap_or(0);
        let resumed_thread_id = self
            .clone()
            .resume_single_agent_from_rollout_owned(config.clone(), thread_id, session_source)
            .await?;
        let state = self.upgrade()?;
        let Ok(resumed_thread) = state.clone().get_thread_owned(resumed_thread_id).await else {
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
                let child_resumed = if state
                    .clone()
                    .get_thread_owned(child_thread_id)
                    .await
                    .is_ok()
                {
                    true
                } else {
                    let child_session_source =
                        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                            parent_thread_id,
                            depth: child_depth,
                            agent_path: None,
                            agent_nickname: None,
                            agent_role: None,
                        });
                    match self
                        .clone()
                        .resume_single_agent_from_rollout_owned(
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
        let archived_external_handle = self
            .clone()
            .take_archived_external_handle_owned(thread_id)
            .await;
        if let Some(archived) = archived_external_handle.as_ref() {
            apply_archived_spawned_agent_config(&mut config, archived.config_snapshot());
        }
        if let SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) = &session_source
            && *depth >= config.agent_max_depth
        {
            let _ = config.features.disable(Feature::SpawnCsv);
            let _ = config.features.disable(Feature::Collab);
        }
        let state = self.upgrade()?;
        let mut reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;
        let (session_source, agent_metadata) = match session_source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth,
                agent_path,
                agent_role: _,
                agent_nickname: _,
            }) => {
                let (resumed_agent_nickname, resumed_agent_role) =
                    if let Some(state_db_ctx) = get_state_db_owned(config.clone()).await {
                        match state_db_ctx.get_thread(thread_id).await {
                            Ok(Some(metadata)) => (metadata.agent_nickname, metadata.agent_role),
                            Ok(None) | Err(_) => (None, None),
                        }
                    } else {
                        (None, None)
                    };
                self.prepare_thread_spawn(
                    &mut reservation,
                    &config,
                    parent_thread_id,
                    depth,
                    agent_path,
                    resumed_agent_role,
                    resumed_agent_nickname,
                )?
            }
            other => (other, AgentMetadata::default()),
        };
        let notification_source = session_source.clone();
        let inherited_shell_snapshot = self
            .inherited_shell_snapshot_for_source(&state, Some(&session_source))
            .await;
        let inherited_exec_policy = self
            .inherited_exec_policy_for_source(&state, Some(&session_source), &config)
            .await;
        let rollout_path =
            match find_thread_path_by_id_owned(
                config.codex_home.to_path_buf(),
                thread_id.to_string(),
            )
                .await?
            {
                Some(rollout_path) => rollout_path,
                None => find_archived_thread_path_by_id_owned(
                    config.codex_home.to_path_buf(),
                    thread_id.to_string(),
                )
                .await?
                .ok_or_else(|| CodexErr::ThreadNotFound(thread_id))?,
            };

        let resumed_thread = state
            .clone()
            .resume_thread_from_rollout_with_source_owned(
                config,
                rollout_path,
                self.clone(),
                session_source,
                inherited_shell_snapshot,
                inherited_exec_policy,
            )
            .await?;
        let mut agent_metadata = agent_metadata;
        agent_metadata.agent_id = Some(resumed_thread.thread_id);
        reservation.commit(agent_metadata.clone());
        if let Some(archived) = archived_external_handle {
            self.clone()
                .register_external_handle_owned(archived.into_live_handle(Arc::downgrade(&state)))
                .await;
        }
        // Resumed threads are re-registered in-memory and need the same listener
        // attachment path as freshly spawned threads.
        state.notify_thread_created(resumed_thread.thread_id);
        if !resumed_thread.thread.enabled(Feature::MultiAgentV2) {
            let child_reference = agent_metadata
                .agent_path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| resumed_thread.thread_id.to_string());
            self.maybe_start_completion_watcher(
                resumed_thread.thread_id,
                Some(notification_source.clone()),
                child_reference,
                agent_metadata.agent_path.clone(),
            );
        }
        self.persist_thread_spawn_edge_for_source(
            resumed_thread.thread.as_ref(),
            resumed_thread.thread_id,
            Some(&notification_source),
        )
        .await;

        Ok(resumed_thread.thread_id)
    }

    async fn resume_single_agent_from_rollout_owned(
        self,
        mut config: crate::config::Config,
        thread_id: ThreadId,
        session_source: SessionSource,
    ) -> CodexResult<ThreadId> {
        let archived_external_handle = self
            .clone()
            .take_archived_external_handle_owned(thread_id)
            .await;
        if let Some(archived) = archived_external_handle.as_ref() {
            apply_archived_spawned_agent_config(&mut config, archived.config_snapshot());
        }
        if let SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) = &session_source
            && *depth >= config.agent_max_depth
        {
            let _ = config.features.disable(Feature::SpawnCsv);
            let _ = config.features.disable(Feature::Collab);
        }
        let state = self.upgrade()?;
        let mut reservation = self.state.reserve_spawn_slot(config.agent_max_threads)?;
        let (session_source, agent_metadata) = match session_source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth,
                agent_path,
                agent_role: _,
                agent_nickname: _,
            }) => {
                let (resumed_agent_nickname, resumed_agent_role) =
                    if let Some(state_db_ctx) = get_state_db_owned(config.clone()).await {
                        match state_db_ctx.get_thread(thread_id).await {
                            Ok(Some(metadata)) => (metadata.agent_nickname, metadata.agent_role),
                            Ok(None) | Err(_) => (None, None),
                        }
                    } else {
                        (None, None)
                    };
                self.prepare_thread_spawn(
                    &mut reservation,
                    &config,
                    parent_thread_id,
                    depth,
                    agent_path,
                    resumed_agent_role,
                    resumed_agent_nickname,
                )?
            }
            other => (other, AgentMetadata::default()),
        };
        let notification_source = session_source.clone();
        let inherited_shell_snapshot = Self::inherited_shell_snapshot_for_source_owned(
            state.clone(),
            Some(session_source.clone()),
        )
        .await;
        let inherited_exec_policy = Self::inherited_exec_policy_for_source_owned(
            state.clone(),
            Some(session_source.clone()),
            config.clone(),
        )
        .await;
        let rollout_path =
            match find_thread_path_by_id_owned(
                config.codex_home.to_path_buf(),
                thread_id.to_string(),
            )
                .await?
            {
                Some(rollout_path) => rollout_path,
                None => find_archived_thread_path_by_id_owned(
                    config.codex_home.to_path_buf(),
                    thread_id.to_string(),
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
        let mut agent_metadata = agent_metadata;
        agent_metadata.agent_id = Some(resumed_thread.thread_id);
        reservation.commit(agent_metadata.clone());
        if let Some(archived) = archived_external_handle {
            self.clone()
                .register_external_handle_owned(archived.into_live_handle(Arc::downgrade(&state)))
                .await;
        }
        state.notify_thread_created(resumed_thread.thread_id);
        if !resumed_thread.thread.enabled(Feature::MultiAgentV2) {
            let child_reference = agent_metadata
                .agent_path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| resumed_thread.thread_id.to_string());
            self.maybe_start_completion_watcher(
                resumed_thread.thread_id,
                Some(notification_source.clone()),
                child_reference,
                agent_metadata.agent_path.clone(),
            );
        }
        Self::persist_thread_spawn_edge_for_source_owned(
            resumed_thread.thread.state_db(),
            resumed_thread.thread_id,
            Some(notification_source),
        )
        .await;

        Ok(resumed_thread.thread_id)
    }

    /// Send rich user input items to an existing agent thread.
    pub(crate) async fn send_input(
        &self,
        agent_id: ThreadId,
        initial_operation: Op,
    ) -> CodexResult<String> {
        let last_task_message = render_input_preview(&initial_operation);
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            let items = external_items_from_op(&initial_operation)?;
            let result = handle.send_input(items.clone()).await;
            if result.is_ok() {
                self.state
                    .update_last_task_message(agent_id, last_task_message);
                self.clone()
                    .record_external_input_owned(agent_id, items)
                    .await;
                let status = handle.status().await;
                if is_final(&status) {
                    self.clone()
                        .record_external_completion_owned(agent_id, status)
                        .await;
                } else {
                    self.sync_external_turn_completion(agent_id, handle);
                }
            }
            return result;
        }
        let state = self.upgrade()?;
        let result = self
            .handle_thread_request_result(
                agent_id,
                &state,
                state.send_op(agent_id, initial_operation).await,
            )
            .await;
        if result.is_ok() {
            self.state
                .update_last_task_message(agent_id, last_task_message);
        }
        result
    }

    pub(crate) async fn send_input_owned(
        self,
        agent_id: ThreadId,
        initial_operation: Op,
    ) -> CodexResult<String> {
        let last_task_message = render_input_preview(&initial_operation);
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            let items = external_items_from_op(&initial_operation)?;
            let result = handle.send_input(items.clone()).await;
            if result.is_ok() {
                self.state
                    .update_last_task_message(agent_id, last_task_message);
                self.clone()
                    .record_external_input_owned(agent_id, items)
                    .await;
                let status = handle.status().await;
                if is_final(&status) {
                    self.clone()
                        .record_external_completion_owned(agent_id, status)
                        .await;
                } else {
                    self.sync_external_turn_completion(agent_id, handle);
                }
            }
            return result;
        }
        let state = self.upgrade()?;
        let send_result = state
            .clone()
            .send_op_owned(agent_id, initial_operation)
            .await;
        let result = self
            .clone()
            .handle_thread_request_result_owned(agent_id, state.clone(), send_result)
            .await;
        if result.is_ok() {
            self.state
                .update_last_task_message(agent_id, last_task_message);
        }
        result
    }

    /// Append a prebuilt message to an existing agent thread outside the normal user-input path.
    #[cfg(test)]
    pub(crate) async fn append_message(
        &self,
        agent_id: ThreadId,
        message: ResponseItem,
    ) -> CodexResult<String> {
        let state = self.upgrade()?;
        self.handle_thread_request_result(
            agent_id,
            &state,
            state.append_message(agent_id, message).await,
        )
        .await
    }

    pub(crate) async fn send_inter_agent_communication(
        &self,
        agent_id: ThreadId,
        communication: InterAgentCommunication,
    ) -> CodexResult<String> {
        let last_task_message = communication.content.clone();
        if let Some(handle) = self.live_external_handle(agent_id).await {
            let items = vec![UserInput::Text {
                text: communication.content.clone(),
                text_elements: Vec::new(),
            }];
            let result = handle.send_input(items.clone()).await;
            if result.is_ok() {
                self.state
                    .update_last_task_message(agent_id, last_task_message);
                self.record_external_input(agent_id, &items).await;
                let status = handle.status().await;
                if is_final(&status) {
                    self.record_external_completion(agent_id, status).await;
                } else {
                    self.sync_external_turn_completion(agent_id, handle);
                }
            }
            return result;
        }
        let state = self.upgrade()?;
        let result = self
            .handle_thread_request_result(
                agent_id,
                &state,
                state
                    .send_op(agent_id, Op::InterAgentCommunication { communication })
                    .await,
            )
            .await;
        if result.is_ok() {
            self.state
                .update_last_task_message(agent_id, last_task_message);
        }
        result
    }

    /// Interrupt the current task for an existing agent thread.
    pub(crate) async fn interrupt_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        if let Some(handle) = self.live_external_handle(agent_id).await {
            return handle.interrupt().await;
        }
        let state = self.upgrade()?;
        state.send_op(agent_id, Op::Interrupt).await
    }

    async fn handle_thread_request_result(
        &self,
        agent_id: ThreadId,
        state: &Arc<ThreadManagerState>,
        result: CodexResult<String>,
    ) -> CodexResult<String> {
        if matches!(result, Err(CodexErr::InternalAgentDied)) {
            let _ = state.clone().remove_thread_owned(agent_id).await;
            self.state.release_spawned_thread(agent_id);
        }
        result
    }

    async fn handle_thread_request_result_owned(
        self,
        agent_id: ThreadId,
        state: Arc<ThreadManagerState>,
        result: CodexResult<String>,
    ) -> CodexResult<String> {
        if matches!(result, Err(CodexErr::InternalAgentDied)) {
            let _ = state.remove_thread(&agent_id).await;
            self.state.release_spawned_thread(agent_id);
        }
        result
    }

    /// Submit a shutdown request for a live agent without marking it explicitly closed in
    /// persisted spawn-edge state.
    pub(crate) async fn shutdown_live_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.upgrade()?;
        if let Some(handle) = self.clone().take_live_external_handle_owned(agent_id).await {
            self.clone()
                .archive_external_handle_if_supported_owned(handle.clone())
                .await;
            let result = handle.shutdown_live().await;
            let _ = state.remove_thread(&agent_id).await;
            self.state.release_spawned_thread(agent_id);
            return result;
        }
        let result = if let Ok(thread) = state.get_thread(agent_id).await {
            thread.codex.session.ensure_rollout_materialized().await;
            thread.codex.session.flush_rollout().await?;
            if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state.send_op(agent_id, Op::Shutdown {}).await
            }
        } else {
            state.send_op(agent_id, Op::Shutdown {}).await
        };
        let _ = state.remove_thread(&agent_id).await;
        self.state.release_spawned_thread(agent_id);
        result
    }

    pub(crate) async fn shutdown_live_agent_owned(self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.upgrade()?;
        if let Some(handle) = self.clone().take_live_external_handle_owned(agent_id).await {
            self.clone()
                .archive_external_handle_if_supported_owned(handle.clone())
                .await;
            let result = handle.shutdown_live().await;
            let _ = state.clone().remove_thread_owned(agent_id).await;
            self.state.release_spawned_thread(agent_id);
            return result;
        }
        let result = if let Ok(thread) = state.clone().get_thread_owned(agent_id).await {
            let session = thread.codex.session.clone();
            session.ensure_rollout_materialized().await;
            session.flush_rollout().await;
            if matches!(
                thread.clone().agent_status_owned().await,
                AgentStatus::Shutdown
            ) {
                Ok(String::new())
            } else {
                state.clone().send_op_owned(agent_id, Op::Shutdown {}).await
            }
        } else {
            state.clone().send_op_owned(agent_id, Op::Shutdown {}).await
        };
        let _ = state.clone().remove_thread_owned(agent_id).await;
        self.state.release_spawned_thread(agent_id);
        result
    }

    /// Mark `agent_id` as explicitly closed in persisted spawn-edge state, then shut down the
    /// agent and any live descendants reached from the in-memory tree.
    pub(crate) async fn close_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.upgrade()?;
        if let Ok(thread) = state.get_thread(agent_id).await
            && let Some(state_db_ctx) = thread.state_db()
            && let Err(err) = state_db_ctx
                .set_thread_spawn_edge_status(agent_id, DirectionalThreadSpawnEdgeStatus::Closed)
                .await
        {
            warn!("failed to persist thread-spawn edge status for {agent_id}: {err}");
        }
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
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            let status = handle.status().await;
            if is_final(&status) {
                self.clone()
                    .record_external_completion_owned(agent_id, status.clone())
                    .await;
            }
            return status;
        }
        let Ok(state) = self.upgrade() else {
            // No agent available if upgrade fails.
            return AgentStatus::NotFound;
        };
        let Ok(thread) = state.clone().get_thread_owned(agent_id).await else {
            return AgentStatus::NotFound;
        };
        thread.clone().agent_status_owned().await
    }

    pub(crate) async fn get_status_owned(self, agent_id: ThreadId) -> AgentStatus {
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            let status = handle.status().await;
            if is_final(&status) {
                self.clone()
                    .record_external_completion_owned(agent_id, status.clone())
                    .await;
            }
            return status;
        }
        let Ok(state) = self.upgrade() else {
            return AgentStatus::NotFound;
        };
        let Ok(thread) = state.clone().get_thread_owned(agent_id).await else {
            return AgentStatus::NotFound;
        };
        thread.clone().agent_status_owned().await
    }

    pub(crate) fn register_session_root(
        &self,
        current_thread_id: ThreadId,
        current_session_source: &SessionSource,
    ) {
        if thread_spawn_parent_thread_id(current_session_source).is_none() {
            self.state.register_root_thread(current_thread_id);
        }
    }

    pub(crate) fn get_agent_metadata(&self, agent_id: ThreadId) -> Option<AgentMetadata> {
        self.state.agent_metadata_for_thread(agent_id)
    }

    pub(crate) async fn list_live_agent_subtree_thread_ids(
        &self,
        agent_id: ThreadId,
    ) -> CodexResult<Vec<ThreadId>> {
        let mut thread_ids = vec![agent_id];
        thread_ids.extend(self.live_thread_spawn_descendants(agent_id).await?);
        Ok(thread_ids)
    }

    pub(crate) async fn get_agent_config_snapshot(
        &self,
        agent_id: ThreadId,
    ) -> Option<ThreadConfigSnapshot> {
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            return handle.config_snapshot().await;
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.clone().get_thread_owned(agent_id).await else {
            return None;
        };
        Some(thread.clone().config_snapshot_owned().await)
    }

    pub(crate) async fn get_agent_config_snapshot_owned(
        self,
        agent_id: ThreadId,
    ) -> Option<ThreadConfigSnapshot> {
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            return handle.config_snapshot().await;
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.clone().get_thread_owned(agent_id).await else {
            return None;
        };
        Some(thread.clone().config_snapshot_owned().await)
    }

    pub(crate) async fn resolve_agent_reference(
        &self,
        _current_thread_id: ThreadId,
        current_session_source: &SessionSource,
        agent_reference: &str,
    ) -> CodexResult<ThreadId> {
        let current_agent_path = current_session_source
            .get_agent_path()
            .unwrap_or_else(AgentPath::root);
        let agent_path = current_agent_path
            .resolve(agent_reference)
            .map_err(CodexErr::UnsupportedOperation)?;
        if let Some(thread_id) = self.state.agent_id_for_path(&agent_path) {
            return Ok(thread_id);
        }
        Err(CodexErr::UnsupportedOperation(format!(
            "live agent path `{}` not found",
            agent_path.as_str()
        )))
    }

    /// Subscribe to status updates for `agent_id`, yielding the latest value and changes.
    pub(crate) async fn subscribe_status(
        &self,
        agent_id: ThreadId,
    ) -> CodexResult<watch::Receiver<AgentStatus>> {
        if let Some(handle) = self.live_external_handle(agent_id).await {
            return handle.subscribe_status().await;
        }
        let state = self.upgrade()?;
        let thread = state.clone().get_thread_owned(agent_id).await?;
        Ok(thread.subscribe_status())
    }

    pub(crate) async fn subscribe_status_owned(
        self,
        agent_id: ThreadId,
    ) -> CodexResult<watch::Receiver<AgentStatus>> {
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            return handle.subscribe_status().await;
        }
        let state = self.upgrade()?;
        let thread = state.clone().get_thread_owned(agent_id).await?;
        Ok(thread.subscribe_status())
    }

    pub(crate) async fn get_total_token_usage(&self, agent_id: ThreadId) -> Option<TokenUsage> {
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            return handle.total_token_usage().await;
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.clone().get_thread_owned(agent_id).await else {
            return None;
        };
        thread.clone().total_token_usage_owned().await
    }

    pub(crate) async fn get_total_token_usage_owned(
        self,
        agent_id: ThreadId,
    ) -> Option<TokenUsage> {
        if let Some(handle) = self.clone().live_external_handle_owned(agent_id).await {
            return handle.total_token_usage().await;
        }
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.clone().get_thread_owned(agent_id).await else {
            return None;
        };
        thread.clone().total_token_usage_owned().await
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
            .map(|(thread_id, metadata)| {
                let reference = metadata
                    .agent_path
                    .as_ref()
                    .map(|agent_path| agent_path.name().to_string())
                    .unwrap_or_else(|| thread_id.to_string());
                format_subagent_context_line(reference.as_str(), metadata.agent_nickname.as_deref())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) async fn list_agents(
        &self,
        current_session_source: &SessionSource,
        path_prefix: Option<&str>,
    ) -> CodexResult<Vec<ListedAgent>> {
        let resolved_prefix = path_prefix
            .map(|prefix| {
                current_session_source
                    .get_agent_path()
                    .unwrap_or_else(AgentPath::root)
                    .resolve(prefix)
                    .map_err(CodexErr::UnsupportedOperation)
            })
            .transpose()?;

        let mut live_agents = self.state.live_agents();
        live_agents.sort_by(|left, right| {
            left.agent_path
                .as_deref()
                .unwrap_or_default()
                .cmp(right.agent_path.as_deref().unwrap_or_default())
                .then_with(|| {
                    left.agent_id
                        .map(|id| id.to_string())
                        .unwrap_or_default()
                        .cmp(&right.agent_id.map(|id| id.to_string()).unwrap_or_default())
                })
        });

        let root_path = AgentPath::root();
        let mut agents = Vec::with_capacity(live_agents.len().saturating_add(1));
        if resolved_prefix
            .as_ref()
            .is_none_or(|prefix| agent_matches_prefix(Some(&root_path), prefix))
            && let Some(root_thread_id) = self.state.agent_id_for_path(&root_path)
        {
            agents.push(ListedAgent {
                agent_name: root_path.to_string(),
                agent_status: self.get_status(root_thread_id).await,
                last_task_message: Some(ROOT_LAST_TASK_MESSAGE.to_string()),
            });
        }

        for metadata in live_agents {
            let Some(thread_id) = metadata.agent_id else {
                continue;
            };
            if resolved_prefix
                .as_ref()
                .is_some_and(|prefix| !agent_matches_prefix(metadata.agent_path.as_ref(), prefix))
            {
                continue;
            }
            let agent_name = metadata
                .agent_path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| thread_id.to_string());
            let last_task_message = metadata.last_task_message.clone();
            agents.push(ListedAgent {
                agent_name,
                agent_status: self.get_status(thread_id).await,
                last_task_message,
            });
        }

        Ok(agents)
    }

    /// Starts a detached watcher for sub-agents spawned from another thread.
    ///
    /// This is only enabled for `SubAgentSource::ThreadSpawn`, where a parent thread exists and
    /// can receive completion notifications.
    fn maybe_start_completion_watcher(
        &self,
        child_thread_id: ThreadId,
        session_source: Option<SessionSource>,
        child_reference: String,
        child_agent_path: Option<AgentPath>,
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
            let child_thread = state.get_thread(child_thread_id).await.ok();
            let message = format_subagent_notification_message(child_reference.as_str(), &status);
            if child_agent_path.is_some()
                && child_thread
                    .as_ref()
                    .map(|thread| thread.enabled(Feature::MultiAgentV2))
                    .unwrap_or(true)
            {
                let Some(child_agent_path) = child_agent_path.clone() else {
                    return;
                };
                let Some(parent_agent_path) = child_agent_path
                    .as_str()
                    .rsplit_once('/')
                    .and_then(|(parent, _)| AgentPath::try_from(parent).ok())
                else {
                    return;
                };
                let communication = InterAgentCommunication::new(
                    child_agent_path,
                    parent_agent_path,
                    Vec::new(),
                    message,
                    /*trigger_turn*/ false,
                );
                let _ = control
                    .send_inter_agent_communication(parent_thread_id, communication)
                    .await;
                return;
            }
            let Ok(parent_thread) = state.get_thread(parent_thread_id).await else {
                return;
            };
            parent_thread
                .inject_user_message_without_turn(message)
                .await;
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_thread_spawn(
        &self,
        reservation: &mut crate::agent::registry::SpawnReservation,
        config: &crate::config::Config,
        parent_thread_id: ThreadId,
        depth: i32,
        agent_path: Option<AgentPath>,
        agent_role: Option<String>,
        preferred_agent_nickname: Option<String>,
    ) -> CodexResult<(SessionSource, AgentMetadata)> {
        if depth == 1 {
            self.state.register_root_thread(parent_thread_id);
        }
        if let Some(agent_path) = agent_path.as_ref() {
            reservation.reserve_agent_path(agent_path)?;
        }
        let candidate_names = agent_nickname_candidates(config, agent_role.as_deref());
        let candidate_name_refs: Vec<&str> = candidate_names.iter().map(String::as_str).collect();
        let agent_nickname = Some(reservation.reserve_agent_nickname_with_preference(
            &candidate_name_refs,
            preferred_agent_nickname.as_deref(),
        )?);
        let session_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id,
            depth,
            agent_path: agent_path.clone(),
            agent_nickname: agent_nickname.clone(),
            agent_role: agent_role.clone(),
        });
        let agent_metadata = AgentMetadata {
            agent_id: None,
            agent_path,
            agent_nickname,
            agent_role,
            last_task_message: None,
        };
        Ok((session_source, agent_metadata))
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

    async fn inherited_shell_snapshot_for_source_owned(
        state: Arc<ThreadManagerState>,
        session_source: Option<SessionSource>,
    ) -> Option<Arc<ShellSnapshot>> {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return None;
        };

        let parent_thread = state
            .clone()
            .get_thread_owned(parent_thread_id)
            .await
            .ok()?;
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

    async fn inherited_exec_policy_for_source_owned(
        state: Arc<ThreadManagerState>,
        session_source: Option<SessionSource>,
        child_config: crate::config::Config,
    ) -> Option<Arc<crate::exec_policy::ExecPolicyManager>> {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return None;
        };

        let parent_thread = state
            .clone()
            .get_thread_owned(parent_thread_id)
            .await
            .ok()?;
        let parent_config = parent_thread.codex.session.get_config().await;
        if !crate::exec_policy::child_uses_parent_exec_policy(&parent_config, &child_config) {
            return None;
        }

        Some(Arc::clone(
            &parent_thread.codex.session.services.exec_policy,
        ))
    }

    async fn open_thread_spawn_children(
        &self,
        parent_thread_id: ThreadId,
    ) -> CodexResult<Vec<(ThreadId, AgentMetadata)>> {
        let mut children_by_parent = self.live_thread_spawn_children().await?;
        Ok(children_by_parent
            .remove(&parent_thread_id)
            .unwrap_or_default())
    }

    async fn live_thread_spawn_children(
        &self,
    ) -> CodexResult<HashMap<ThreadId, Vec<(ThreadId, AgentMetadata)>>> {
        let state = self.upgrade()?;
        let mut children_by_parent = HashMap::<ThreadId, Vec<(ThreadId, AgentMetadata)>>::new();

        for thread_id in state.list_thread_ids().await {
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
                .push((
                    thread_id,
                    self.state
                        .agent_metadata_for_thread(thread_id)
                        .unwrap_or(AgentMetadata {
                            agent_id: Some(thread_id),
                            ..Default::default()
                        }),
                ));
        }

        for children in children_by_parent.values_mut() {
            children.sort_by(|left, right| {
                left.1
                    .agent_path
                    .as_deref()
                    .unwrap_or_default()
                    .cmp(right.1.agent_path.as_deref().unwrap_or_default())
                    .then_with(|| left.0.to_string().cmp(&right.0.to_string()))
            });
        }

        Ok(children_by_parent)
    }

    async fn persist_thread_spawn_edge_for_source(
        &self,
        thread: &crate::CodexThread,
        child_thread_id: ThreadId,
        session_source: Option<&SessionSource>,
    ) {
        let Some(parent_thread_id) = session_source.and_then(thread_spawn_parent_thread_id) else {
            return;
        };
        let Some(state_db_ctx) = thread.state_db() else {
            return;
        };
        if let Err(err) = state_db_ctx
            .upsert_thread_spawn_edge(
                parent_thread_id,
                child_thread_id,
                DirectionalThreadSpawnEdgeStatus::Open,
            )
            .await
        {
            warn!("failed to persist thread-spawn edge: {err}");
        }
    }

    async fn persist_thread_spawn_edge_for_source_owned(
        state_db_ctx: Option<state_db::StateDbHandle>,
        child_thread_id: ThreadId,
        session_source: Option<SessionSource>,
    ) {
        let Some(parent_thread_id) = session_source
            .as_ref()
            .and_then(thread_spawn_parent_thread_id)
        else {
            return;
        };
        let Some(state_db_ctx) = state_db_ctx else {
            return;
        };
        if let Err(err) = state_db_ctx
            .upsert_thread_spawn_edge(
                parent_thread_id,
                child_thread_id,
                DirectionalThreadSpawnEdgeStatus::Open,
            )
            .await
        {
            warn!("failed to persist thread-spawn edge: {err}");
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

fn agent_matches_prefix(agent_path: Option<&AgentPath>, prefix: &AgentPath) -> bool {
    if prefix.is_root() {
        return true;
    }

    agent_path.is_some_and(|agent_path| {
        agent_path == prefix
            || agent_path
                .as_str()
                .strip_prefix(prefix.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub(crate) fn render_input_preview(initial_operation: &Op) -> String {
    match initial_operation {
        Op::UserInput { items, .. } => items
            .iter()
            .map(render_user_input_for_external_history)
            .collect::<Vec<_>>()
            .join("\n"),
        Op::InterAgentCommunication { communication } => communication.content.clone(),
        _ => String::new(),
    }
}

fn external_items_from_op(initial_operation: &Op) -> CodexResult<Vec<UserInput>> {
    match initial_operation {
        Op::UserInput { items, .. } => Ok(items.clone()),
        Op::InterAgentCommunication { communication } => Ok(vec![UserInput::Text {
            text: communication.content.clone(),
            text_elements: Vec::new(),
        }]),
        other => Err(CodexErr::UnsupportedOperation(format!(
            "external backend does not support op kind '{}'",
            other.kind()
        ))),
    }
}

fn render_user_input_for_external_history(item: &UserInput) -> String {
    match item {
        UserInput::Text { text, .. } => text.clone(),
        UserInput::Image { .. } => "[image]".to_string(),
        UserInput::LocalImage { path } => format!("[local_image:{}]", path.display()),
        UserInput::Skill { name, path } => format!("[skill:${name}]({})", path.display()),
        UserInput::Mention { name, path } => format!("[mention:${name}]({path})"),
        _ => "[input]".to_string(),
    }
}

async fn get_state_db_owned(config: crate::config::Config) -> Option<state_db::StateDbHandle> {
    state_db::get_state_db(&config).await
}

async fn require_send_owned<F>(future: F) -> F::Output
where
    F: Future + Send,
{
    Box::pin(future).await
}

async fn find_thread_path_by_id_owned(
    codex_home: std::path::PathBuf,
    thread_id: String,
) -> CodexResult<Option<std::path::PathBuf>> {
    find_thread_path_by_id_in_subdir_owned(codex_home, SESSIONS_SUBDIR, thread_id).await
}

async fn find_archived_thread_path_by_id_owned(
    codex_home: std::path::PathBuf,
    thread_id: String,
) -> CodexResult<Option<std::path::PathBuf>> {
    find_thread_path_by_id_in_subdir_owned(codex_home, ARCHIVED_SESSIONS_SUBDIR, thread_id).await
}

async fn find_thread_path_by_id_in_subdir_owned(
    codex_home: std::path::PathBuf,
    subdir: &'static str,
    thread_id_str: String,
) -> CodexResult<Option<std::path::PathBuf>> {
    if Uuid::parse_str(&thread_id_str).is_err() {
        return Ok(None);
    }

    let archived_only = match subdir {
        SESSIONS_SUBDIR => Some(false),
        ARCHIVED_SESSIONS_SUBDIR => Some(true),
        _ => None,
    };
    let thread_id = ThreadId::from_string(&thread_id_str).ok();
    let state_db_ctx = state_db::open_if_present_owned(codex_home.clone(), String::new()).await;
    if let Some(thread_id) = thread_id
        && let Some(db_path) = state_db::find_rollout_path_by_id_owned(
            state_db_ctx.clone(),
            thread_id,
            archived_only,
            "find_path_query",
        )
        .await
    {
        if tokio::fs::try_exists(&db_path).await.unwrap_or(false) {
            return Ok(Some(db_path));
        }
        tracing::error!(
            "state db returned stale rollout path for thread {thread_id_str}: {}",
            db_path.display()
        );
        tracing::warn!(
            "state db discrepancy during find_thread_path_by_id_in_subdir_owned: stale_db_path"
        );
    }

    let mut root = codex_home;
    root.push(subdir);
    if !root.exists() {
        return Ok(None);
    }

    #[allow(clippy::unwrap_used)]
    let limit = NonZero::new(1).unwrap();
    let options = codex_file_search::FileSearchOptions {
        limit,
        compute_indices: false,
        respect_gitignore: false,
        ..Default::default()
    };

    let results = codex_file_search::run(
        &thread_id_str,
        vec![root],
        options,
        /*cancel_flag*/ None,
    )
    .map_err(|err| CodexErr::Fatal(format!("file search failed: {err}")))?;

    let found = results.matches.into_iter().next().map(|m| m.full_path());
    if found.is_some() {
        tracing::debug!("state db missing rollout path for thread {thread_id_str}");
        tracing::warn!(
            "state db discrepancy during find_thread_path_by_id_in_subdir_owned: falling_back"
        );
    }

    Ok(found)
}

fn thread_last_message_matches(
    history: &crate::context_manager::ContextManager,
    expected_role: &str,
    expected_text: &str,
) -> bool {
    let Some(ResponseItem::Message { role, content, .. }) = history.raw_items().last() else {
        return false;
    };
    if role != expected_role {
        return false;
    }

    content.iter().any(|content_item| match content_item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => text == expected_text,
        ContentItem::InputImage { .. } => false,
    })
}

fn thread_spawn_depth(session_source: &SessionSource) -> Option<i32> {
    match session_source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) => Some(*depth),
        _ => None,
    }
}
#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
