use crate::agent::exceeds_thread_spawn_depth_limit;
use crate::agent::next_thread_spawn_depth;
use crate::agent::status::is_final;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::config::Config;
use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::multi_agents::build_agent_spawn_config_owned;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch::Receiver;
use tokio::time::Duration;
use tokio::time::Instant;
use tokio::time::timeout;
use uuid::Uuid;

pub struct BatchJobHandler;

const DEFAULT_AGENT_JOB_CONCURRENCY: usize = 16;
const MAX_AGENT_JOB_CONCURRENCY: usize = 64;
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_AGENT_JOB_ITEM_TIMEOUT: Duration = Duration::from_secs(60 * 30);

enum BatchJobTool {
    SpawnAgentsOnCsv,
    ReportAgentJobResult,
}

#[derive(Debug, Deserialize)]
struct SpawnAgentsOnCsvArgs {
    csv_path: String,
    instruction: String,
    id_column: Option<String>,
    output_csv_path: Option<String>,
    output_schema: Option<Value>,
    max_concurrency: Option<usize>,
    max_workers: Option<usize>,
    max_runtime_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ReportAgentJobResultArgs {
    job_id: String,
    item_id: String,
    result: Value,
    stop: Option<bool>,
}

#[derive(Debug, Serialize)]
struct SpawnAgentsOnCsvResult {
    job_id: String,
    status: String,
    output_csv_path: String,
    total_items: usize,
    completed_items: usize,
    failed_items: usize,
    job_error: Option<String>,
    failed_item_errors: Option<Vec<AgentJobFailureSummary>>,
}

#[derive(Debug, Serialize)]
struct AgentJobFailureSummary {
    item_id: String,
    source_id: Option<String>,
    last_error: String,
}

#[derive(Debug, Serialize)]
struct AgentJobProgressUpdate {
    job_id: String,
    total_items: usize,
    pending_items: usize,
    running_items: usize,
    completed_items: usize,
    failed_items: usize,
    eta_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ReportAgentJobResultToolResult {
    accepted: bool,
}

#[derive(Debug, Clone)]
struct JobRunnerOptions {
    max_concurrency: usize,
    spawn_config: Config,
}

#[derive(Debug, Clone)]
struct ActiveJobItem {
    item_id: String,
    started_at: Instant,
    status_rx: Option<Receiver<AgentStatus>>,
}

struct JobProgressEmitter {
    started_at: Instant,
    last_emit_at: Instant,
    last_processed: usize,
    last_failed: usize,
}

struct PreparedSpawnAgentsJob {
    job_id: String,
    job_name: String,
    instruction: String,
    output_schema_json: Option<Value>,
    headers: Vec<String>,
    items: Vec<codex_state::AgentJobItemCreateParams>,
    input_csv_path: String,
    output_csv_path: AbsolutePathBuf,
    max_runtime_seconds: Option<u64>,
    requested_concurrency: Option<usize>,
}

impl JobProgressEmitter {
    fn new() -> Self {
        let now = Instant::now();
        let last_emit_at = now.checked_sub(PROGRESS_EMIT_INTERVAL).unwrap_or(now);
        Self {
            started_at: now,
            last_emit_at,
            last_processed: 0,
            last_failed: 0,
        }
    }

    async fn maybe_emit(
        &mut self,
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        job_id: String,
        progress: codex_state::AgentJobProgress,
        force: bool,
    ) -> anyhow::Result<()> {
        let processed = progress.completed_items + progress.failed_items;
        let should_emit = force
            || processed != self.last_processed
            || progress.failed_items != self.last_failed
            || self.last_emit_at.elapsed() >= PROGRESS_EMIT_INTERVAL;
        if !should_emit {
            return Ok(());
        }
        let elapsed = self.started_at.elapsed().as_secs_f64();
        let eta_seconds = if processed > 0 && elapsed > 0.0 {
            let remaining = progress.total_items.saturating_sub(processed) as f64;
            let rate = processed as f64 / elapsed;
            if rate > 0.0 {
                Some((remaining / rate).round() as u64)
            } else {
                None
            }
        } else {
            None
        };
        let update = AgentJobProgressUpdate {
            job_id: job_id.to_string(),
            total_items: progress.total_items,
            pending_items: progress.pending_items,
            running_items: progress.running_items,
            completed_items: progress.completed_items,
            failed_items: progress.failed_items,
            eta_seconds,
        };
        let payload = serde_json::to_string(&update)?;
        session
            .notify_background_event_owned(turn, format!("agent_job_progress:{payload}"))
            .await;
        self.last_emit_at = Instant::now();
        self.last_processed = processed;
        self.last_failed = progress.failed_items;
        Ok(())
    }
}

impl ToolHandler for BatchJobHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> impl std::future::Future<Output = Result<Self::Output, FunctionCallError>> + Send {
        async move { handle_batch_job(invocation).await }
    }
}

async fn handle_batch_job(
    invocation: ToolInvocation,
) -> Result<FunctionToolOutput, FunctionCallError> {
    let ToolInvocation {
        session,
        turn,
        tool_name,
        payload,
        ..
    } = invocation;

    let arguments = match payload {
        ToolPayload::Function { arguments } => arguments,
        _ => {
            return Err(FunctionCallError::RespondToModel(
                "agent jobs handler received unsupported payload".to_string(),
            ));
        }
    };

    let tool = if tool_name == "spawn_agents_on_csv" {
        BatchJobTool::SpawnAgentsOnCsv
    } else if tool_name == "report_agent_job_result" {
        BatchJobTool::ReportAgentJobResult
    } else {
        return Err(FunctionCallError::RespondToModel(format!(
            "unsupported agent job tool {tool_name}"
        )));
    };

    match tool {
        BatchJobTool::SpawnAgentsOnCsv => {
            spawn_agents_on_csv::handle(session, turn, arguments).await
        }
        BatchJobTool::ReportAgentJobResult => {
            report_agent_job_result::handle(session, arguments).await
        }
    }
}

mod spawn_agents_on_csv {
    use super::*;

    /// Create a new agent job from a CSV and run it to completion.
    ///
    /// Each CSV row becomes a job item. The instruction string is a template where `{column}`
    /// placeholders are filled with values from that row. Results are reported by workers via
    /// `report_agent_job_result`, then exported to CSV on completion.
    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        arguments: String,
    ) -> Result<FunctionToolOutput, FunctionCallError> {
        let args: SpawnAgentsOnCsvArgs = parse_function_arguments(arguments)?;
        if args.instruction.trim().is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "instruction must be non-empty".to_string(),
            ));
        }

        let db = required_state_db(&session)?;
        let input_path = turn.resolve_path(Some(args.csv_path.clone()));
        let input_path_display = input_path.display().to_string();
        let csv_content = tokio::fs::read_to_string(input_path.clone().to_path_buf())
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to read csv input {input_path_display}: {err}"
                ))
            })?;
        let (headers, rows) = parse_csv_string(csv_content).map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to parse csv input: {err}"))
        })?;
        let prepared = prepare_spawn_agents_job(args, turn.as_ref(), input_path, headers, rows)?;
        let _job = db
            .clone()
            .create_agent_job_owned(
                codex_state::AgentJobCreateParams {
                    id: prepared.job_id.clone(),
                    name: prepared.job_name,
                    instruction: prepared.instruction,
                    auto_export: true,
                    max_runtime_seconds: prepared.max_runtime_seconds,
                    output_schema_json: prepared.output_schema_json,
                    input_headers: prepared.headers,
                    input_csv_path: prepared.input_csv_path,
                    output_csv_path: prepared.output_csv_path.to_string_lossy().into_owned(),
                },
                prepared.items,
            )
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!("failed to create agent job: {err}"))
            })?;

        let requested_concurrency = prepared.requested_concurrency;
        let job_id = prepared.job_id;
        let options = match build_runner_options(
            session.clone(),
            turn.clone(),
            requested_concurrency,
        )
        .await
        {
            Ok(options) => options,
            Err(err) => {
                let error_message = err.to_string();
                let _ = db
                    .clone()
                    .mark_agent_job_failed_owned(job_id.clone(), error_message)
                    .await;
                return Err(err);
            }
        };
        db.clone()
            .mark_agent_job_running_owned(job_id.clone())
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to transition agent job {job_id} to running: {err}"
                ))
            })?;
        let max_threads = turn.config.agent_max_threads;
        let effective_concurrency = options.max_concurrency;
        let message = format!(
            "agent job concurrency: job_id={job_id} requested={requested_concurrency:?} max_threads={max_threads:?} effective={effective_concurrency}"
        );
        let _ = session
            .clone()
            .notify_background_event_owned(turn.clone(), message)
            .await;
        if let Err(err) = run_agent_job_loop(
            session.clone(),
            turn.clone(),
            db.clone(),
            job_id.clone(),
            options,
        )
        .await
        {
            let error_message = format!("job runner failed: {err}");
            let _ = db
                .clone()
                .mark_agent_job_failed_owned(job_id.clone(), error_message)
                .await;
            return Err(FunctionCallError::RespondToModel(format!(
                "agent job {job_id} failed: {err}"
            )));
        }

        let job = db
            .clone()
            .get_agent_job_owned(job_id.clone())
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to load agent job {job_id}: {err}"
                ))
            })?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!("agent job {job_id} not found"))
            })?;
        let output_path = PathBuf::from(job.output_csv_path.clone());
        if !tokio::fs::try_exists(output_path.clone())
            .await
            .unwrap_or(false)
        {
            export_job_csv_snapshot(db.clone(), job.clone())
                .await
                .map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "failed to export output csv {job_id}: {err}"
                    ))
                })?;
        }
        let progress = db
            .clone()
            .get_agent_job_progress_owned(job_id.clone())
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to load agent job progress {job_id}: {err}"
                ))
            })?;
        let mut job_error = job.last_error.clone().filter(|err| !err.trim().is_empty());
        let failed_item_errors = if progress.failed_items > 0 {
            let items = db
                .clone()
                .list_agent_job_items_owned(
                    job_id.clone(),
                    Some(codex_state::AgentJobItemStatus::Failed),
                    Some(5),
                )
                .await
                .unwrap_or_default();
            let summaries: Vec<_> = items
                .into_iter()
                .filter_map(|item| {
                    let last_error = item.last_error.unwrap_or_default();
                    if last_error.trim().is_empty() {
                        return None;
                    }
                    Some(AgentJobFailureSummary {
                        item_id: item.item_id,
                        source_id: item.source_id,
                        last_error,
                    })
                })
                .collect();
            if summaries.is_empty() {
                if job_error.is_none() {
                    job_error = Some(
                        "agent job has failed items but no error details were recorded".to_string(),
                    );
                }
                None
            } else {
                Some(summaries)
            }
        } else {
            None
        };
        let content = serde_json::to_string(&SpawnAgentsOnCsvResult {
            job_id,
            status: job.status.as_str().to_string(),
            output_csv_path: job.output_csv_path,
            total_items: progress.total_items,
            completed_items: progress.completed_items,
            failed_items: progress.failed_items,
            job_error,
            failed_item_errors,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!(
                "failed to serialize spawn_agents_on_csv result: {err}"
            ))
        })?;
        Ok(FunctionToolOutput::from_text(content, Some(true)))
    }
}

mod report_agent_job_result {
    use super::*;

    pub async fn handle(
        session: Arc<Session>,
        arguments: String,
    ) -> Result<FunctionToolOutput, FunctionCallError> {
        let args: ReportAgentJobResultArgs = parse_function_arguments(arguments)?;
        if !args.result.is_object() {
            return Err(FunctionCallError::RespondToModel(
                "result must be a JSON object".to_string(),
            ));
        }
        let db = required_state_db(&session)?;
        let reporting_thread_id = session.conversation_id.to_string();
        let job_id = args.job_id.clone();
        let item_id = args.item_id.clone();
        let result = args.result.clone();
        let accepted = db
            .clone()
            .report_agent_job_item_result_owned(
                job_id.clone(),
                item_id.clone(),
                reporting_thread_id,
                result,
            )
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to record agent job result for {job_id} / {item_id}: {err}"
                ))
            })?;
        if accepted && args.stop.unwrap_or(false) {
            let message = "cancelled by worker request";
            let _ = db
                .clone()
                .mark_agent_job_cancelled_owned(args.job_id.clone(), message.to_string())
                .await;
        }
        let content =
            serde_json::to_string(&ReportAgentJobResultToolResult { accepted }).map_err(|err| {
                FunctionCallError::Fatal(format!(
                    "failed to serialize report_agent_job_result result: {err}"
                ))
            })?;
        Ok(FunctionToolOutput::from_text(content, Some(true)))
    }
}

fn parse_function_arguments<T>(arguments: String) -> Result<T, FunctionCallError>
where
    T: serde::de::DeserializeOwned,
{
    parse_arguments(arguments.as_str())
}

fn parse_csv_string(content: String) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    parse_csv(content.as_str())
}

fn required_state_db(
    session: &Arc<Session>,
) -> Result<Arc<codex_state::StateRuntime>, FunctionCallError> {
    session.state_db().ok_or_else(|| {
        FunctionCallError::Fatal("sqlite state db is unavailable for this session".to_string())
    })
}

async fn build_runner_options(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    requested_concurrency: Option<usize>,
) -> Result<JobRunnerOptions, FunctionCallError> {
    let session_source = turn.session_source.clone();
    let child_depth = next_thread_spawn_depth(&session_source);
    let max_depth = turn.config.agent_max_depth;
    if exceeds_thread_spawn_depth_limit(child_depth, max_depth) {
        return Err(FunctionCallError::RespondToModel(
            "agent depth limit reached; this session cannot spawn more subagents".to_string(),
        ));
    }
    let max_concurrency =
        normalize_concurrency(requested_concurrency, turn.config.agent_max_threads);
    let base_instructions = session.clone().get_base_instructions_owned().await;
    let spawn_config = build_agent_spawn_config_owned(base_instructions, turn)?;
    Ok(JobRunnerOptions {
        max_concurrency,
        spawn_config,
    })
}

fn normalize_concurrency(requested: Option<usize>, max_threads: Option<usize>) -> usize {
    let requested = requested.unwrap_or(DEFAULT_AGENT_JOB_CONCURRENCY).max(1);
    let requested = requested.min(MAX_AGENT_JOB_CONCURRENCY);
    if let Some(max_threads) = max_threads {
        requested.min(max_threads.max(1))
    } else {
        requested
    }
}

fn normalize_max_runtime_seconds(requested: Option<u64>) -> Result<Option<u64>, FunctionCallError> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    if requested == 0 {
        return Err(FunctionCallError::RespondToModel(
            "max_runtime_seconds must be >= 1".to_string(),
        ));
    }
    Ok(Some(requested))
}

fn prepare_spawn_agents_job(
    args: SpawnAgentsOnCsvArgs,
    turn: &TurnContext,
    input_path: AbsolutePathBuf,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
) -> Result<PreparedSpawnAgentsJob, FunctionCallError> {
    if headers.is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "csv input must include a header row".to_string(),
        ));
    }
    ensure_unique_headers(headers.as_slice())?;

    let id_column_index = args.id_column.as_ref().map_or(Ok(None), |column_name| {
        headers
            .iter()
            .position(|header| header == column_name)
            .map(Some)
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "id_column {column_name} was not found in csv headers"
                ))
            })
    })?;

    let mut items = Vec::with_capacity(rows.len());
    let mut seen_ids = HashSet::new();
    for (idx, row) in rows.into_iter().enumerate() {
        if row.len() != headers.len() {
            let row_index = idx + 2;
            let row_len = row.len();
            let header_len = headers.len();
            return Err(FunctionCallError::RespondToModel(format!(
                "csv row {row_index} has {row_len} fields but header has {header_len}"
            )));
        }

        let source_id = id_column_index
            .and_then(|index| row.get(index).cloned())
            .filter(|value| !value.trim().is_empty());
        let row_index = idx + 1;
        let base_item_id = source_id
            .clone()
            .unwrap_or_else(|| format!("row-{row_index}"));
        let mut item_id = base_item_id.clone();
        let mut suffix = 2usize;
        while !seen_ids.insert(item_id.clone()) {
            item_id = format!("{base_item_id}-{suffix}");
            suffix = suffix.saturating_add(1);
        }

        let row_object = headers
            .iter()
            .cloned()
            .zip(row.into_iter().map(Value::String))
            .collect::<serde_json::Map<_, _>>();
        items.push(codex_state::AgentJobItemCreateParams {
            item_id,
            row_index: idx as i64,
            source_id,
            row_json: Value::Object(row_object),
        });
    }

    let job_id = Uuid::new_v4().to_string();
    let output_csv_path = args.output_csv_path.map_or_else(
        || default_output_csv_path(input_path.clone(), job_id.clone()),
        |path| turn.resolve_path(Some(path)),
    );
    let max_runtime_seconds = normalize_max_runtime_seconds(
        args.max_runtime_seconds
            .or(turn.config.agent_job_max_runtime_seconds),
    )?;

    Ok(PreparedSpawnAgentsJob {
        job_name: format!("agent-job-{}", job_id.chars().take(8).collect::<String>()),
        instruction: args.instruction,
        output_schema_json: args.output_schema,
        headers,
        items,
        input_csv_path: input_path.to_string_lossy().into_owned(),
        output_csv_path,
        max_runtime_seconds,
        requested_concurrency: args.max_concurrency.or(args.max_workers),
        job_id,
    })
}

async fn run_agent_job_loop(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    db: Arc<codex_state::StateRuntime>,
    job_id: String,
    options: JobRunnerOptions,
) -> anyhow::Result<()> {
    let job = db
        .clone()
        .get_agent_job_owned(job_id.clone())
        .await?
        .ok_or_else(|| anyhow::anyhow!("agent job {job_id} was not found"))?;
    let runtime_timeout = job_runtime_timeout(&job);
    let mut active_items: HashMap<ThreadId, ActiveJobItem> = HashMap::new();
    let mut progress_emitter = JobProgressEmitter::new();
    recover_running_items(
        session.clone(),
        db.clone(),
        job_id.clone(),
        &mut active_items,
        runtime_timeout,
    )
    .await?;
    let initial_progress = db
        .clone()
        .get_agent_job_progress_owned(job_id.clone())
        .await?;
    progress_emitter
        .maybe_emit(
            session.clone(),
            turn.clone(),
            job_id.clone(),
            initial_progress,
            /*force*/ true,
        )
        .await?;

    let mut cancel_requested = db
        .clone()
        .is_agent_job_cancelled_owned(job_id.clone())
        .await?;
    loop {
        let mut progressed = false;

        if !cancel_requested
            && db
                .clone()
                .is_agent_job_cancelled_owned(job_id.clone())
                .await?
        {
            cancel_requested = true;
            let _ = session
                .clone()
                .notify_background_event_owned(
                    turn.clone(),
                    format!("agent job {job_id} cancellation requested; stopping new workers"),
                )
                .await;
        }

        if !cancel_requested && active_items.len() < options.max_concurrency {
            let slots = options.max_concurrency - active_items.len();
            let pending_items = db
                .clone()
                .list_agent_job_items_owned(
                    job_id.clone(),
                    Some(codex_state::AgentJobItemStatus::Pending),
                    Some(slots),
                )
                .await?;
            for item in pending_items {
                let prompt = build_worker_prompt_owned(
                    job.id.clone(),
                    item.item_id.clone(),
                    job.instruction.clone(),
                    item.row_json.clone(),
                    job.output_schema_json.clone(),
                )?;
                let items = vec![UserInput::Text {
                    text: prompt,
                    text_elements: Vec::new(),
                }];
                let thread_id = match session
                    .services
                    .agent_control
                    .clone()
                    .spawn_agent_owned(
                        options.spawn_config.clone(),
                        items.into(),
                        Some(SessionSource::SubAgent(SubAgentSource::Other(format!(
                            "agent_job:{job_id}"
                        )))),
                    )
                    .await
                {
                    Ok(thread_id) => thread_id,
                    Err(CodexErr::AgentLimitReached { .. }) => {
                        db.clone()
                            .mark_agent_job_item_pending_owned(
                                job_id.clone(),
                                item.item_id.clone(),
                                /*error_message*/ None,
                            )
                            .await?;
                        break;
                    }
                    Err(err) => {
                        let error_message = format!("failed to spawn worker: {err}");
                        db.clone()
                            .mark_agent_job_item_failed_owned(
                                job_id.clone(),
                                item.item_id.clone(),
                                error_message,
                            )
                            .await?;
                        progressed = true;
                        continue;
                    }
                };
                let thread_id_string = thread_id.to_string();
                let assigned = db
                    .clone()
                    .mark_agent_job_item_running_with_thread_owned(
                        job_id.clone(),
                        item.item_id.clone(),
                        thread_id_string,
                    )
                    .await?;
                if !assigned {
                    let _ = session
                        .services
                        .agent_control
                        .clone()
                        .shutdown_live_agent_owned(thread_id)
                        .await;
                    continue;
                }
                active_items.insert(
                    thread_id,
                    ActiveJobItem {
                        item_id: item.item_id.clone(),
                        started_at: Instant::now(),
                        status_rx: session
                            .services
                            .agent_control
                            .clone()
                            .subscribe_status_owned(thread_id)
                            .await
                            .ok(),
                    },
                );
                progressed = true;
            }
        }

        if reap_stale_active_items(
            session.clone(),
            db.clone(),
            job_id.clone(),
            &mut active_items,
            runtime_timeout,
        )
        .await?
        {
            progressed = true;
        }

        let active_items_snapshot = active_items
            .iter()
            .map(|(thread_id, item)| (*thread_id, item.clone()))
            .collect::<Vec<_>>();
        let finished = find_finished_threads(session.clone(), active_items_snapshot).await;
        if finished.is_empty() {
            let progress = db
                .clone()
                .get_agent_job_progress_owned(job_id.clone())
                .await?;
            if cancel_requested {
                if progress.running_items == 0 && active_items.is_empty() {
                    break;
                }
            } else if progress.pending_items == 0
                && progress.running_items == 0
                && active_items.is_empty()
            {
                break;
            }
            if !progressed {
                let active_items_snapshot = active_items.values().cloned().collect::<Vec<_>>();
                wait_for_status_change(active_items_snapshot).await;
            }
            continue;
        }

        for (thread_id, item_id) in finished {
            finalize_finished_item(
                session.clone(),
                db.clone(),
                job_id.clone(),
                item_id.clone(),
                thread_id,
            )
            .await?;
            active_items.remove(&thread_id);
            let progress = db
                .clone()
                .get_agent_job_progress_owned(job_id.clone())
                .await?;
            progress_emitter
                .maybe_emit(
                    session.clone(),
                    turn.clone(),
                    job_id.clone(),
                    progress,
                    /*force*/ false,
                )
                .await?;
        }
    }

    let progress = db
        .clone()
        .get_agent_job_progress_owned(job_id.clone())
        .await?;
    if let Err(err) = export_job_csv_snapshot(db.clone(), job.clone()).await {
        let message = format!("auto-export failed: {err}");
        db.clone()
            .mark_agent_job_failed_owned(job_id.clone(), message)
            .await?;
        return Ok(());
    }
    let cancelled = cancel_requested
        || db
            .clone()
            .is_agent_job_cancelled_owned(job_id.clone())
            .await?;
    if cancelled {
        let pending_items = progress.pending_items;
        let message =
            format!("agent job {job_id} cancelled with {pending_items} unprocessed items");
        let _ = session
            .clone()
            .notify_background_event_owned(turn.clone(), message)
            .await;
        progress_emitter
            .maybe_emit(
                session.clone(),
                turn.clone(),
                job_id.clone(),
                progress,
                /*force*/ true,
            )
            .await?;
        return Ok(());
    }
    if progress.failed_items > 0 {
        let failed_items = progress.failed_items;
        let message = format!("agent job completed with {failed_items} failed items");
        let _ = session
            .clone()
            .notify_background_event_owned(turn.clone(), message)
            .await;
    }
    db.clone()
        .mark_agent_job_completed_owned(job_id.clone())
        .await?;
    let progress = db.get_agent_job_progress_owned(job_id.clone()).await?;
    progress_emitter
        .maybe_emit(session, turn, job_id, progress, /*force*/ true)
        .await?;
    Ok(())
}

async fn export_job_csv_snapshot(
    db: Arc<codex_state::StateRuntime>,
    job: codex_state::AgentJob,
) -> anyhow::Result<()> {
    let items = db
        .clone()
        .list_agent_job_items_owned(job.id.clone(), /*status*/ None, /*limit*/ None)
        .await?;
    let csv_content = render_job_csv(job.input_headers.as_slice(), items.as_slice())
        .map_err(|err| anyhow::anyhow!("failed to render job csv for auto-export: {err}"))?;
    let output_path = PathBuf::from(job.output_csv_path.clone());
    if let Some(parent) = output_path.parent().map(std::path::Path::to_path_buf) {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(output_path.clone(), csv_content).await?;
    Ok(())
}

async fn recover_running_items(
    session: Arc<Session>,
    db: Arc<codex_state::StateRuntime>,
    job_id: String,
    active_items: &mut HashMap<ThreadId, ActiveJobItem>,
    runtime_timeout: Duration,
) -> anyhow::Result<()> {
    let running_items = db
        .clone()
        .list_agent_job_items_owned(
            job_id.clone(),
            Some(codex_state::AgentJobItemStatus::Running),
            /*limit*/ None,
        )
        .await?;
    for item in running_items {
        if is_item_stale(&item, runtime_timeout) {
            let error_message = format!("worker exceeded max runtime of {runtime_timeout:?}");
            db.clone()
                .mark_agent_job_item_failed_owned(
                    job_id.clone(),
                    item.item_id.clone(),
                    error_message,
                )
                .await?;
            if let Some(assigned_thread_id) = item.assigned_thread_id.as_ref()
                && let Ok(thread_id) = ThreadId::from_string(assigned_thread_id.as_str())
            {
                let _ = session
                    .services
                    .agent_control
                    .clone()
                    .shutdown_live_agent_owned(thread_id)
                    .await;
            }
            continue;
        }
        let Some(assigned_thread_id) = item.assigned_thread_id.clone() else {
            db.clone()
                .mark_agent_job_item_failed_owned(
                    job_id.clone(),
                    item.item_id.clone(),
                    "running item is missing assigned_thread_id".to_string(),
                )
                .await?;
            continue;
        };
        let thread_id = match ThreadId::from_string(assigned_thread_id.as_str()) {
            Ok(thread_id) => thread_id,
            Err(err) => {
                let error_message = format!("invalid assigned_thread_id: {err:?}");
                db.clone()
                    .mark_agent_job_item_failed_owned(
                        job_id.clone(),
                        item.item_id.clone(),
                        error_message,
                    )
                    .await?;
                continue;
            }
        };
        if is_final(
            &session
                .services
                .agent_control
                .clone()
                .get_status_owned(thread_id)
                .await,
        ) {
            finalize_finished_item(
                session.clone(),
                db.clone(),
                job_id.clone(),
                item.item_id.clone(),
                thread_id,
            )
            .await?;
        } else {
            active_items.insert(
                thread_id,
                ActiveJobItem {
                    item_id: item.item_id.clone(),
                    started_at: started_at_from_item(&item),
                    status_rx: session
                        .services
                        .agent_control
                        .clone()
                        .subscribe_status_owned(thread_id)
                        .await
                        .ok(),
                },
            );
        }
    }
    Ok(())
}

async fn find_finished_threads(
    session: Arc<Session>,
    active_items: Vec<(ThreadId, ActiveJobItem)>,
) -> Vec<(ThreadId, String)> {
    let mut finished = Vec::new();
    for (thread_id, item) in active_items {
        let status = active_item_status(session.clone(), thread_id, &item).await;
        if is_final(&status) {
            finished.push((thread_id, item.item_id.clone()));
        }
    }
    finished
}

async fn active_item_status(
    session: Arc<Session>,
    thread_id: ThreadId,
    item: &ActiveJobItem,
) -> AgentStatus {
    if let Some(status_rx) = item.status_rx.as_ref()
        && status_rx.has_changed().is_ok()
    {
        return status_rx.borrow().clone();
    }
    session
        .services
        .agent_control
        .clone()
        .get_status_owned(thread_id)
        .await
}

async fn wait_for_status_change(active_items: Vec<ActiveJobItem>) {
    let mut waiters = FuturesUnordered::new();
    for item in active_items {
        if let Some(status_rx) = item.status_rx.as_ref() {
            let mut status_rx = status_rx.clone();
            waiters.push(async move {
                let _ = status_rx.changed().await;
            });
        }
    }
    if waiters.is_empty() {
        tokio::time::sleep(STATUS_POLL_INTERVAL).await;
        return;
    }
    let _ = timeout(STATUS_POLL_INTERVAL, waiters.next()).await;
}

async fn reap_stale_active_items(
    session: Arc<Session>,
    db: Arc<codex_state::StateRuntime>,
    job_id: String,
    active_items: &mut HashMap<ThreadId, ActiveJobItem>,
    runtime_timeout: Duration,
) -> anyhow::Result<bool> {
    let mut stale = Vec::new();
    for (thread_id, item) in active_items.iter() {
        if item.started_at.elapsed() >= runtime_timeout {
            stale.push((*thread_id, item.item_id.clone()));
        }
    }
    if stale.is_empty() {
        return Ok(false);
    }
    for (thread_id, item_id) in stale {
        let error_message = format!("worker exceeded max runtime of {runtime_timeout:?}");
        db.clone()
            .mark_agent_job_item_failed_owned(job_id.clone(), item_id.clone(), error_message)
            .await?;
        let _ = session
            .services
            .agent_control
            .clone()
            .shutdown_live_agent_owned(thread_id)
            .await;
        active_items.remove(&thread_id);
    }
    Ok(true)
}

async fn finalize_finished_item(
    session: Arc<Session>,
    db: Arc<codex_state::StateRuntime>,
    job_id: String,
    item_id: String,
    thread_id: ThreadId,
) -> anyhow::Result<()> {
    let item = db
        .clone()
        .get_agent_job_item_owned(job_id.clone(), item_id.clone())
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("job item not found for finalization: {job_id}/{item_id}")
        })?;
    if matches!(item.status, codex_state::AgentJobItemStatus::Running) {
        if item.result_json.is_some() {
            let _ = db
                .mark_agent_job_item_completed_owned(job_id.clone(), item_id.clone())
                .await?;
        } else {
            let _ = db
                .clone()
                .mark_agent_job_item_failed_owned(
                    job_id.clone(),
                    item_id.clone(),
                    "worker finished without calling report_agent_job_result".to_string(),
                )
                .await?;
        }
    }
    let _ = session
        .services
        .agent_control
        .clone()
        .shutdown_live_agent_owned(thread_id)
        .await;
    Ok(())
}

fn build_worker_prompt_owned(
    job_id: String,
    item_id: String,
    instruction_template: String,
    row_json: Value,
    output_schema_json: Option<Value>,
) -> anyhow::Result<String> {
    let instruction = render_instruction_template_owned(instruction_template, row_json.clone());
    let output_schema = output_schema_json
        .as_ref()
        .map(serde_json::to_string_pretty)
        .transpose()?
        .unwrap_or_else(|| "{}".to_string());
    let row_json = serde_json::to_string_pretty(&row_json)?;
    Ok(format!(
        "You are processing one item for a generic agent job.\n\
Job ID: {job_id}\n\
Item ID: {item_id}\n\n\
Task instruction:\n\
{instruction}\n\n\
Input row (JSON):\n\
{row_json}\n\n\
Expected result schema (JSON Schema or {{}}):\n\
{output_schema}\n\n\
You MUST call the `report_agent_job_result` tool exactly once with:\n\
1. `job_id` = \"{job_id}\"\n\
2. `item_id` = \"{item_id}\"\n\
3. `result` = a JSON object that contains your analysis result for this row.\n\n\
If you need to stop the job early, include `stop` = true in the tool call.\n\n\
After the tool call succeeds, stop.",
    ))
}

fn render_instruction_template_owned(instruction: String, row_json: Value) -> String {
    const OPEN_BRACE_SENTINEL: &str = "__CODEX_OPEN_BRACE__";
    const CLOSE_BRACE_SENTINEL: &str = "__CODEX_CLOSE_BRACE__";

    let mut rendered = instruction
        .replace("{{", OPEN_BRACE_SENTINEL)
        .replace("}}", CLOSE_BRACE_SENTINEL);
    let Some(row) = row_json.as_object() else {
        return rendered
            .replace(OPEN_BRACE_SENTINEL, "{")
            .replace(CLOSE_BRACE_SENTINEL, "}");
    };
    for (key, value) in row {
        let placeholder = format!("{{{key}}}");
        let replacement = value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string());
        rendered = rendered.replace(placeholder.as_str(), replacement.as_str());
    }
    rendered
        .replace(OPEN_BRACE_SENTINEL, "{")
        .replace(CLOSE_BRACE_SENTINEL, "}")
}

fn ensure_unique_headers(headers: &[String]) -> Result<(), FunctionCallError> {
    let mut seen = HashSet::new();
    for header in headers {
        if !seen.insert(header) {
            return Err(FunctionCallError::RespondToModel(format!(
                "csv header {header} is duplicated"
            )));
        }
    }
    Ok(())
}

fn job_runtime_timeout(job: &codex_state::AgentJob) -> Duration {
    job.max_runtime_seconds
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_AGENT_JOB_ITEM_TIMEOUT)
}

fn started_at_from_item(item: &codex_state::AgentJobItem) -> Instant {
    let now = chrono::Utc::now();
    let age = now.signed_duration_since(item.updated_at);
    if let Ok(age) = age.to_std() {
        Instant::now().checked_sub(age).unwrap_or_else(Instant::now)
    } else {
        Instant::now()
    }
}

fn is_item_stale(item: &codex_state::AgentJobItem, runtime_timeout: Duration) -> bool {
    let now = chrono::Utc::now();
    if let Ok(age) = now.signed_duration_since(item.updated_at).to_std() {
        age >= runtime_timeout
    } else {
        false
    }
}

fn default_output_csv_path(input_csv_path: AbsolutePathBuf, job_id: String) -> AbsolutePathBuf {
    let stem = input_csv_path
        .as_path()
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agent_job_output".to_string());
    let job_suffix = job_id.chars().take(8).collect::<String>();
    let output_dir = input_csv_path
        .parent()
        .unwrap_or_else(|| input_csv_path.clone());
    output_dir.join(format!("{stem}.agent-job-{job_suffix}.csv"))
}

fn parse_csv(content: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());
    let headers_record = reader.headers().map_err(|err| err.to_string())?;
    let mut headers: Vec<String> = headers_record.iter().map(str::to_string).collect();
    if let Some(first) = headers.first_mut() {
        *first = first.trim_start_matches('\u{feff}').to_string();
    }
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|err| err.to_string())?;
        let row: Vec<String> = record.iter().map(str::to_string).collect();
        if row.iter().all(std::string::String::is_empty) {
            continue;
        }
        rows.push(row);
    }
    Ok((headers, rows))
}

fn render_job_csv(
    headers: &[String],
    items: &[codex_state::AgentJobItem],
) -> Result<String, FunctionCallError> {
    let mut csv = String::new();
    let mut output_headers = headers.to_vec();
    output_headers.extend([
        "job_id".to_string(),
        "item_id".to_string(),
        "row_index".to_string(),
        "source_id".to_string(),
        "status".to_string(),
        "attempt_count".to_string(),
        "last_error".to_string(),
        "result_json".to_string(),
        "reported_at".to_string(),
        "completed_at".to_string(),
    ]);
    csv.push_str(
        output_headers
            .iter()
            .map(|header| csv_escape(header.as_str()))
            .collect::<Vec<_>>()
            .join(",")
            .as_str(),
    );
    csv.push('\n');
    for item in items {
        let row_object = item.row_json.as_object().ok_or_else(|| {
            let item_id = item.item_id.as_str();
            FunctionCallError::RespondToModel(format!(
                "row_json for item {item_id} is not a JSON object"
            ))
        })?;
        let mut row_values = Vec::new();
        for header in headers {
            let value = row_object
                .get(header)
                .map_or_else(String::new, value_to_csv_string);
            row_values.push(csv_escape(value.as_str()));
        }
        row_values.push(csv_escape(item.job_id.as_str()));
        row_values.push(csv_escape(item.item_id.as_str()));
        row_values.push(csv_escape(item.row_index.to_string().as_str()));
        row_values.push(csv_escape(
            item.source_id.clone().unwrap_or_default().as_str(),
        ));
        row_values.push(csv_escape(item.status.as_str()));
        row_values.push(csv_escape(item.attempt_count.to_string().as_str()));
        row_values.push(csv_escape(
            item.last_error.clone().unwrap_or_default().as_str(),
        ));
        row_values.push(csv_escape(
            item.result_json
                .as_ref()
                .map_or_else(String::new, std::string::ToString::to_string)
                .as_str(),
        ));
        row_values.push(csv_escape(
            item.reported_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default()
                .as_str(),
        ));
        row_values.push(csv_escape(
            item.completed_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default()
                .as_str(),
        ));
        csv.push_str(row_values.join(",").as_str());
        csv.push('\n');
    }
    Ok(csv)
}

fn value_to_csv_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('\n') || value.contains('\r') || value.contains('"') {
        let escaped = value.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
#[path = "agent_jobs_tests.rs"]
mod tests;
