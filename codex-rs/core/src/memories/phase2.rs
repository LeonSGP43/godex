use crate::agent::AgentStatus;
use crate::agent::status::is_final as is_final_agent_status;
use crate::codex::Session;
use crate::codex::emit_subagent_session_started;
use crate::config::Config;
use crate::memories::metrics;
use crate::memories::phase_two;
use crate::memories::prompts::build_consolidation_prompt;
use crate::memories::storage::rebuild_raw_memories_file_from_memories_owned;
use crate::memories::storage::rollout_summary_file_stem;
use crate::memories::storage::sync_rollout_summaries_from_memories_owned;
use codex_config::Constrained;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::user_input::UserInput;
use codex_state::Stage1Output;
use codex_state::StateRuntime;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::warn;

#[derive(Debug, Clone, Default)]
struct Claim {
    token: String,
    watermark: i64,
    memory_scope_kind: String,
    memory_scope_key: String,
}

#[derive(Debug, Clone, Default)]
struct Counters {
    input: i64,
}

async fn load_phase2_input_selection(
    db: Arc<StateRuntime>,
    max_raw_memories: usize,
    max_unused_days: i64,
    memory_scope_kind: String,
    memory_scope_key: String,
) -> anyhow::Result<codex_state::Phase2InputSelection> {
    db.clone()
        .get_phase2_input_selection_in_scope_owned(
            max_raw_memories,
            max_unused_days,
            memory_scope_kind,
            memory_scope_key,
        )
        .await
        .map_err(anyhow::Error::from)
}

async fn sync_phase2_artifacts(
    root: std::path::PathBuf,
    artifact_memories: Vec<Stage1Output>,
    semantic_index_enabled: bool,
) -> anyhow::Result<()> {
    let artifact_count = artifact_memories.len();
    sync_rollout_summaries_from_memories_owned(
        root.clone(),
        artifact_memories.clone(),
        artifact_count,
        semantic_index_enabled,
    )
    .await?;
    rebuild_raw_memories_file_from_memories_owned(root, artifact_memories, artifact_count).await?;
    Ok(())
}

/// Runs memory phase 2 (aka consolidation) in strict order. The method represents the linear
/// flow of the consolidation phase.
pub(super) async fn run(session: Arc<Session>, config: Arc<Config>) {
    let phase_two_e2e_timer = session
        .services
        .session_telemetry
        .start_timer(metrics::MEMORY_PHASE_TWO_E2E_MS, &[])
        .ok();

    let Some(db) = session.services.state_db.clone() else {
        // This should not happen.
        return;
    };
    let memory_scope_kind = config.memory_scope_kind.clone();
    let memory_scope_key = config.memory_scope_key.clone();
    let root = crate::fork_patch::memory::scoped_artifact_root(
        &config.codex_home,
        &memory_scope_kind,
        &memory_scope_key,
    );
    let max_raw_memories = config.memories.max_raw_memories_for_consolidation;
    let max_unused_days = config.memories.max_unused_days;
    let semantic_index_enabled = config.memories.semantic_index_enabled;

    // 1. Claim the job.
    let claim = match require_send(job::claim(
        session.clone(),
        db.clone(),
        memory_scope_kind.clone(),
        memory_scope_key.clone(),
    ))
    .await
    {
        Ok(claim) => claim,
        Err(e) => {
            session.services.session_telemetry.counter(
                metrics::MEMORY_PHASE_TWO_JOBS,
                /*inc*/ 1,
                &[("status", e)],
            );
            return;
        }
    };

    // 2. Get the config for the agent
    let Some(agent_config) = agent::get_config(config.clone(), &root) else {
        // If we can't get the config, we can't consolidate.
        tracing::error!("failed to get agent config");
        job::failed(
            session.clone(),
            db.clone(),
            claim.clone(),
            "failed_sandbox_policy",
        )
        .await;
        return;
    };

    // 3. Query the memories
    let selection = match require_send(load_phase2_input_selection(
        db.clone(),
        max_raw_memories,
        max_unused_days,
        claim.memory_scope_kind.clone(),
        claim.memory_scope_key.clone(),
    ))
    .await
    {
        Ok(selection) => selection,
        Err(err) => {
            tracing::error!(
                "failed to list stage1 outputs for scoped consolidation: {}",
                err
            );
            job::failed(
                session.clone(),
                db.clone(),
                claim.clone(),
                "failed_load_stage1_outputs",
            )
            .await;
            return;
        }
    };
    let raw_memories = selection.selected.to_vec();
    let artifact_memories = artifact_memories_for_phase2(&selection);
    let new_watermark = get_watermark(claim.watermark, &raw_memories);

    // 4. Update the file system by syncing the raw memories with the one extracted from DB at
    //    step 3
    // [`rollout_summaries/`]
    if let Err(err) = require_send(sync_phase2_artifacts(
        root.clone(),
        artifact_memories.clone(),
        semantic_index_enabled,
    ))
    .await
    {
        tracing::error!("failed syncing local memory artifacts for scoped consolidation: {err}");
        job::failed(
            session.clone(),
            db.clone(),
            claim.clone(),
            "failed_rebuild_raw_memories",
        )
        .await;
        return;
    }
    if raw_memories.is_empty() {
        // We check only after sync of the file system.
        job::succeed(
            session.clone(),
            db.clone(),
            claim.clone(),
            new_watermark,
            Vec::new(),
            "succeeded_no_input",
        )
        .await;
        return;
    }

    // 5. Spawn the agent
    let prompt = agent::get_prompt(&root, &selection);
    let source = SessionSource::SubAgent(SubAgentSource::MemoryConsolidation);
    let agent_control = session.services.agent_control.clone();
    let thread_id = match require_send(agent_control.clone().spawn_agent_owned(
        agent_config,
        prompt.into(),
        Some(source),
    ))
    .await
    {
        Ok(thread_id) => thread_id,
        Err(err) => {
            tracing::error!("failed to spawn scoped memory consolidation agent: {err}");
            job::failed(
                session.clone(),
                db.clone(),
                claim.clone(),
                "failed_spawn_agent",
            )
            .await;
            return;
        }
    };

    if let Some(thread_config) = session
        .services
        .agent_control
        .get_agent_config_snapshot(thread_id)
        .await
    {
        if session.enabled(Feature::GeneralAnalytics) {
            let client_metadata = session.app_server_client_metadata().await;
            emit_subagent_session_started(
                &session.services.analytics_events_client,
                client_metadata,
                thread_id,
                /*parent_thread_id*/ None,
                thread_config,
                SubAgentSource::MemoryConsolidation,
            );
        }
    } else {
        warn!("failed to load memory consolidation thread config for analytics: {thread_id}");
    }

    // 6. Spawn the agent handler.
    agent::handle(
        session.clone(),
        claim,
        new_watermark,
        raw_memories.clone(),
        thread_id,
        phase_two_e2e_timer,
    );

    // 7. Metrics and logs.
    let counters = Counters {
        input: raw_memories.len() as i64,
    };
    emit_metrics(&session, counters);
}

async fn require_send<F>(future: F) -> F::Output
where
    F: Future + Send,
{
    Box::pin(future).await
}

fn artifact_memories_for_phase2(
    selection: &codex_state::Phase2InputSelection,
) -> Vec<Stage1Output> {
    let mut seen = HashSet::new();
    let mut memories = selection.selected.clone();
    for memory in &selection.selected {
        seen.insert(rollout_summary_file_stem(memory));
    }
    for memory in &selection.previous_selected {
        if seen.insert(rollout_summary_file_stem(memory)) {
            memories.push(memory.clone());
        }
    }
    memories
}

mod job {
    use super::*;

    pub(super) async fn claim(
        session: Arc<Session>,
        db: Arc<StateRuntime>,
        memory_scope_kind: String,
        memory_scope_key: String,
    ) -> Result<Claim, &'static str> {
        let worker_id = session.conversation_id.clone();
        let claim = db
            .clone()
            .try_claim_phase2_job_owned(
                memory_scope_kind.clone(),
                memory_scope_key.clone(),
                worker_id,
                phase_two::JOB_LEASE_SECONDS,
            )
            .await
            .map_err(|e| {
                tracing::error!("failed to claim job: {}", e);
                "failed_claim"
            })?;
        let (token, watermark) = match claim {
            codex_state::Phase2JobClaimOutcome::Claimed {
                ownership_token,
                input_watermark,
            } => {
                session.services.session_telemetry.counter(
                    metrics::MEMORY_PHASE_TWO_JOBS,
                    /*inc*/ 1,
                    &[("status", "claimed")],
                );
                (ownership_token, input_watermark)
            }
            codex_state::Phase2JobClaimOutcome::SkippedNotDirty => return Err("skipped_not_dirty"),
            codex_state::Phase2JobClaimOutcome::SkippedRunning => return Err("skipped_running"),
        };

        Ok(Claim {
            token,
            watermark,
            memory_scope_kind,
            memory_scope_key,
        })
    }

    pub(super) async fn failed(
        session: Arc<Session>,
        db: Arc<StateRuntime>,
        claim: Claim,
        reason: &'static str,
    ) {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_TWO_JOBS,
            /*inc*/ 1,
            &[("status", reason)],
        );
        if matches!(
            db.clone()
                .mark_phase2_job_failed_owned(
                    claim.memory_scope_kind.clone(),
                    claim.memory_scope_key.clone(),
                    claim.token.clone(),
                    reason.to_string(),
                    phase_two::JOB_RETRY_DELAY_SECONDS,
                )
                .await,
            Ok(false)
        ) {
            let _ = db
                .clone()
                .mark_phase2_job_failed_if_unowned_owned(
                    claim.memory_scope_kind.clone(),
                    claim.memory_scope_key.clone(),
                    claim.token.clone(),
                    reason.to_string(),
                    phase_two::JOB_RETRY_DELAY_SECONDS,
                )
                .await;
        }
    }

    pub(super) async fn succeed(
        session: Arc<Session>,
        db: Arc<StateRuntime>,
        claim: Claim,
        completion_watermark: i64,
        selected_outputs: Vec<codex_state::Stage1Output>,
        reason: &'static str,
    ) {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_TWO_JOBS,
            /*inc*/ 1,
            &[("status", reason)],
        );
        let _ = db
            .clone()
            .mark_phase2_job_succeeded_owned(
                claim.memory_scope_kind.clone(),
                claim.memory_scope_key.clone(),
                claim.token.clone(),
                completion_watermark,
                selected_outputs,
            )
            .await;
    }
}

mod agent {
    use super::*;

    pub(super) fn get_config(config: Arc<Config>, root: &std::path::Path) -> Option<Config> {
        let mut agent_config = config.as_ref().clone();

        match AbsolutePathBuf::from_absolute_path(root.to_path_buf()) {
            Ok(root) => agent_config.cwd = root,
            Err(err) => {
                warn!(
                    "memory phase-2 consolidation could not set cwd from codex_home {}: {err}",
                    agent_config.codex_home.display()
                );
                return None;
            }
        }
        // Consolidation threads must never feed back into phase-1 memory generation.
        agent_config.memories.generate_memories = false;
        // Approval policy
        agent_config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
        // Consolidation runs as an internal sub-agent and must not recursively delegate.
        let _ = agent_config.features.disable(Feature::SpawnCsv);
        let _ = agent_config.features.disable(Feature::Collab);
        let _ = agent_config.features.disable(Feature::MemoryTool);

        // Sandbox policy
        let mut writable_roots = Vec::new();
        match AbsolutePathBuf::from_absolute_path(agent_config.codex_home.clone()) {
            Ok(codex_home) => writable_roots.push(codex_home),
            Err(err) => warn!(
                "memory phase-2 consolidation could not add codex_home writable root {}: {err}",
                agent_config.codex_home.display()
            ),
        }
        // The consolidation agent only needs local codex_home write access and no network.
        let consolidation_sandbox_policy = SandboxPolicy::WorkspaceWrite {
            writable_roots,
            read_only_access: Default::default(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
        agent_config
            .permissions
            .sandbox_policy
            .set(consolidation_sandbox_policy)
            .ok()?;

        agent_config.model = Some(
            config
                .memories
                .consolidation_model
                .clone()
                .unwrap_or(phase_two::MODEL.to_string()),
        );
        agent_config.model_reasoning_effort = Some(phase_two::REASONING_EFFORT);

        Some(agent_config)
    }

    pub(super) fn get_prompt(
        root: &std::path::Path,
        selection: &codex_state::Phase2InputSelection,
    ) -> Vec<UserInput> {
        let prompt = build_consolidation_prompt(root, selection);
        vec![UserInput::Text {
            text: prompt,
            text_elements: vec![],
        }]
    }

    /// Handle the agent while it is running.
    pub(super) fn handle(
        session: Arc<Session>,
        claim: Claim,
        new_watermark: i64,
        selected_outputs: Vec<codex_state::Stage1Output>,
        thread_id: ThreadId,
        phase_two_e2e_timer: Option<codex_otel::Timer>,
    ) {
        let Some(db) = session.services.state_db.clone() else {
            return;
        };

        tokio::spawn(async move {
            let _phase_two_e2e_timer = phase_two_e2e_timer;
            let agent_control = session.services.agent_control.clone();

            // TODO(jif) we might have a very small race here.
            let rx = match agent_control
                .clone()
                .subscribe_status_owned(thread_id)
                .await
            {
                Ok(rx) => rx,
                Err(err) => {
                    tracing::error!("agent_control.subscribe_status failed: {err:?}");
                    job::failed(
                        session.clone(),
                        db.clone(),
                        claim.clone(),
                        "failed_subscribe_status",
                    )
                    .await;
                    return;
                }
            };

            // Loop the agent until we have the final status.
            let final_status = loop_agent(
                db.clone(),
                claim.token.clone(),
                claim.memory_scope_kind.clone(),
                claim.memory_scope_key.clone(),
                new_watermark,
                thread_id,
                rx,
            )
            .await;

            if matches!(final_status, AgentStatus::Completed(_)) {
                if let Some(token_usage) = agent_control
                    .clone()
                    .get_total_token_usage_owned(thread_id)
                    .await
                {
                    emit_token_usage_metrics(&session, &token_usage);
                }
                job::succeed(
                    session.clone(),
                    db.clone(),
                    claim.clone(),
                    new_watermark,
                    selected_outputs,
                    "succeeded",
                )
                .await;
            } else {
                job::failed(session.clone(), db.clone(), claim.clone(), "failed_agent").await;
            }

            // Fire and forget close of the agent.
            if !matches!(final_status, AgentStatus::Shutdown | AgentStatus::NotFound) {
                tokio::spawn(async move {
                    if let Err(err) = agent_control.shutdown_live_agent_owned(thread_id).await {
                        warn!(
                            "failed to auto-close scoped memory consolidation agent {thread_id}: {err}"
                        );
                    }
                });
            } else {
                tracing::warn!("The agent was already gone");
            }
        });
    }

    async fn loop_agent(
        db: Arc<StateRuntime>,
        token: String,
        memory_scope_kind: String,
        memory_scope_key: String,
        _new_watermark: i64,
        thread_id: ThreadId,
        mut rx: watch::Receiver<AgentStatus>,
    ) -> AgentStatus {
        let mut heartbeat_interval =
            tokio::time::interval(Duration::from_secs(phase_two::JOB_HEARTBEAT_SECONDS));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let status = rx.borrow().clone();
            if is_final_agent_status(&status) {
                break status;
            }

            tokio::select! {
                update = rx.changed() => {
                    if update.is_err() {
                        tracing::warn!(
                            "lost status updates for global memory consolidation agent {thread_id}"
                        );
                        break status;
                    }
                }
                _ = heartbeat_interval.tick() => {
                    match db
                        .clone()
                        .heartbeat_phase2_job_owned(
                            memory_scope_kind.clone(),
                            memory_scope_key.clone(),
                            token.clone(),
                            phase_two::JOB_LEASE_SECONDS,
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            break AgentStatus::Errored(
                                "lost scoped phase-2 ownership during heartbeat".to_string(),
                            );
                        }
                        Err(err) => {
                            break AgentStatus::Errored(format!(
                                "phase-2 heartbeat update failed: {err}"
                            ));
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn get_watermark(
    claimed_watermark: i64,
    latest_memories: &[codex_state::Stage1Output],
) -> i64 {
    latest_memories
        .iter()
        .map(|memory| memory.source_updated_at.timestamp())
        .max()
        .unwrap_or(claimed_watermark)
        .max(claimed_watermark) // todo double check the claimed here.
}

fn emit_metrics(session: &Arc<Session>, counters: Counters) {
    let otel = session.services.session_telemetry.clone();
    if counters.input > 0 {
        otel.counter(metrics::MEMORY_PHASE_TWO_INPUT, counters.input, &[]);
    }

    otel.counter(
        metrics::MEMORY_PHASE_TWO_JOBS,
        /*inc*/ 1,
        &[("status", "agent_spawned")],
    );
}

fn emit_token_usage_metrics(session: &Arc<Session>, token_usage: &TokenUsage) {
    let otel = session.services.session_telemetry.clone();
    otel.histogram(
        metrics::MEMORY_PHASE_TWO_TOKEN_USAGE,
        token_usage.total_tokens.max(0),
        &[("token_type", "total")],
    );
    otel.histogram(
        metrics::MEMORY_PHASE_TWO_TOKEN_USAGE,
        token_usage.input_tokens.max(0),
        &[("token_type", "input")],
    );
    otel.histogram(
        metrics::MEMORY_PHASE_TWO_TOKEN_USAGE,
        token_usage.cached_input(),
        &[("token_type", "cached_input")],
    );
    otel.histogram(
        metrics::MEMORY_PHASE_TWO_TOKEN_USAGE,
        token_usage.output_tokens.max(0),
        &[("token_type", "output")],
    );
    otel.histogram(
        metrics::MEMORY_PHASE_TWO_TOKEN_USAGE,
        token_usage.reasoning_output_tokens.max(0),
        &[("token_type", "reasoning_output")],
    );
}
