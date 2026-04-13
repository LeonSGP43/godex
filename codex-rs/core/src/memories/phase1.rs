use crate::Prompt;
use crate::RolloutRecorder;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::config::Config;
use crate::contextual_user_message::is_memory_excluded_contextual_user_fragment;
use crate::memories::metrics;
use crate::memories::phase_one;
use crate::memories::phase_one::PRUNE_BATCH_SIZE;
use crate::memories::prompts::build_stage_one_input_message;
use crate::rollout::INTERACTIVE_SESSION_SOURCES;
use crate::rollout::policy::should_persist_response_item_for_memories;
use codex_api::ResponseEvent;
use codex_otel::SessionTelemetry;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::config_types::ServiceTier;
use codex_protocol::error::CodexErr;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::TokenUsage;
use codex_secrets::redact_secrets;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use tracing::info;
use tracing::warn;

#[derive(Clone, Debug)]
pub(in crate::memories) struct RequestContext {
    pub(in crate::memories) model_info: ModelInfo,
    pub(in crate::memories) session_telemetry: SessionTelemetry,
    pub(in crate::memories) reasoning_effort: Option<ReasoningEffortConfig>,
    pub(in crate::memories) reasoning_summary: ReasoningSummaryConfig,
    pub(in crate::memories) service_tier: Option<ServiceTier>,
    pub(in crate::memories) turn_metadata_header: Option<String>,
}

struct JobResult {
    outcome: JobOutcome,
    token_usage: Option<TokenUsage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JobOutcome {
    SucceededWithOutput,
    SucceededNoOutput,
    Failed,
}

struct Stats {
    claimed: usize,
    succeeded_with_output: usize,
    succeeded_no_output: usize,
    failed: usize,
    total_token_usage: Option<TokenUsage>,
}

/// Phase 1 model output payload.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct StageOneOutput {
    /// Detailed markdown raw memory for a single rollout.
    #[serde(rename = "raw_memory")]
    pub(crate) raw_memory: String,
    /// Compact summary line used for routing and indexing.
    #[serde(rename = "rollout_summary")]
    pub(crate) rollout_summary: String,
    /// Optional slug used to derive rollout summary artifact filenames.
    #[serde(default, rename = "rollout_slug")]
    pub(crate) rollout_slug: Option<String>,
}

/// Runs memory phase 1 in strict step order:
/// 1) claim eligible rollout jobs
/// 2) build one stage-1 request context
/// 3) run stage-1 extraction jobs in parallel
/// 4) emit metrics and logs
pub(in crate::memories) async fn run(session: Arc<Session>, config: Arc<Config>) {
    let _phase_one_e2e_timer = session
        .services
        .session_telemetry
        .start_timer(metrics::MEMORY_PHASE_ONE_E2E_MS, &[])
        .ok();

    // 1. Claim startup job.
    let Some(claimed_candidates) =
        claim_startup_jobs(Arc::clone(&session), Arc::clone(&config)).await
    else {
        return;
    };
    if claimed_candidates.is_empty() {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_ONE_JOBS,
            /*inc*/ 1,
            &[("status", "skipped_no_candidates")],
        );
        return;
    }

    // 2. Build request.
    let stage_one_context = build_request_context(Arc::clone(&session), Arc::clone(&config)).await;

    // 3. Run the parallel sampling.
    let outcomes = run_jobs(Arc::clone(&session), claimed_candidates, stage_one_context).await;

    // 4. Metrics and logs.
    let counts = aggregate_stats(outcomes);
    emit_metrics(session.as_ref(), &counts);
    info!(
        "memory stage-1 extraction complete: {} job(s) claimed, {} succeeded ({} with output, {} no output), {} failed",
        counts.claimed,
        counts.succeeded_with_output + counts.succeeded_no_output,
        counts.succeeded_with_output,
        counts.succeeded_no_output,
        counts.failed
    );
}

/// Prune old un-used "dead" raw memories.
pub(in crate::memories) async fn prune(session: Arc<Session>, config: Arc<Config>) {
    let Some(db) = session.services.state_db.clone() else {
        return;
    };
    let max_unused_days = config.memories.max_unused_days;
    let memory_scope_kind = config.memory_scope_kind.clone();
    let memory_scope_key = config.memory_scope_key.clone();
    match db
        .clone()
        .prune_stage1_outputs_for_retention_in_scope_owned(
            memory_scope_kind,
            memory_scope_key,
            max_unused_days,
            PRUNE_BATCH_SIZE,
        )
        .await
    {
        Ok(pruned) => {
            if pruned > 0 {
                info!(
                    "memory startup pruned {pruned} stale stage-1 output row(s) older than {max_unused_days} days"
                );
            }
        }
        Err(err) => {
            warn!(
                "state db prune_stage1_outputs_for_retention failed during memories startup: {err}"
            );
        }
    }
}

/// JSON schema used to constrain phase-1 model output.
pub fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "rollout_summary": { "type": "string" },
            "rollout_slug": { "type": ["string", "null"] },
            "raw_memory": { "type": "string" }
        },
        "required": ["rollout_summary", "rollout_slug", "raw_memory"],
        "additionalProperties": false
    })
}

impl RequestContext {
    pub(in crate::memories) fn from_turn_context(
        turn_context: &TurnContext,
        turn_metadata_header: Option<String>,
        model_info: ModelInfo,
    ) -> Self {
        Self {
            model_info,
            turn_metadata_header,
            session_telemetry: turn_context.session_telemetry.clone(),
            reasoning_effort: Some(phase_one::REASONING_EFFORT),
            reasoning_summary: turn_context.reasoning_summary,
            service_tier: turn_context.config.service_tier,
        }
    }
}

async fn get_model_info_owned(
    session: Arc<Session>,
    model_name: String,
    models_manager_config: codex_models_manager::ModelsManagerConfig,
) -> ModelInfo {
    session
        .services
        .models_manager
        .clone()
        .get_model_info_owned(model_name, models_manager_config)
        .await
}

async fn claim_startup_jobs(
    session: Arc<Session>,
    config: Arc<Config>,
) -> Option<Vec<codex_state::Stage1JobClaim>> {
    let Some(state_db) = session.services.state_db.clone() else {
        // This should not happen.
        warn!("state db unavailable while claiming phase-1 startup jobs; skipping");
        return None;
    };

    let allowed_sources = INTERACTIVE_SESSION_SOURCES
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    match state_db
        .clone()
        .claim_stage1_jobs_for_startup_in_scope_owned(
            session.conversation_id,
            phase_one::THREAD_SCAN_LIMIT,
            config.memories.max_rollouts_per_startup,
            config.memories.max_rollout_age_days,
            config.memories.min_rollout_idle_hours,
            allowed_sources,
            phase_one::JOB_LEASE_SECONDS,
            config.memory_scope_kind.clone(),
            config.memory_scope_key.clone(),
        )
        .await
    {
        Ok(claims) => Some(claims),
        Err(err) => {
            warn!("state db claim_stage1_jobs_for_startup failed during memories startup: {err}");
            session.services.session_telemetry.counter(
                metrics::MEMORY_PHASE_ONE_JOBS,
                /*inc*/ 1,
                &[("status", "failed_claim")],
            );
            None
        }
    }
}

async fn build_request_context(session: Arc<Session>, config: Arc<Config>) -> RequestContext {
    let model_name = config
        .memories
        .extract_model
        .clone()
        .unwrap_or(phase_one::MODEL.to_string());
    let models_manager_config = config.to_models_manager_config();
    let model = get_model_info_owned(Arc::clone(&session), model_name, models_manager_config).await;
    let turn_context = session.new_default_turn().await;
    RequestContext::from_turn_context(
        turn_context.as_ref(),
        turn_context.turn_metadata_state.current_header_value(),
        model,
    )
}

async fn run_jobs(
    session: Arc<Session>,
    claimed_candidates: Vec<codex_state::Stage1JobClaim>,
    stage_one_context: RequestContext,
) -> Vec<JobResult> {
    futures::stream::iter(claimed_candidates.into_iter())
        .map(move |claim| {
            let session = Arc::clone(&session);
            let stage_one_context = stage_one_context.clone();
            async move { job::run(session, claim, stage_one_context).await }
        })
        .buffer_unordered(phase_one::CONCURRENCY_LIMIT)
        .collect::<Vec<_>>()
        .await
}

mod job {
    use super::*;

    pub(in crate::memories) async fn run(
        session: Arc<Session>,
        claim: codex_state::Stage1JobClaim,
        stage_one_context: RequestContext,
    ) -> JobResult {
        let thread = claim.thread;
        let RequestContext {
            model_info,
            session_telemetry,
            reasoning_effort,
            reasoning_summary,
            service_tier,
            turn_metadata_header,
        } = stage_one_context;
        let (stage_one_output, token_usage) = match sample(
            Arc::clone(&session),
            thread.rollout_path.clone(),
            thread.cwd.clone(),
            model_info,
            session_telemetry,
            reasoning_effort,
            reasoning_summary,
            service_tier,
            turn_metadata_header,
        )
        .await
        {
            Ok(output) => output,
            Err(reason) => {
                result::failed(
                    Arc::clone(&session),
                    thread.id,
                    claim.ownership_token.clone(),
                    reason.to_string(),
                )
                .await;
                return JobResult {
                    outcome: JobOutcome::Failed,
                    token_usage: None,
                };
            }
        };

        if stage_one_output.raw_memory.is_empty() || stage_one_output.rollout_summary.is_empty() {
            return JobResult {
                outcome: result::no_output(
                    Arc::clone(&session),
                    thread.id,
                    claim.ownership_token.clone(),
                )
                .await,
                token_usage,
            };
        }

        JobResult {
            outcome: result::success(
                session,
                thread.id,
                claim.ownership_token,
                thread.updated_at.timestamp(),
                stage_one_output.raw_memory,
                stage_one_output.rollout_summary,
                stage_one_output.rollout_slug,
            )
            .await,
            token_usage,
        }
    }

    /// Extract the rollout and perform the actual sampling.
    async fn sample(
        session: Arc<Session>,
        rollout_path: std::path::PathBuf,
        rollout_cwd: std::path::PathBuf,
        model_info: ModelInfo,
        session_telemetry: SessionTelemetry,
        reasoning_effort: Option<ReasoningEffortConfig>,
        reasoning_summary: ReasoningSummaryConfig,
        service_tier: Option<ServiceTier>,
        _turn_metadata_header: Option<String>,
    ) -> anyhow::Result<(StageOneOutput, Option<TokenUsage>)> {
        let (rollout_items, _, _) = RolloutRecorder::load_rollout_items(&rollout_path).await?;
        let rollout_contents = serialize_filtered_rollout_response_items(&rollout_items)?;

        let prompt = Prompt {
            input: vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: build_stage_one_input_message(
                        &model_info,
                        &rollout_path,
                        &rollout_cwd,
                        &rollout_contents,
                    )?,
                }],
                end_turn: None,
                phase: None,
            }],
            tools: Vec::new(),
            parallel_tool_calls: false,
            base_instructions: BaseInstructions {
                text: phase_one::PROMPT.to_string(),
            },
            personality: None,
            output_schema: Some(output_schema()),
        };

        let mut client_session = session.services.model_client.new_session();
        let mut stream = client_session
            .stream(
                &prompt,
                &model_info,
                &session_telemetry,
                reasoning_effort,
                reasoning_summary,
                service_tier,
                None,
            )
            .await?;

        // TODO(jif) we should have a shared helper somewhere for this.
        // Unwrap the stream.
        let mut result = String::new();
        let mut token_usage = None;
        while let Some(message) = stream.next().await.transpose()? {
            match message {
                ResponseEvent::OutputTextDelta(delta) => result.push_str(&delta),
                ResponseEvent::OutputItemDone(item) => {
                    if result.is_empty()
                        && let ResponseItem::Message { content, .. } = item
                        && let Some(text) = crate::compact::content_items_to_text(&content)
                    {
                        result.push_str(&text);
                    }
                }
                ResponseEvent::Completed {
                    token_usage: usage, ..
                } => {
                    token_usage = usage;
                    break;
                }
                _ => {}
            }
        }

        let mut output: StageOneOutput = serde_json::from_str(&result)?;
        output.raw_memory = redact_secrets(output.raw_memory);
        output.rollout_summary = redact_secrets(output.rollout_summary);
        output.rollout_slug = output.rollout_slug.map(redact_secrets);

        Ok((output, token_usage))
    }

    mod result {
        use super::*;

        pub(in crate::memories) async fn failed(
            session: Arc<Session>,
            thread_id: codex_protocol::ThreadId,
            ownership_token: String,
            reason: String,
        ) {
            tracing::warn!("Phase 1 job failed for thread {thread_id}: {reason}");
            let Some(state_db) = session.services.state_db.clone() else {
                return;
            };
            let _ = state_db
                .clone()
                .mark_stage1_job_failed_owned(
                    thread_id,
                    ownership_token,
                    reason,
                    phase_one::JOB_RETRY_DELAY_SECONDS,
                )
                .await;
        }

        pub(in crate::memories) async fn no_output(
            session: Arc<Session>,
            thread_id: codex_protocol::ThreadId,
            ownership_token: String,
        ) -> JobOutcome {
            let Some(state_db) = session.services.state_db.clone() else {
                return JobOutcome::Failed;
            };

            if state_db
                .clone()
                .mark_stage1_job_succeeded_no_output_owned(thread_id, ownership_token)
                .await
                .unwrap_or(false)
            {
                JobOutcome::SucceededNoOutput
            } else {
                JobOutcome::Failed
            }
        }

        pub(in crate::memories) async fn success(
            session: Arc<Session>,
            thread_id: codex_protocol::ThreadId,
            ownership_token: String,
            source_updated_at: i64,
            raw_memory: String,
            rollout_summary: String,
            rollout_slug: Option<String>,
        ) -> JobOutcome {
            let Some(state_db) = session.services.state_db.clone() else {
                return JobOutcome::Failed;
            };

            if state_db
                .clone()
                .mark_stage1_job_succeeded_owned(
                    thread_id,
                    ownership_token,
                    source_updated_at,
                    raw_memory,
                    rollout_summary,
                    rollout_slug,
                )
                .await
                .unwrap_or(false)
            {
                JobOutcome::SucceededWithOutput
            } else {
                JobOutcome::Failed
            }
        }
    }

    /// Serializes filtered stage-1 memory items for prompt inclusion.
    pub(super) fn serialize_filtered_rollout_response_items(
        items: &[RolloutItem],
    ) -> codex_protocol::error::Result<String> {
        let filtered = items
            .iter()
            .filter_map(|item| {
                if let RolloutItem::ResponseItem(item) = item {
                    sanitize_response_item_for_memories(item)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&filtered).map_err(|err| {
            CodexErr::InvalidRequest(format!("failed to serialize rollout memory: {err}"))
        })
    }

    fn sanitize_response_item_for_memories(item: &ResponseItem) -> Option<ResponseItem> {
        let ResponseItem::Message {
            id,
            role,
            content,
            end_turn,
            phase,
        } = item
        else {
            return should_persist_response_item_for_memories(item).then(|| item.clone());
        };

        if role == "developer" {
            return None;
        }

        if role != "user" {
            return Some(item.clone());
        }

        let content = content
            .iter()
            .filter(|content_item| !is_memory_excluded_contextual_user_fragment(content_item))
            .cloned()
            .collect::<Vec<_>>();
        if content.is_empty() {
            return None;
        }

        Some(ResponseItem::Message {
            id: id.clone(),
            role: role.clone(),
            content,
            end_turn: *end_turn,
            phase: phase.clone(),
        })
    }
}

fn aggregate_stats(outcomes: Vec<JobResult>) -> Stats {
    let claimed = outcomes.len();
    let mut succeeded_with_output = 0;
    let mut succeeded_no_output = 0;
    let mut failed = 0;
    let mut total_token_usage = TokenUsage::default();
    let mut has_token_usage = false;

    for outcome in outcomes {
        match outcome.outcome {
            JobOutcome::SucceededWithOutput => succeeded_with_output += 1,
            JobOutcome::SucceededNoOutput => succeeded_no_output += 1,
            JobOutcome::Failed => failed += 1,
        }

        if let Some(token_usage) = outcome.token_usage {
            total_token_usage.add_assign(&token_usage);
            has_token_usage = true;
        }
    }

    Stats {
        claimed,
        succeeded_with_output,
        succeeded_no_output,
        failed,
        total_token_usage: has_token_usage.then_some(total_token_usage),
    }
}

fn emit_metrics(session: &Session, counts: &Stats) {
    if counts.claimed > 0 {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_ONE_JOBS,
            counts.claimed as i64,
            &[("status", "claimed")],
        );
    }
    if counts.succeeded_with_output > 0 {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_ONE_JOBS,
            counts.succeeded_with_output as i64,
            &[("status", "succeeded")],
        );
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_ONE_OUTPUT,
            counts.succeeded_with_output as i64,
            &[],
        );
    }
    if counts.succeeded_no_output > 0 {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_ONE_JOBS,
            counts.succeeded_no_output as i64,
            &[("status", "succeeded_no_output")],
        );
    }
    if counts.failed > 0 {
        session.services.session_telemetry.counter(
            metrics::MEMORY_PHASE_ONE_JOBS,
            counts.failed as i64,
            &[("status", "failed")],
        );
    }
    if let Some(token_usage) = counts.total_token_usage.as_ref() {
        session.services.session_telemetry.histogram(
            metrics::MEMORY_PHASE_ONE_TOKEN_USAGE,
            token_usage.total_tokens.max(0),
            &[("token_type", "total")],
        );
        session.services.session_telemetry.histogram(
            metrics::MEMORY_PHASE_ONE_TOKEN_USAGE,
            token_usage.input_tokens.max(0),
            &[("token_type", "input")],
        );
        session.services.session_telemetry.histogram(
            metrics::MEMORY_PHASE_ONE_TOKEN_USAGE,
            token_usage.cached_input(),
            &[("token_type", "cached_input")],
        );
        session.services.session_telemetry.histogram(
            metrics::MEMORY_PHASE_ONE_TOKEN_USAGE,
            token_usage.output_tokens.max(0),
            &[("token_type", "output")],
        );
        session.services.session_telemetry.histogram(
            metrics::MEMORY_PHASE_ONE_TOKEN_USAGE,
            token_usage.reasoning_output_tokens.max(0),
            &[("token_type", "reasoning_output")],
        );
    }
}

#[cfg(test)]
#[path = "phase1_tests.rs"]
mod tests;
