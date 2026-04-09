use crate::agent::AgentStatus;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::config::SpawnedAgentBackendConfig;
use crate::config::SpawnedAgentBackendProtocol;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::rollout::RolloutRecorder;
use crate::thread_manager::ThreadManagerState;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::user_input::UserInput;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
#[cfg(test)]
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::Mutex as StdMutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tokio::time::sleep;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SpawnedAgentBackendKind {
    #[default]
    Codex,
    ClaudeCode,
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedSpawnedAgentBackend {
    pub(crate) id: String,
    pub(crate) kind: SpawnedAgentBackendKind,
    pub(crate) command_backend: Option<SpawnedAgentBackendConfig>,
}

impl ResolvedSpawnedAgentBackend {
    pub(crate) fn default_model(&self) -> Option<&str> {
        match self.kind {
            SpawnedAgentBackendKind::Codex => None,
            SpawnedAgentBackendKind::ClaudeCode => Some("claude-code"),
            SpawnedAgentBackendKind::Command => self
                .command_backend
                .as_ref()
                .and_then(|backend| backend.default_model.as_deref()),
        }
    }

    pub(crate) fn command_backend(&self) -> Option<&SpawnedAgentBackendConfig> {
        self.command_backend.as_ref()
    }
}

pub(crate) fn resolve_spawned_agent_backend(
    config: &crate::config::Config,
    value: Option<&str>,
) -> Result<ResolvedSpawnedAgentBackend, String> {
    let backend_id = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("codex");
    match backend_id {
        "codex" => Ok(ResolvedSpawnedAgentBackend {
            id: "codex".to_string(),
            kind: SpawnedAgentBackendKind::Codex,
            command_backend: None,
        }),
        "claude_code" | "claude-code" | "claudecode" => Ok(ResolvedSpawnedAgentBackend {
            id: "claude_code".to_string(),
            kind: SpawnedAgentBackendKind::ClaudeCode,
            command_backend: None,
        }),
        other => config
            .agent_backends
            .get(other)
            .cloned()
            .map(|backend| {
                let kind = match backend.protocol {
                    SpawnedAgentBackendProtocol::ClaudeCliLegacy => {
                        SpawnedAgentBackendKind::ClaudeCode
                    }
                    SpawnedAgentBackendProtocol::JsonStdioV1 => SpawnedAgentBackendKind::Command,
                };
                ResolvedSpawnedAgentBackend {
                    id: other.to_string(),
                    kind,
                    command_backend: Some(backend),
                }
            })
            .ok_or_else(|| {
                format!("unknown backend '{other}'; expected 'codex', 'claude_code', or a configured [agent_backends.<name>] entry")
            }),
    }
}

pub(crate) fn backend_id_from_config(config: &crate::config::Config) -> Option<&str> {
    Some(config.agent_backend_id.as_str())
}

pub(crate) fn resolve_spawned_agent_backend_from_config(
    config: &crate::config::Config,
) -> Result<ResolvedSpawnedAgentBackend, String> {
    resolve_spawned_agent_backend(config, backend_id_from_config(config))
}

pub(crate) fn normalize_backend_id(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("codex")
        .to_string()
}

pub(crate) fn apply_archived_spawned_agent_config(
    config: &mut crate::config::Config,
    snapshot: &ThreadConfigSnapshot,
) {
    config.agent_backend_id = snapshot.agent_backend_id.clone();
    config.model = Some(snapshot.model.clone());
    config.model_reasoning_effort = snapshot.reasoning_effort;
    if let Ok(cwd) = codex_utils_absolute_path::AbsolutePathBuf::try_from(snapshot.cwd.clone()) {
        config.cwd = cwd;
    }
}

impl SpawnedAgentBackendKind {
    #[cfg(test)]
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Codex),
            Some("codex") => Ok(Self::Codex),
            Some("claude_code" | "claude-code" | "claudecode") => Ok(Self::ClaudeCode),
            Some("command") => Ok(Self::Command),
            Some(other) => Err(format!("unknown backend '{other}'")),
        }
    }
}

#[derive(Clone)]
/// Per-agent runtime handle for spawned agents.
pub(crate) enum SpawnedAgentHandle {
    Codex(CodexSpawnedAgentHandle),
    ClaudeCode(ClaudeCodeSpawnedAgentHandle),
    Command(CommandSpawnedAgentHandle),
}

#[derive(Clone, Debug)]
pub(crate) enum ArchivedSpawnedAgentHandle {
    ClaudeCode(ClaudeCodeResumeState),
    Command(CommandBackendResumeState),
}

impl ArchivedSpawnedAgentHandle {
    pub(crate) fn agent_id(&self) -> ThreadId {
        match self {
            Self::ClaudeCode(state) => state.agent_id,
            Self::Command(state) => state.agent_id,
        }
    }

    pub(crate) fn config_snapshot(&self) -> &ThreadConfigSnapshot {
        match self {
            Self::ClaudeCode(state) => &state.config_snapshot,
            Self::Command(state) => &state.config_snapshot,
        }
    }

    pub(crate) fn into_live_handle(self, manager: Weak<ThreadManagerState>) -> SpawnedAgentHandle {
        match self {
            Self::ClaudeCode(state) => SpawnedAgentHandle::ClaudeCode(
                ClaudeCodeSpawnedAgentHandle::from_resume_state(manager, state),
            ),
            Self::Command(state) => SpawnedAgentHandle::Command(
                CommandSpawnedAgentHandle::from_resume_state(manager, state),
            ),
        }
    }
}

impl SpawnedAgentHandle {
    pub(crate) fn codex(manager: Weak<ThreadManagerState>, agent_id: ThreadId) -> Self {
        Self::Codex(CodexSpawnedAgentHandle::new(manager, agent_id))
    }

    pub(crate) fn from_resolved_backend(
        manager: Weak<ThreadManagerState>,
        config: &crate::config::Config,
        agent_id: ThreadId,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
        backend: &ResolvedSpawnedAgentBackend,
    ) -> CodexResult<Option<Self>> {
        match backend.kind {
            SpawnedAgentBackendKind::Codex => Ok(None),
            SpawnedAgentBackendKind::ClaudeCode => {
                let command = if let Some(command_backend) = backend.command_backend() {
                    ClaudeCodeCommand::from_command_prefix(command_backend.command.clone())
                } else {
                    ClaudeCodeCommand::from_config(config)
                };
                Ok(Some(Self::ClaudeCode(ClaudeCodeSpawnedAgentHandle::new(
                    manager,
                    agent_id,
                    config_snapshot,
                    developer_instructions,
                    command,
                    AgentStatus::PendingInit,
                    /*session_id*/ None,
                    /*total_token_usage*/ None,
                ))))
            }
            SpawnedAgentBackendKind::Command => {
                let backend_config = backend.command_backend().cloned().ok_or_else(|| {
                    CodexErr::Fatal(format!(
                        "backend '{}' missing command backend config",
                        backend.id
                    ))
                })?;
                Ok(Some(Self::Command(CommandSpawnedAgentHandle::new(
                    manager,
                    agent_id,
                    backend.id.clone(),
                    config_snapshot,
                    developer_instructions,
                    CommandBackendCommand::from_config(&backend_config),
                    CommandBackendCommand::from_healthcheck_config(&backend_config),
                    backend_config
                        .healthcheck_timeout_seconds
                        .map(Duration::from_secs),
                    backend_config.turn_timeout_seconds.map(Duration::from_secs),
                    backend_config.max_retries,
                    backend_config.supports_resume,
                    backend_config.supports_interrupt,
                    AgentStatus::PendingInit,
                    /*session_id*/ None,
                    /*total_token_usage*/ None,
                ))))
            }
        }
    }

    pub(crate) fn from_config(
        manager: Weak<ThreadManagerState>,
        config: &crate::config::Config,
        agent_id: ThreadId,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
    ) -> CodexResult<Option<Self>> {
        let resolved_backend = resolve_spawned_agent_backend_from_config(config)
            .map_err(CodexErr::UnsupportedOperation)?;
        Self::from_resolved_backend(
            manager,
            config,
            agent_id,
            config_snapshot,
            developer_instructions,
            &resolved_backend,
        )
    }

    pub(crate) async fn claude_code(
        manager: Weak<ThreadManagerState>,
        agent_id: ThreadId,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
        items: Vec<UserInput>,
        command: ClaudeCodeCommand,
    ) -> CodexResult<Self> {
        let handle = ClaudeCodeSpawnedAgentHandle::new(
            manager,
            agent_id,
            config_snapshot,
            developer_instructions,
            command,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );
        handle.send_input(items).await?;
        Ok(Self::ClaudeCode(handle))
    }

    pub(crate) fn resumed_claude_code(
        manager: Weak<ThreadManagerState>,
        state: ClaudeCodeResumeState,
    ) -> Self {
        Self::ClaudeCode(ClaudeCodeSpawnedAgentHandle::from_resume_state(
            manager, state,
        ))
    }

    #[cfg(test)]
    pub(crate) async fn claude_code_for_test(
        agent_id: ThreadId,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
        items: Vec<UserInput>,
        command: ClaudeCodeCommand,
    ) -> CodexResult<Self> {
        let handle = ClaudeCodeSpawnedAgentHandle::new(
            Weak::new(),
            agent_id,
            config_snapshot,
            developer_instructions,
            command,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );
        handle.send_input(items).await?;
        Ok(Self::ClaudeCode(handle))
    }

    pub(crate) fn agent_id(&self) -> ThreadId {
        match self {
            Self::Codex(handle) => handle.agent_id,
            Self::ClaudeCode(handle) => handle.agent_id(),
            Self::Command(handle) => handle.agent_id(),
        }
    }

    pub(crate) async fn send_input(&self, items: Vec<UserInput>) -> CodexResult<String> {
        match self {
            Self::Codex(handle) => handle.send_input(items).await,
            Self::ClaudeCode(handle) => handle.send_input(items).await,
            Self::Command(handle) => handle.send_input(items).await,
        }
    }

    pub(crate) async fn interrupt(&self) -> CodexResult<String> {
        match self {
            Self::Codex(handle) => handle.interrupt().await,
            Self::ClaudeCode(handle) => handle.interrupt().await,
            Self::Command(handle) => handle.interrupt().await,
        }
    }

    pub(crate) async fn cleanup_after_internal_death(&self) {
        match self {
            Self::Codex(handle) => handle.cleanup_after_internal_death().await,
            Self::ClaudeCode(handle) => handle.cleanup_after_internal_death().await,
            Self::Command(handle) => handle.cleanup_after_internal_death().await,
        }
    }

    pub(crate) async fn shutdown_live(&self) -> CodexResult<String> {
        match self {
            Self::Codex(handle) => handle.shutdown_live().await,
            Self::ClaudeCode(handle) => handle.shutdown_live().await,
            Self::Command(handle) => handle.shutdown_live().await,
        }
    }

    pub(crate) async fn status(&self) -> AgentStatus {
        match self {
            Self::Codex(handle) => handle.status().await,
            Self::ClaudeCode(handle) => handle.status().await,
            Self::Command(handle) => handle.status().await,
        }
    }

    pub(crate) async fn config_snapshot(&self) -> Option<ThreadConfigSnapshot> {
        match self {
            Self::Codex(handle) => handle.config_snapshot().await,
            Self::ClaudeCode(handle) => handle.config_snapshot().await,
            Self::Command(handle) => handle.config_snapshot().await,
        }
    }

    pub(crate) async fn subscribe_status(&self) -> CodexResult<watch::Receiver<AgentStatus>> {
        match self {
            Self::Codex(handle) => handle.subscribe_status().await,
            Self::ClaudeCode(handle) => handle.subscribe_status().await,
            Self::Command(handle) => handle.subscribe_status().await,
        }
    }

    pub(crate) async fn total_token_usage(&self) -> Option<TokenUsage> {
        match self {
            Self::Codex(handle) => handle.total_token_usage().await,
            Self::ClaudeCode(handle) => handle.total_token_usage().await,
            Self::Command(handle) => handle.total_token_usage().await,
        }
    }

    pub(crate) async fn archived_state(&self) -> Option<ArchivedSpawnedAgentHandle> {
        match self {
            Self::ClaudeCode(handle) => Some(ArchivedSpawnedAgentHandle::ClaudeCode(
                handle.resume_state().await,
            )),
            Self::Command(handle) => handle
                .resume_state()
                .await
                .map(ArchivedSpawnedAgentHandle::Command),
            Self::Codex(_) => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CodexSpawnedAgentHandle {
    manager: Weak<ThreadManagerState>,
    agent_id: ThreadId,
}

impl CodexSpawnedAgentHandle {
    pub(crate) fn new(manager: Weak<ThreadManagerState>, agent_id: ThreadId) -> Self {
        Self { manager, agent_id }
    }

    async fn send_input(&self, items: Vec<UserInput>) -> CodexResult<String> {
        self.manager()?
            .send_op(
                self.agent_id,
                Op::UserInput {
                    items,
                    final_output_json_schema: None,
                },
            )
            .await
    }

    async fn interrupt(&self) -> CodexResult<String> {
        self.manager()?.send_op(self.agent_id, Op::Interrupt).await
    }

    async fn cleanup_after_internal_death(&self) {
        if let Ok(state) = self.manager() {
            let _ = state.remove_thread(&self.agent_id).await;
        }
    }

    async fn shutdown_live(&self) -> CodexResult<String> {
        let state = self.manager()?;
        let result = if let Ok(thread) = state.get_thread(self.agent_id).await {
            thread.codex.session.ensure_rollout_materialized().await;
            thread.codex.session.flush_rollout().await;
            if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state.send_op(self.agent_id, Op::Shutdown {}).await
            }
        } else {
            state.send_op(self.agent_id, Op::Shutdown {}).await
        };
        let _ = state.remove_thread(&self.agent_id).await;
        result
    }

    async fn status(&self) -> AgentStatus {
        let Ok(state) = self.manager() else {
            return AgentStatus::NotFound;
        };
        let Ok(thread) = state.get_thread(self.agent_id).await else {
            return AgentStatus::NotFound;
        };
        thread.agent_status().await
    }

    async fn config_snapshot(&self) -> Option<ThreadConfigSnapshot> {
        let state = self.manager().ok()?;
        let thread = state.get_thread(self.agent_id).await.ok()?;
        Some(thread.config_snapshot().await)
    }

    async fn subscribe_status(&self) -> CodexResult<watch::Receiver<AgentStatus>> {
        let state = self.manager()?;
        let thread = state.get_thread(self.agent_id).await?;
        Ok(thread.subscribe_status())
    }

    async fn total_token_usage(&self) -> Option<TokenUsage> {
        let state = self.manager().ok()?;
        let thread = state.get_thread(self.agent_id).await.ok()?;
        thread.total_token_usage().await
    }

    fn manager(&self) -> CodexResult<Arc<ThreadManagerState>> {
        self.manager
            .upgrade()
            .ok_or_else(|| CodexErr::UnsupportedOperation("thread manager dropped".to_string()))
    }
}

#[derive(Clone)]
pub(crate) struct ClaudeCodeSpawnedAgentHandle {
    state: Arc<ClaudeCodeSpawnedAgentState>,
}

impl ClaudeCodeSpawnedAgentHandle {
    fn new(
        manager: Weak<ThreadManagerState>,
        agent_id: ThreadId,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
        command: ClaudeCodeCommand,
        initial_status: AgentStatus,
        session_id: Option<String>,
        total_token_usage: Option<TokenUsage>,
    ) -> Self {
        let (status_tx, _) = watch::channel(initial_status);
        let state = Arc::new(ClaudeCodeSpawnedAgentState {
            manager,
            agent_id,
            config_snapshot,
            developer_instructions,
            command,
            status_tx,
            total_token_usage: RwLock::new(total_token_usage),
            session_id: RwLock::new(session_id),
            child: Mutex::new(None),
            stdout_task: Mutex::new(None),
            stderr_task: Mutex::new(None),
            turn_running: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            interrupt_requested: AtomicBool::new(false),
            submission_counter: AtomicU64::new(1),
            turn_generation: AtomicU64::new(0),
            turn_lock: Mutex::new(()),
        });

        Self { state }
    }

    fn from_resume_state(manager: Weak<ThreadManagerState>, state: ClaudeCodeResumeState) -> Self {
        Self::new(
            manager,
            state.agent_id,
            state.config_snapshot,
            state.developer_instructions,
            state.command,
            state.status,
            state.session_id,
            state.total_token_usage,
        )
    }

    fn agent_id(&self) -> ThreadId {
        self.state.agent_id
    }

    async fn send_input(&self, items: Vec<UserInput>) -> CodexResult<String> {
        let prompt = render_claude_prompt(
            &load_rollout_prompt_context(&self.state.manager, self.state.agent_id).await,
            &items,
        );
        let _guard = self.state.turn_lock.lock().await;
        self.refresh_process_status().await;
        if self.state.turn_running.load(Ordering::SeqCst) {
            return Err(CodexErr::UnsupportedOperation(
                "backend=claude_code agent is already running; use interrupt=true first"
                    .to_string(),
            ));
        }

        let resume_session_id = self.state.session_id.read().await.clone();
        let mut child = self.state.command.spawn(
            &self.state.config_snapshot,
            self.state.developer_instructions.as_deref(),
            resume_session_id.as_deref(),
        )?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| CodexErr::Fatal("Claude Code child missing stdin pipe".to_string()))?;
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to write Claude prompt: {err}")))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to terminate Claude prompt: {err}")))?;
        stdin
            .shutdown()
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to close Claude stdin: {err}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CodexErr::Fatal("Claude Code child missing stdout pipe".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CodexErr::Fatal("Claude Code child missing stderr pipe".to_string()))?;

        *self.state.child.lock().await = Some(child);
        *self.state.stdout_task.lock().await = Some(tokio::spawn(read_pipe_to_string(stdout)));
        *self.state.stderr_task.lock().await = Some(tokio::spawn(read_pipe_to_string(stderr)));
        self.state.shutdown_requested.store(false, Ordering::SeqCst);
        self.state
            .interrupt_requested
            .store(false, Ordering::SeqCst);
        self.state.turn_running.store(true, Ordering::SeqCst);
        let generation = self.state.turn_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.status_tx.send_replace(AgentStatus::Running);
        self.start_poll_task(generation);

        Ok(format!(
            "claude-code-turn-{}",
            self.state.submission_counter.fetch_add(1, Ordering::SeqCst)
        ))
    }

    async fn interrupt(&self) -> CodexResult<String> {
        let _guard = self.state.turn_lock.lock().await;
        self.refresh_process_status().await;

        let mut child_guard = self.state.child.lock().await;
        let Some(child) = child_guard.as_mut() else {
            return Ok(String::new());
        };

        if let Some(exit_status) = child
            .try_wait()
            .map_err(|err| CodexErr::Fatal(format!("failed to poll Claude child: {err}")))?
        {
            let _ = child_guard.take();
            drop(child_guard);
            self.finish_process(exit_status).await;
            return Ok(String::new());
        }

        self.state.interrupt_requested.store(true, Ordering::SeqCst);
        child
            .kill()
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to kill Claude child: {err}")))?;
        let exit_status = child
            .wait()
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to wait for Claude child: {err}")))?;
        let _ = child_guard.take();
        drop(child_guard);
        self.finish_process(exit_status).await;
        Ok(String::new())
    }

    async fn cleanup_after_internal_death(&self) {}

    async fn shutdown_live(&self) -> CodexResult<String> {
        let _guard = self.state.turn_lock.lock().await;
        self.refresh_process_status().await;

        let mut child_guard = self.state.child.lock().await;
        let Some(child) = child_guard.as_mut() else {
            return Ok(String::new());
        };

        if let Some(exit_status) = child
            .try_wait()
            .map_err(|err| CodexErr::Fatal(format!("failed to poll Claude child: {err}")))?
        {
            let _ = child_guard.take();
            drop(child_guard);
            self.finish_process(exit_status).await;
            return Ok(String::new());
        }

        self.state.shutdown_requested.store(true, Ordering::SeqCst);
        child
            .kill()
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to kill Claude child: {err}")))?;
        let exit_status = child
            .wait()
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to wait for Claude child: {err}")))?;
        let _ = child_guard.take();
        drop(child_guard);
        self.finish_process(exit_status).await;
        Ok(String::new())
    }

    async fn status(&self) -> AgentStatus {
        self.refresh_process_status().await;
        self.state.status_tx.borrow().clone()
    }

    async fn config_snapshot(&self) -> Option<ThreadConfigSnapshot> {
        Some(self.state.config_snapshot.clone())
    }

    async fn subscribe_status(&self) -> CodexResult<watch::Receiver<AgentStatus>> {
        self.refresh_process_status().await;
        Ok(self.state.status_tx.subscribe())
    }

    async fn total_token_usage(&self) -> Option<TokenUsage> {
        self.refresh_process_status().await;
        self.state.total_token_usage.read().await.clone()
    }

    async fn resume_state(&self) -> ClaudeCodeResumeState {
        let status = {
            let current = self.state.status_tx.borrow();
            current.clone()
        };
        let session_id = self.state.session_id.read().await.clone();
        let total_token_usage = self.state.total_token_usage.read().await.clone();
        ClaudeCodeResumeState {
            agent_id: self.state.agent_id,
            config_snapshot: self.state.config_snapshot.clone(),
            developer_instructions: self.state.developer_instructions.clone(),
            command: self.state.command.clone(),
            session_id,
            status,
            total_token_usage,
        }
    }

    fn start_poll_task(&self, generation: u64) {
        let handle = self.clone();
        tokio::spawn(async move {
            loop {
                if !handle.state.turn_running.load(Ordering::SeqCst)
                    || handle.state.turn_generation.load(Ordering::SeqCst) != generation
                {
                    break;
                }
                handle.refresh_process_status().await;
                sleep(Duration::from_millis(100)).await;
            }
        });
    }

    async fn refresh_process_status(&self) {
        if !self.state.turn_running.load(Ordering::SeqCst) {
            return;
        }

        let exit_status = {
            let mut child_guard = self.state.child.lock().await;
            let Some(child) = child_guard.as_mut() else {
                return;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    let _ = child_guard.take();
                    Some(status)
                }
                Ok(None) => None,
                Err(err) => {
                    let _ = child_guard.take();
                    self.state
                        .status_tx
                        .send_replace(AgentStatus::Errored(format!(
                            "failed to poll Claude child: {err}"
                        )));
                    self.state.turn_running.store(false, Ordering::SeqCst);
                    None
                }
            }
        };

        if let Some(exit_status) = exit_status {
            self.finish_process(exit_status).await;
        }
    }

    async fn finish_process(&self, exit_status: ExitStatus) {
        if !self.state.turn_running.swap(false, Ordering::SeqCst) {
            return;
        }

        let stdout = take_join_output(&self.state.stdout_task).await;
        let stderr = take_join_output(&self.state.stderr_task).await;
        let parsed = parse_claude_json_output(&stdout, &stderr);

        if let Some(session_id) = parsed.session_id.as_ref() {
            *self.state.session_id.write().await = Some(session_id.clone());
        }

        if let Some(token_usage) = parsed.token_usage.clone() {
            let mut total = self.state.total_token_usage.write().await;
            *total = Some(match total.clone() {
                Some(existing) => add_token_usage(existing, token_usage),
                None => token_usage,
            });
        }

        if self.state.shutdown_requested.load(Ordering::SeqCst) {
            self.state.status_tx.send_replace(AgentStatus::Shutdown);
            return;
        }

        if self.state.interrupt_requested.load(Ordering::SeqCst) {
            self.state.status_tx.send_replace(AgentStatus::Interrupted);
            return;
        }

        let next_status = if exit_status.success() {
            AgentStatus::Completed(parsed.message)
        } else {
            AgentStatus::Errored(parsed.error_message(exit_status))
        };
        self.state.status_tx.send_replace(next_status);
    }
}

struct ClaudeCodeSpawnedAgentState {
    manager: Weak<ThreadManagerState>,
    agent_id: ThreadId,
    config_snapshot: ThreadConfigSnapshot,
    developer_instructions: Option<String>,
    command: ClaudeCodeCommand,
    status_tx: watch::Sender<AgentStatus>,
    total_token_usage: RwLock<Option<TokenUsage>>,
    session_id: RwLock<Option<String>>,
    child: Mutex<Option<Child>>,
    stdout_task: Mutex<Option<JoinHandle<String>>>,
    stderr_task: Mutex<Option<JoinHandle<String>>>,
    turn_running: AtomicBool,
    shutdown_requested: AtomicBool,
    interrupt_requested: AtomicBool,
    submission_counter: AtomicU64,
    turn_generation: AtomicU64,
    turn_lock: Mutex<()>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClaudeCodeResumeState {
    pub(crate) agent_id: ThreadId,
    pub(crate) config_snapshot: ThreadConfigSnapshot,
    pub(crate) developer_instructions: Option<String>,
    pub(crate) command: ClaudeCodeCommand,
    pub(crate) session_id: Option<String>,
    pub(crate) status: AgentStatus,
    pub(crate) total_token_usage: Option<TokenUsage>,
}

#[derive(Clone)]
pub(crate) struct CommandSpawnedAgentHandle {
    state: Arc<CommandSpawnedAgentState>,
}

impl CommandSpawnedAgentHandle {
    #[allow(clippy::too_many_arguments)]
    fn new(
        manager: Weak<ThreadManagerState>,
        agent_id: ThreadId,
        backend_id: String,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
        command: CommandBackendCommand,
        healthcheck: Option<CommandBackendCommand>,
        healthcheck_timeout: Option<Duration>,
        turn_timeout: Option<Duration>,
        max_retries: u32,
        supports_resume: bool,
        supports_interrupt: bool,
        initial_status: AgentStatus,
        session_id: Option<String>,
        total_token_usage: Option<TokenUsage>,
    ) -> Self {
        let (status_tx, _) = watch::channel(initial_status);
        let state = Arc::new(CommandSpawnedAgentState {
            manager,
            agent_id,
            backend_id,
            config_snapshot,
            developer_instructions,
            command,
            healthcheck,
            healthcheck_timeout,
            turn_timeout,
            max_retries,
            supports_resume,
            supports_interrupt,
            status_tx,
            total_token_usage: RwLock::new(total_token_usage),
            session_id: RwLock::new(session_id),
            child: Mutex::new(None),
            stdout_task: Mutex::new(None),
            stderr_task: Mutex::new(None),
            inflight_turn: Mutex::new(None),
            turn_deadline: Mutex::new(None),
            turn_running: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            interrupt_requested: AtomicBool::new(false),
            timeout_requested: AtomicBool::new(false),
            submission_counter: AtomicU64::new(1),
            turn_generation: AtomicU64::new(0),
            turn_lock: Mutex::new(()),
        });

        Self { state }
    }

    fn from_resume_state(
        manager: Weak<ThreadManagerState>,
        state: CommandBackendResumeState,
    ) -> Self {
        Self::new(
            manager,
            state.agent_id,
            state.backend_id,
            state.config_snapshot,
            state.developer_instructions,
            state.command,
            state.healthcheck,
            state.healthcheck_timeout,
            state.turn_timeout,
            state.max_retries,
            state.supports_resume,
            state.supports_interrupt,
            state.status,
            state.session_id,
            state.total_token_usage,
        )
    }

    fn agent_id(&self) -> ThreadId {
        self.state.agent_id
    }

    async fn send_input(&self, items: Vec<UserInput>) -> CodexResult<String> {
        let history = load_rollout_messages(&self.state.manager, self.state.agent_id).await;
        let _guard = self.state.turn_lock.lock().await;
        self.refresh_process_status().await;
        if self.state.turn_running.load(Ordering::SeqCst) {
            return Err(CodexErr::UnsupportedOperation(format!(
                "backend={} agent is already running; use interrupt=true first",
                self.state.backend_id
            )));
        }

        *self.state.inflight_turn.lock().await = Some(InFlightCommandTurn {
            history,
            items,
            attempt: 0,
        });

        if let Err(failure) = self.start_attempt_with_retry().await {
            *self.state.inflight_turn.lock().await = None;
            self.state
                .status_tx
                .send_replace(AgentStatus::Errored(failure.message.clone()));
            return Err(CodexErr::UnsupportedOperation(failure.message));
        }

        Ok(format!(
            "{}-turn-{}",
            self.state.backend_id,
            self.state.submission_counter.fetch_add(1, Ordering::SeqCst)
        ))
    }

    async fn interrupt(&self) -> CodexResult<String> {
        if !self.state.supports_interrupt {
            return Err(CodexErr::UnsupportedOperation(format!(
                "backend '{}' does not support interrupt",
                self.state.backend_id
            )));
        }
        self.kill_running_process(/*shutdown*/ false).await
    }

    async fn cleanup_after_internal_death(&self) {}

    async fn shutdown_live(&self) -> CodexResult<String> {
        self.kill_running_process(/*shutdown*/ true).await
    }

    async fn status(&self) -> AgentStatus {
        self.refresh_process_status().await;
        self.state.status_tx.borrow().clone()
    }

    async fn config_snapshot(&self) -> Option<ThreadConfigSnapshot> {
        Some(self.state.config_snapshot.clone())
    }

    async fn subscribe_status(&self) -> CodexResult<watch::Receiver<AgentStatus>> {
        self.refresh_process_status().await;
        Ok(self.state.status_tx.subscribe())
    }

    async fn total_token_usage(&self) -> Option<TokenUsage> {
        self.refresh_process_status().await;
        self.state.total_token_usage.read().await.clone()
    }

    async fn resume_state(&self) -> Option<CommandBackendResumeState> {
        if !self.state.supports_resume {
            return None;
        }
        let status = {
            let current = self.state.status_tx.borrow();
            current.clone()
        };
        Some(CommandBackendResumeState {
            agent_id: self.state.agent_id,
            backend_id: self.state.backend_id.clone(),
            config_snapshot: self.state.config_snapshot.clone(),
            developer_instructions: self.state.developer_instructions.clone(),
            command: self.state.command.clone(),
            healthcheck: self.state.healthcheck.clone(),
            healthcheck_timeout: self.state.healthcheck_timeout,
            turn_timeout: self.state.turn_timeout,
            max_retries: self.state.max_retries,
            supports_resume: self.state.supports_resume,
            supports_interrupt: self.state.supports_interrupt,
            session_id: self.state.session_id.read().await.clone(),
            status,
            total_token_usage: self.state.total_token_usage.read().await.clone(),
        })
    }

    fn start_poll_task(&self, generation: u64) {
        let handle = self.clone();
        tokio::spawn(async move {
            loop {
                if !handle.state.turn_running.load(Ordering::SeqCst)
                    || handle.state.turn_generation.load(Ordering::SeqCst) != generation
                {
                    break;
                }
                handle.refresh_process_status().await;
                sleep(Duration::from_millis(100)).await;
            }
        });
    }

    async fn refresh_process_status(&self) {
        if !self.state.turn_running.load(Ordering::SeqCst) {
            return;
        }

        let deadline = *self.state.turn_deadline.lock().await;
        let exit_status = {
            let mut child_guard = self.state.child.lock().await;
            let Some(child) = child_guard.as_mut() else {
                return;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    let _ = child_guard.take();
                    Some(status)
                }
                Ok(None) => {
                    let timed_out = deadline.is_some_and(|deadline| Instant::now() >= deadline);
                    if timed_out {
                        self.state.timeout_requested.store(true, Ordering::SeqCst);
                        if let Err(err) = child.kill().await {
                            self.state
                                .status_tx
                                .send_replace(AgentStatus::Errored(format!(
                                    "failed to kill timed out backend '{}': {err}",
                                    self.state.backend_id
                                )));
                            self.state.turn_running.store(false, Ordering::SeqCst);
                            let _ = child_guard.take();
                            return;
                        }
                        match child.wait().await {
                            Ok(status) => {
                                let _ = child_guard.take();
                                Some(status)
                            }
                            Err(err) => {
                                self.state
                                    .status_tx
                                    .send_replace(AgentStatus::Errored(format!(
                                        "failed to wait for timed out backend '{}': {err}",
                                        self.state.backend_id
                                    )));
                                self.state.turn_running.store(false, Ordering::SeqCst);
                                let _ = child_guard.take();
                                None
                            }
                        }
                    } else {
                        None
                    }
                }
                Err(err) => {
                    let _ = child_guard.take();
                    self.state
                        .status_tx
                        .send_replace(AgentStatus::Errored(format!(
                            "failed to poll backend '{}': {err}",
                            self.state.backend_id
                        )));
                    self.state.turn_running.store(false, Ordering::SeqCst);
                    None
                }
            }
        };

        if let Some(exit_status) = exit_status {
            self.finish_process(exit_status).await;
        }
    }

    async fn kill_running_process(&self, shutdown: bool) -> CodexResult<String> {
        let _guard = self.state.turn_lock.lock().await;
        self.refresh_process_status().await;

        let mut child_guard = self.state.child.lock().await;
        let Some(child) = child_guard.as_mut() else {
            return Ok(String::new());
        };

        if let Some(exit_status) = child.try_wait().map_err(|err| {
            CodexErr::Fatal(format!(
                "failed to poll backend '{}' child: {err}",
                self.state.backend_id
            ))
        })? {
            let _ = child_guard.take();
            drop(child_guard);
            self.finish_process(exit_status).await;
            return Ok(String::new());
        }

        if shutdown {
            self.state.shutdown_requested.store(true, Ordering::SeqCst);
        } else {
            self.state.interrupt_requested.store(true, Ordering::SeqCst);
        }
        child.kill().await.map_err(|err| {
            CodexErr::Fatal(format!(
                "failed to kill backend '{}' child: {err}",
                self.state.backend_id
            ))
        })?;
        let exit_status = child.wait().await.map_err(|err| {
            CodexErr::Fatal(format!(
                "failed to wait for backend '{}' child: {err}",
                self.state.backend_id
            ))
        })?;
        let _ = child_guard.take();
        drop(child_guard);
        self.finish_process(exit_status).await;
        Ok(String::new())
    }

    async fn finish_process(&self, exit_status: ExitStatus) {
        if !self.state.turn_running.swap(false, Ordering::SeqCst) {
            return;
        }

        *self.state.turn_deadline.lock().await = None;
        let stdout = take_join_output(&self.state.stdout_task).await;
        let stderr = take_join_output(&self.state.stderr_task).await;
        let parsed = parse_external_json_output(&stdout, &stderr);

        if let Some(session_id) = parsed.session_id.as_ref() {
            *self.state.session_id.write().await = Some(session_id.clone());
        }

        if let Some(token_usage) = parsed.token_usage.clone() {
            let mut total = self.state.total_token_usage.write().await;
            *total = Some(match total.clone() {
                Some(existing) => add_token_usage(existing, token_usage),
                None => token_usage,
            });
        }

        if self.state.shutdown_requested.load(Ordering::SeqCst) {
            *self.state.inflight_turn.lock().await = None;
            self.state.status_tx.send_replace(AgentStatus::Shutdown);
            return;
        }

        if self.state.interrupt_requested.load(Ordering::SeqCst) {
            *self.state.inflight_turn.lock().await = None;
            self.state.status_tx.send_replace(AgentStatus::Interrupted);
            return;
        }

        let failure = if self.state.timeout_requested.swap(false, Ordering::SeqCst) {
            Some(CommandAttemptFailure::retryable(format!(
                "backend '{}' turn timed out after {}s",
                self.state.backend_id,
                self.state
                    .turn_timeout
                    .map(|duration| duration.as_secs())
                    .unwrap_or_default()
            )))
        } else if let Some(error) = parsed.error.as_ref() {
            Some(CommandAttemptFailure {
                message: error.display_message(&self.state.backend_id),
                retryable: error.retryable,
            })
        } else if exit_status.success() {
            None
        } else {
            Some(CommandAttemptFailure::non_retryable(
                parsed.error_message(exit_status),
            ))
        };

        if let Some(failure) = failure {
            if self.should_retry(&failure).await {
                let _guard = self.state.turn_lock.lock().await;
                if let Some(turn) = self.state.inflight_turn.lock().await.as_mut() {
                    turn.attempt += 1;
                }
                match self.start_attempt_with_retry().await {
                    Ok(()) => return,
                    Err(retry_failure) => {
                        *self.state.inflight_turn.lock().await = None;
                        self.state
                            .status_tx
                            .send_replace(AgentStatus::Errored(retry_failure.message));
                        return;
                    }
                }
            }

            *self.state.inflight_turn.lock().await = None;
            self.state
                .status_tx
                .send_replace(AgentStatus::Errored(failure.message));
            return;
        }

        *self.state.inflight_turn.lock().await = None;
        self.state
            .status_tx
            .send_replace(AgentStatus::Completed(parsed.message));
    }

    async fn should_retry(&self, failure: &CommandAttemptFailure) -> bool {
        if !failure.retryable {
            return false;
        }
        let attempt = self
            .state
            .inflight_turn
            .lock()
            .await
            .as_ref()
            .map(|turn| turn.attempt)
            .unwrap_or_default();
        attempt < self.state.max_retries
    }

    async fn start_attempt_with_retry(&self) -> Result<(), CommandAttemptFailure> {
        loop {
            match self.run_healthcheck_if_needed().await {
                Ok(()) => {}
                Err(failure) if self.should_retry(&failure).await => {
                    if let Some(turn) = self.state.inflight_turn.lock().await.as_mut() {
                        turn.attempt += 1;
                    }
                    continue;
                }
                Err(failure) => return Err(failure),
            }
            match self.start_current_attempt().await {
                Ok(()) => return Ok(()),
                Err(failure) if self.should_retry(&failure).await => {
                    if let Some(turn) = self.state.inflight_turn.lock().await.as_mut() {
                        turn.attempt += 1;
                    }
                }
                Err(failure) => return Err(failure),
            }
        }
    }

    async fn run_healthcheck_if_needed(&self) -> Result<(), CommandAttemptFailure> {
        let Some(command) = self.state.healthcheck.clone() else {
            return Ok(());
        };

        let mut child = command.spawn(&self.state.config_snapshot).map_err(|err| {
            CommandAttemptFailure::non_retryable(format!(
                "failed to launch backend '{}' healthcheck: {err}",
                self.state.backend_id
            ))
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.shutdown().await.map_err(|err| {
                CommandAttemptFailure::non_retryable(format!(
                    "failed to close backend '{}' healthcheck stdin: {err}",
                    self.state.backend_id
                ))
            })?;
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            CommandAttemptFailure::non_retryable(format!(
                "backend '{}' healthcheck child missing stdout pipe",
                self.state.backend_id
            ))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            CommandAttemptFailure::non_retryable(format!(
                "backend '{}' healthcheck child missing stderr pipe",
                self.state.backend_id
            ))
        })?;
        let stdout_task = tokio::spawn(read_pipe_to_string(stdout));
        let stderr_task = tokio::spawn(read_pipe_to_string(stderr));
        let deadline = self
            .state
            .healthcheck_timeout
            .map(|timeout| Instant::now() + timeout);

        let exit_status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    let timed_out = deadline.is_some_and(|deadline| Instant::now() >= deadline);
                    if timed_out {
                        child.kill().await.map_err(|err| {
                            CommandAttemptFailure::retryable(format!(
                                "failed to kill timed out backend '{}' healthcheck: {err}",
                                self.state.backend_id
                            ))
                        })?;
                        let _ = child.wait().await;
                        let _ = stdout_task.await;
                        let _ = stderr_task.await;
                        let timeout_secs = self
                            .state
                            .healthcheck_timeout
                            .map(|timeout| timeout.as_secs())
                            .unwrap_or_default();
                        return Err(CommandAttemptFailure::retryable(format!(
                            "backend '{}' healthcheck timed out after {}s",
                            self.state.backend_id, timeout_secs
                        )));
                    }
                    sleep(Duration::from_millis(50)).await;
                }
                Err(err) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return Err(CommandAttemptFailure::non_retryable(format!(
                        "failed to poll backend '{}' healthcheck: {err}",
                        self.state.backend_id
                    )));
                }
            }
        };

        let stdout = stdout_task.await.unwrap_or_default();
        let stderr = stderr_task.await.unwrap_or_default();
        let parsed = parse_external_json_output_with_label(&stdout, &stderr, "Backend healthcheck");

        if exit_status.success() && parsed.error.is_none() {
            return Ok(());
        }

        let failure = if let Some(error) = parsed.error {
            CommandAttemptFailure {
                message: format!(
                    "backend '{}' healthcheck failed: {}",
                    self.state.backend_id,
                    error.display_message(&self.state.backend_id)
                ),
                retryable: error.retryable,
            }
        } else {
            CommandAttemptFailure::non_retryable(format!(
                "backend '{}' healthcheck failed: {}",
                self.state.backend_id,
                parsed.error_message(exit_status)
            ))
        };
        Err(failure)
    }

    async fn start_current_attempt(&self) -> Result<(), CommandAttemptFailure> {
        let (history, items) = {
            let inflight = self.state.inflight_turn.lock().await;
            let turn = inflight.as_ref().ok_or_else(|| {
                CommandAttemptFailure::non_retryable(format!(
                    "backend '{}' missing in-flight turn state",
                    self.state.backend_id
                ))
            })?;
            (turn.history.clone(), turn.items.clone())
        };
        let request = build_command_backend_request(
            &self.state.backend_id,
            self.state.agent_id,
            &self.state.config_snapshot,
            self.state.developer_instructions.as_deref(),
            self.state.session_id.read().await.as_deref(),
            history,
            items,
        )
        .map_err(|err| CommandAttemptFailure::non_retryable(err.to_string()))?;

        let mut child = self.state.command.spawn(&self.state.config_snapshot).map_err(|err| {
            CommandAttemptFailure::non_retryable(format!(
                "failed to launch backend '{}': {err}",
                self.state.backend_id
            ))
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            CommandAttemptFailure::non_retryable(format!(
                "backend '{}' child missing stdin pipe",
                self.state.backend_id
            ))
        })?;
        if let Err(err) = stdin.write_all(request.as_bytes()).await {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(CommandAttemptFailure::retryable(format!(
                "failed to write backend '{}' request: {err}",
                self.state.backend_id
            )));
        }
        if let Err(err) = stdin.shutdown().await {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(CommandAttemptFailure::retryable(format!(
                "failed to close backend '{}' stdin: {err}",
                self.state.backend_id
            )));
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            CommandAttemptFailure::non_retryable(format!(
                "backend '{}' child missing stdout pipe",
                self.state.backend_id
            ))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            CommandAttemptFailure::non_retryable(format!(
                "backend '{}' child missing stderr pipe",
                self.state.backend_id
            ))
        })?;

        *self.state.child.lock().await = Some(child);
        *self.state.stdout_task.lock().await = Some(tokio::spawn(read_pipe_to_string(stdout)));
        *self.state.stderr_task.lock().await = Some(tokio::spawn(read_pipe_to_string(stderr)));
        *self.state.turn_deadline.lock().await = self
            .state
            .turn_timeout
            .map(|timeout| Instant::now() + timeout);
        self.state.shutdown_requested.store(false, Ordering::SeqCst);
        self.state
            .interrupt_requested
            .store(false, Ordering::SeqCst);
        self.state.timeout_requested.store(false, Ordering::SeqCst);
        self.state.turn_running.store(true, Ordering::SeqCst);
        let generation = self.state.turn_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.status_tx.send_replace(AgentStatus::Running);
        self.start_poll_task(generation);
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct InFlightCommandTurn {
    history: Vec<PromptHistoryMessage>,
    items: Vec<UserInput>,
    attempt: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandAttemptFailure {
    message: String,
    retryable: bool,
}

impl CommandAttemptFailure {
    fn non_retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }

    fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }
}

struct CommandSpawnedAgentState {
    manager: Weak<ThreadManagerState>,
    agent_id: ThreadId,
    backend_id: String,
    config_snapshot: ThreadConfigSnapshot,
    developer_instructions: Option<String>,
    command: CommandBackendCommand,
    healthcheck: Option<CommandBackendCommand>,
    healthcheck_timeout: Option<Duration>,
    turn_timeout: Option<Duration>,
    max_retries: u32,
    supports_resume: bool,
    supports_interrupt: bool,
    status_tx: watch::Sender<AgentStatus>,
    total_token_usage: RwLock<Option<TokenUsage>>,
    session_id: RwLock<Option<String>>,
    child: Mutex<Option<Child>>,
    stdout_task: Mutex<Option<JoinHandle<String>>>,
    stderr_task: Mutex<Option<JoinHandle<String>>>,
    inflight_turn: Mutex<Option<InFlightCommandTurn>>,
    turn_deadline: Mutex<Option<Instant>>,
    turn_running: AtomicBool,
    shutdown_requested: AtomicBool,
    interrupt_requested: AtomicBool,
    timeout_requested: AtomicBool,
    submission_counter: AtomicU64,
    turn_generation: AtomicU64,
    turn_lock: Mutex<()>,
}

#[derive(Clone, Debug)]
pub(crate) struct CommandBackendResumeState {
    pub(crate) agent_id: ThreadId,
    pub(crate) backend_id: String,
    pub(crate) config_snapshot: ThreadConfigSnapshot,
    pub(crate) developer_instructions: Option<String>,
    pub(crate) command: CommandBackendCommand,
    pub(crate) healthcheck: Option<CommandBackendCommand>,
    pub(crate) healthcheck_timeout: Option<Duration>,
    pub(crate) turn_timeout: Option<Duration>,
    pub(crate) max_retries: u32,
    pub(crate) supports_resume: bool,
    pub(crate) supports_interrupt: bool,
    pub(crate) session_id: Option<String>,
    pub(crate) status: AgentStatus,
    pub(crate) total_token_usage: Option<TokenUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandBackendCommand {
    program: String,
    args: Vec<String>,
    working_dir: Option<PathBuf>,
    env: BTreeMap<String, String>,
}

impl CommandBackendCommand {
    fn from_config(config: &SpawnedAgentBackendConfig) -> Self {
        Self::from_parts(
            config.command.clone(),
            config.working_dir.clone(),
            config.env.clone(),
        )
    }

    fn from_healthcheck_config(config: &SpawnedAgentBackendConfig) -> Option<Self> {
        config.healthcheck.clone().map(|command| {
            Self::from_parts(command, config.working_dir.clone(), config.env.clone())
        })
    }

    fn from_prefix(command_prefix: Vec<String>) -> Self {
        Self::from_parts(command_prefix, None, BTreeMap::new())
    }

    fn from_parts(
        mut command_prefix: Vec<String>,
        working_dir: Option<PathBuf>,
        env: BTreeMap<String, String>,
    ) -> Self {
        let program = command_prefix
            .drain(..1)
            .next()
            .unwrap_or_else(|| "sh".to_string());
        Self {
            program,
            args: command_prefix,
            working_dir,
            env,
        }
    }

    fn spawn(&self, config_snapshot: &ThreadConfigSnapshot) -> CodexResult<Child> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        let working_dir = self
            .working_dir
            .as_ref()
            .map(|path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    config_snapshot.cwd.join(path)
                }
            })
            .unwrap_or_else(|| config_snapshot.cwd.clone());
        command.current_dir(working_dir);
        command.envs(self.env.iter());
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.spawn().map_err(|err| {
            CodexErr::UnsupportedOperation(format!(
                "failed to launch backend '{}': {err}",
                self.program
            ))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaudeCodeCommand {
    program: String,
    args: Vec<String>,
    clear_env_keys: Vec<String>,
}

impl ClaudeCodeCommand {
    fn default_command() -> Self {
        #[cfg(test)]
        {
            if let Some(command) = DEFAULT_CLAUDE_CODE_COMMAND_OVERRIDE
                .lock()
                .expect("test command override lock poisoned")
                .clone()
            {
                return command;
            }
        }

        Self::from_prefix(["cps", "claude"].into_iter().map(str::to_owned).collect())
    }

    pub(crate) fn from_config(config: &crate::config::Config) -> Self {
        if let Some(command) = config.claude_code_backend_command.clone() {
            return Self::from_prefix(command);
        }
        Self::default_command()
    }

    pub(crate) fn from_command_prefix(command_prefix: Vec<String>) -> Self {
        Self::from_prefix(command_prefix)
    }

    fn from_prefix(mut command_prefix: Vec<String>) -> Self {
        let program = command_prefix
            .drain(..1)
            .next()
            .unwrap_or_else(|| "cps".to_string());
        let mut args = command_prefix;
        args.extend(
            Self::required_suffix_args()
                .iter()
                .map(|part| (*part).to_string()),
        );

        Self {
            program,
            args,
            clear_env_keys: vec![
                "ENV_PREFIX_TARGET_CMD".to_string(),
                "ENV_PREFIX_DEFAULT_ARGS".to_string(),
            ],
        }
    }

    fn required_suffix_args() -> &'static [&'static str] {
        &[
            "-p",
            "--output-format",
            "json",
            "--no-session-persistence",
            "--tools",
            "",
        ]
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            clear_env_keys: Vec::new(),
        }
    }

    fn spawn(
        &self,
        config_snapshot: &ThreadConfigSnapshot,
        developer_instructions: Option<&str>,
        resume_session_id: Option<&str>,
    ) -> CodexResult<Child> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command.current_dir(&config_snapshot.cwd);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        for key in &self.clear_env_keys {
            command.env_remove(key);
        }
        if let Some(developer_instructions) = developer_instructions
            && !developer_instructions.trim().is_empty()
        {
            command.arg("--append-system-prompt");
            command.arg(developer_instructions);
        }
        if let Some(session_id) = resume_session_id {
            command.arg("--resume");
            command.arg(session_id);
        }
        if !config_snapshot.model.trim().is_empty() && config_snapshot.model != "claude-code" {
            command.arg("--model");
            command.arg(&config_snapshot.model);
        }
        if let Some(effort) = claude_effort_arg(config_snapshot.reasoning_effort) {
            command.arg("--effort");
            command.arg(effort);
        }
        command.spawn().map_err(|err| {
            CodexErr::UnsupportedOperation(format!(
                "failed to launch Claude Code backend '{}': {err}",
                self.program
            ))
        })
    }
}

#[cfg(test)]
static DEFAULT_CLAUDE_CODE_COMMAND_OVERRIDE: LazyLock<StdMutex<Option<ClaudeCodeCommand>>> =
    LazyLock::new(|| StdMutex::new(None));

#[cfg(test)]
static CLAUDE_CODE_TEST_LOCK: LazyLock<Arc<tokio::sync::Mutex<()>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));

#[cfg(test)]
pub(crate) async fn acquire_claude_code_test_lock() -> tokio::sync::OwnedMutexGuard<()> {
    CLAUDE_CODE_TEST_LOCK.clone().lock_owned().await
}

#[cfg(test)]
pub(crate) fn set_default_claude_code_command_for_test(command: Option<ClaudeCodeCommand>) {
    *DEFAULT_CLAUDE_CODE_COMMAND_OVERRIDE
        .lock()
        .expect("test command override lock poisoned") = command;
}

#[derive(Serialize)]
struct CommandBackendRequest {
    protocol: &'static str,
    backend_id: String,
    thread_id: String,
    cwd: String,
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
    developer_instructions: Option<String>,
    session_id: Option<String>,
    history: Vec<PromptHistoryMessage>,
    items: Vec<UserInput>,
}

#[derive(Clone, Debug, Serialize)]
struct PromptHistoryMessage {
    role: String,
    content: String,
}

struct ExternalParsedOutput {
    backend_label: String,
    message: Option<String>,
    error: Option<ExternalBackendError>,
    token_usage: Option<TokenUsage>,
    session_id: Option<String>,
    stderr: String,
}

impl ExternalParsedOutput {
    fn explicit_error_message(&self) -> Option<String> {
        self.error
            .as_ref()
            .map(|error| error.display_message(&self.backend_label))
    }

    fn error_message(&self, exit_status: ExitStatus) -> String {
        if let Some(message) = self.explicit_error_message() {
            return message;
        }
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_string();
        }
        match &self.message {
            Some(message) if !message.trim().is_empty() => message.clone(),
            _ => format!("{} exited with status {exit_status}", self.backend_label),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExternalBackendError {
    message: String,
    code: Option<String>,
    retryable: bool,
}

impl ExternalBackendError {
    fn display_message(&self, backend_label: &str) -> String {
        let code_suffix = self
            .code
            .as_deref()
            .filter(|code| !code.trim().is_empty())
            .map(|code| format!(" [{code}]"))
            .unwrap_or_default();
        format!("{backend_label}{code_suffix}: {}", self.message)
    }
}

async fn read_pipe_to_string<T>(mut reader: T) -> String
where
    T: tokio::io::AsyncRead + Unpin,
{
    let mut buf = String::new();
    let _ = reader.read_to_string(&mut buf).await;
    buf
}

async fn take_join_output(task_slot: &Mutex<Option<JoinHandle<String>>>) -> String {
    let task = task_slot.lock().await.take();
    match task {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    }
}

async fn load_rollout_messages(
    manager: &Weak<ThreadManagerState>,
    agent_id: ThreadId,
) -> Vec<PromptHistoryMessage> {
    let Some(state) = manager.upgrade() else {
        return Vec::new();
    };
    let Ok(thread) = state.get_thread(agent_id).await else {
        return Vec::new();
    };
    thread.codex.session.ensure_rollout_materialized().await;
    thread.codex.session.flush_rollout().await;
    let Some(rollout_path) = thread.rollout_path() else {
        return Vec::new();
    };
    let Ok(history) = RolloutRecorder::get_rollout_history(&rollout_path).await else {
        return Vec::new();
    };

    history
        .get_rollout_items()
        .into_iter()
        .filter_map(|item| match item {
            crate::protocol::RolloutItem::ResponseItem(ResponseItem::Message {
                role,
                content,
                ..
            }) => Some(PromptHistoryMessage {
                role,
                content: content_items_to_text(&content),
            }),
            _ => None,
        })
        .filter(|item| !item.content.trim().is_empty())
        .collect()
}

async fn load_rollout_prompt_context(
    manager: &Weak<ThreadManagerState>,
    agent_id: ThreadId,
) -> Vec<PromptHistoryMessage> {
    load_rollout_messages(manager, agent_id).await
}

fn render_claude_prompt(history: &[PromptHistoryMessage], items: &[UserInput]) -> String {
    let mut sections = Vec::new();
    if !history.is_empty() {
        let history_text = history
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("Conversation history:\n{history_text}"));
    }

    let input_text = render_input_items(items);
    if !input_text.is_empty() {
        sections.push(format!("New input:\n{input_text}"));
    }

    sections.join("\n\n")
}

fn render_input_items(items: &[UserInput]) -> String {
    items
        .iter()
        .map(render_input_item)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_input_item(item: &UserInput) -> String {
    match item {
        UserInput::Text { text, .. } => text.clone(),
        UserInput::Image { .. } => "[image]".to_string(),
        UserInput::LocalImage { path } => format!("[local_image:{}]", path.display()),
        UserInput::Skill { name, path } => format!("[skill:${name}]({})", path.display()),
        UserInput::Mention { name, path } => format!("[mention:${name}]({path})"),
        _ => "[input]".to_string(),
    }
}

fn build_command_backend_request(
    backend_id: &str,
    agent_id: ThreadId,
    config_snapshot: &ThreadConfigSnapshot,
    developer_instructions: Option<&str>,
    session_id: Option<&str>,
    history: Vec<PromptHistoryMessage>,
    items: Vec<UserInput>,
) -> CodexResult<String> {
    serde_json::to_string(&CommandBackendRequest {
        protocol: "json_stdio_v1",
        backend_id: backend_id.to_string(),
        thread_id: agent_id.to_string(),
        cwd: config_snapshot.cwd.display().to_string(),
        model: config_snapshot.model.clone(),
        reasoning_effort: config_snapshot.reasoning_effort,
        developer_instructions: developer_instructions
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        session_id: session_id.map(ToOwned::to_owned),
        history,
        items,
    })
    .map_err(|err| CodexErr::Fatal(format!("failed to serialize backend request: {err}")))
}

fn content_items_to_text(items: &[ContentItem]) -> String {
    items
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                Some(text.clone())
            }
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn claude_effort_arg(reasoning_effort: Option<ReasoningEffort>) -> Option<&'static str> {
    match reasoning_effort.unwrap_or(ReasoningEffort::Medium) {
        ReasoningEffort::None | ReasoningEffort::Minimal | ReasoningEffort::Low => Some("low"),
        ReasoningEffort::Medium => Some("medium"),
        ReasoningEffort::High => Some("high"),
        ReasoningEffort::XHigh => Some("max"),
    }
}

fn parse_claude_json_output(stdout: &str, stderr: &str) -> ExternalParsedOutput {
    parse_external_json_output_with_label(stdout, stderr, "Claude Code")
}

fn parse_external_json_output(stdout: &str, stderr: &str) -> ExternalParsedOutput {
    parse_external_json_output_with_label(stdout, stderr, "External backend")
}

fn parse_external_json_output_with_label(
    stdout: &str,
    stderr: &str,
    backend_label: &str,
) -> ExternalParsedOutput {
    let parsed_json = serde_json::from_str::<JsonValue>(stdout).ok();
    let message = parsed_json
        .as_ref()
        .and_then(find_claude_message)
        .or_else(|| non_empty_trimmed(stdout).map(ToOwned::to_owned));
    let error = parsed_json.as_ref().and_then(find_external_error);
    let token_usage = parsed_json.as_ref().and_then(find_claude_token_usage);
    let session_id = parsed_json.as_ref().and_then(find_claude_session_id);

    ExternalParsedOutput {
        backend_label: backend_label.to_string(),
        message,
        error,
        token_usage,
        session_id,
        stderr: stderr.to_string(),
    }
}

fn find_external_error(value: &JsonValue) -> Option<ExternalBackendError> {
    match value {
        JsonValue::Array(items) => items.iter().rev().find_map(find_external_error),
        JsonValue::Object(map) => {
            let error_value = map.get("error");
            if let Some(error) = error_value.and_then(external_error_from_value) {
                return Some(error);
            }

            let message = map
                .get("error_message")
                .and_then(JsonValue::as_str)
                .and_then(non_empty_trimmed)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    map.get("error")
                        .and_then(JsonValue::as_str)
                        .and_then(non_empty_trimmed)
                        .map(ToOwned::to_owned)
                });
            message.map(|message| ExternalBackendError {
                message,
                code: map
                    .get("error_code")
                    .or_else(|| map.get("code"))
                    .and_then(JsonValue::as_str)
                    .and_then(non_empty_trimmed)
                    .map(ToOwned::to_owned),
                retryable: map
                    .get("retryable")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false),
            })
        }
        _ => None,
    }
}

fn external_error_from_value(value: &JsonValue) -> Option<ExternalBackendError> {
    match value {
        JsonValue::String(message) => non_empty_trimmed(message).map(|message| ExternalBackendError {
            message: message.to_string(),
            code: None,
            retryable: false,
        }),
        JsonValue::Object(map) => {
            let message = map
                .get("message")
                .or_else(|| map.get("error"))
                .and_then(JsonValue::as_str)
                .and_then(non_empty_trimmed)?
                .to_string();
            Some(ExternalBackendError {
                message,
                code: map
                    .get("code")
                    .or_else(|| map.get("error_code"))
                    .and_then(JsonValue::as_str)
                    .and_then(non_empty_trimmed)
                    .map(ToOwned::to_owned),
                retryable: map
                    .get("retryable")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false),
            })
        }
        _ => None,
    }
}

fn find_claude_session_id(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Array(items) => items.iter().rev().find_map(find_claude_session_id),
        JsonValue::Object(map) => map
            .get("session_id")
            .and_then(JsonValue::as_str)
            .and_then(non_empty_trimmed)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn find_claude_message(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Array(items) => items.iter().rev().find_map(find_claude_message),
        JsonValue::Object(map) => {
            if let Some(result) = map.get("result").and_then(JsonValue::as_str) {
                return non_empty_trimmed(result).map(ToOwned::to_owned);
            }
            if let Some(message) = map.get("message").and_then(JsonValue::as_str) {
                return non_empty_trimmed(message).map(ToOwned::to_owned);
            }
            if let Some(text) = map.get("text").and_then(JsonValue::as_str) {
                return non_empty_trimmed(text).map(ToOwned::to_owned);
            }
            if let Some(content) = map.get("content").and_then(JsonValue::as_array) {
                let joined = content
                    .iter()
                    .filter_map(|item| match item {
                        JsonValue::Object(item_map) => item_map
                            .get("text")
                            .and_then(JsonValue::as_str)
                            .map(str::to_owned),
                        JsonValue::String(text) => Some(text.clone()),
                        _ => None,
                    })
                    .filter(|text| !text.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
            None
        }
        JsonValue::String(text) => non_empty_trimmed(text).map(ToOwned::to_owned),
        _ => None,
    }
}

fn find_claude_token_usage(value: &JsonValue) -> Option<TokenUsage> {
    match value {
        JsonValue::Array(items) => items.iter().find_map(find_claude_token_usage),
        JsonValue::Object(map) => {
            let usage = map.get("usage").and_then(token_usage_from_object);
            if usage.is_some() {
                return usage;
            }
            token_usage_from_object(value)
        }
        _ => None,
    }
}

fn token_usage_from_object(value: &JsonValue) -> Option<TokenUsage> {
    let JsonValue::Object(map) = value else {
        return None;
    };

    let input_tokens = json_i64(map.get("inputTokens").or_else(|| map.get("input_tokens")))?;
    let output_tokens = json_i64(map.get("outputTokens").or_else(|| map.get("output_tokens")))?;
    let cached_input_tokens = json_i64(
        map.get("cacheReadInputTokens")
            .or_else(|| map.get("cached_input_tokens")),
    )
    .unwrap_or_default();
    let reasoning_output_tokens = json_i64(
        map.get("reasoningOutputTokens")
            .or_else(|| map.get("reasoning_output_tokens")),
    )
    .unwrap_or_default();

    Some(TokenUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens: input_tokens + cached_input_tokens + output_tokens + reasoning_output_tokens,
    })
}

fn json_i64(value: Option<&JsonValue>) -> Option<i64> {
    value.and_then(|value| match value {
        JsonValue::Number(number) => number.as_i64(),
        JsonValue::String(text) => text.parse::<i64>().ok(),
        _ => None,
    })
}

fn non_empty_trimmed(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn add_token_usage(left: TokenUsage, right: TokenUsage) -> TokenUsage {
    TokenUsage {
        input_tokens: left.input_tokens + right.input_tokens,
        cached_input_tokens: left.cached_input_tokens + right.cached_input_tokens,
        output_tokens: left.output_tokens + right.output_tokens,
        reasoning_output_tokens: left.reasoning_output_tokens + right.reasoning_output_tokens,
        total_tokens: left.total_tokens + right.total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::config_types::ApprovalsReviewer;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::SandboxPolicy;
    use codex_protocol::protocol::SessionSource;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::time::timeout;

    fn test_snapshot(cwd: PathBuf) -> ThreadConfigSnapshot {
        ThreadConfigSnapshot {
            agent_backend_id: "claude_code".to_string(),
            model: "claude-code".to_string(),
            model_provider_id: "external".to_string(),
            service_tier: None,
            approval_policy: AskForApproval::UnlessTrusted,
            approvals_reviewer: ApprovalsReviewer::User,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            cwd,
            ephemeral: false,
            reasoning_effort: Some(ReasoningEffort::High),
            personality: None,
            session_source: SessionSource::Exec,
        }
    }

    fn text_input(text: &str) -> Vec<UserInput> {
        vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]
    }

    async fn wait_for_final_status(handle: &CommandSpawnedAgentHandle) -> AgentStatus {
        let mut status_rx = handle.subscribe_status().await.expect("subscribe");
        timeout(Duration::from_secs(10), async {
            loop {
                let current = status_rx.borrow().clone();
                if matches!(
                    current,
                    AgentStatus::Completed(_)
                        | AgentStatus::Errored(_)
                        | AgentStatus::Interrupted
                        | AgentStatus::Shutdown
                ) {
                    break current;
                }
                status_rx.changed().await.expect("status update");
            }
        })
        .await
        .expect("wait for final status")
    }

    #[tokio::test]
    async fn claude_code_handle_completes_and_parses_usage() {
        let tempdir = TempDir::new().expect("tempdir");
        let output = r#"[{"type":"result","result":"OK","usage":{"inputTokens":10,"outputTokens":2,"cacheReadInputTokens":1,"reasoningOutputTokens":3}}]"#;
        let command = ClaudeCodeCommand::new_for_test(
            "bash",
            vec![
                "-lc".to_string(),
                format!("printf '%s' '{}'", output.replace('\'', r"'\''")),
            ],
        );
        let handle = SpawnedAgentHandle::claude_code_for_test(
            ThreadId::new(),
            test_snapshot(tempdir.path().to_path_buf()),
            Some("review carefully".to_string()),
            text_input("say ok"),
            command,
        )
        .await
        .expect("spawn handle");

        let mut status_rx = handle.subscribe_status().await.expect("subscribe");
        let status = timeout(Duration::from_secs(5), async {
            loop {
                let current = status_rx.borrow().clone();
                if matches!(current, AgentStatus::Completed(_)) {
                    break current;
                }
                status_rx.changed().await.expect("status update");
            }
        })
        .await
        .expect("wait for completion");
        assert_eq!(status, AgentStatus::Completed(Some("OK".to_string())));

        let usage = handle.total_token_usage().await.expect("token usage");
        assert_eq!(
            usage,
            TokenUsage {
                input_tokens: 10,
                cached_input_tokens: 1,
                output_tokens: 2,
                reasoning_output_tokens: 3,
                total_tokens: 16,
            }
        );
    }

    #[tokio::test]
    async fn claude_code_handle_shutdown_sets_shutdown_status() {
        let tempdir = TempDir::new().expect("tempdir");
        let command = ClaudeCodeCommand::new_for_test(
            "bash",
            vec![
                "-lc".to_string(),
                "sleep 10; printf '%s' '[{\"type\":\"result\",\"result\":\"late\"}]'".to_string(),
            ],
        );
        let handle = SpawnedAgentHandle::claude_code_for_test(
            ThreadId::new(),
            test_snapshot(tempdir.path().to_path_buf()),
            /*developer_instructions*/ None,
            text_input("long task"),
            command,
        )
        .await
        .expect("spawn handle");

        let mut status_rx = handle.subscribe_status().await.expect("subscribe");
        assert_eq!(status_rx.borrow().clone(), AgentStatus::Running);

        handle.shutdown_live().await.expect("shutdown");

        let status = timeout(Duration::from_secs(5), async {
            loop {
                let current = status_rx.borrow().clone();
                if current == AgentStatus::Shutdown {
                    break current;
                }
                status_rx.changed().await.expect("status update");
            }
        })
        .await
        .expect("wait for shutdown");
        assert_eq!(status, AgentStatus::Shutdown);

        let SpawnedAgentHandle::ClaudeCode(handle) = handle else {
            panic!("expected ClaudeCode handle");
        };
        assert!(!handle.state.turn_running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn command_backend_handle_completes_and_records_resume_state() {
        let tempdir = TempDir::new().expect("tempdir");
        let handle = CommandSpawnedAgentHandle::new(
            Weak::new(),
            ThreadId::new(),
            "gemini_bridge".to_string(),
            test_snapshot(tempdir.path().to_path_buf()),
            Some("inspect carefully".to_string()),
            CommandBackendCommand {
                program: "bash".to_string(),
                args: vec![
                    "-lc".to_string(),
                    r#"input=$(cat); case "$input" in *"say ok"*) result='cmd ok' ;; *) result='missing input' ;; esac; printf '{"message":"%s","usage":{"input_tokens":4,"cached_input_tokens":1,"output_tokens":2,"reasoning_output_tokens":3},"session_id":"cmd-session-1"}' "$result""#.to_string(),
                ],
                working_dir: None,
                env: BTreeMap::new(),
            },
            /*healthcheck*/ None,
            /*healthcheck_timeout*/ None,
            /*turn_timeout*/ None,
            /*max_retries*/ 0,
            /*supports_resume*/ true,
            /*supports_interrupt*/ false,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );

        handle
            .send_input(text_input("say ok"))
            .await
            .expect("send input");

        let mut status_rx = handle.subscribe_status().await.expect("subscribe");
        let status = timeout(Duration::from_secs(5), async {
            loop {
                let current = status_rx.borrow().clone();
                if matches!(current, AgentStatus::Completed(_)) {
                    break current;
                }
                status_rx.changed().await.expect("status update");
            }
        })
        .await
        .expect("wait for completion");
        assert_eq!(status, AgentStatus::Completed(Some("cmd ok".to_string())));

        let usage = handle.total_token_usage().await.expect("token usage");
        assert_eq!(
            usage,
            TokenUsage {
                input_tokens: 4,
                cached_input_tokens: 1,
                output_tokens: 2,
                reasoning_output_tokens: 3,
                total_tokens: 10,
            }
        );

        let resume_state = handle.resume_state().await.expect("resume state");
        assert_eq!(resume_state.backend_id, "gemini_bridge");
        assert_eq!(resume_state.session_id.as_deref(), Some("cmd-session-1"));
        assert_eq!(resume_state.total_token_usage, Some(usage));
    }

    #[tokio::test]
    async fn command_backend_honors_working_dir_and_env_overrides() {
        let tempdir = TempDir::new().expect("tempdir");
        let backend_dir = tempdir.path().join("backend");
        std::fs::create_dir_all(&backend_dir).expect("backend dir");

        let handle = CommandSpawnedAgentHandle::new(
            Weak::new(),
            ThreadId::new(),
            "gemini_bridge".to_string(),
            test_snapshot(tempdir.path().to_path_buf()),
            None,
            CommandBackendCommand {
                program: "bash".to_string(),
                args: vec![
                    "-lc".to_string(),
                    r#"printf '{"message":"cwd=%s env=%s"}' "$(pwd)" "${BACKEND_MARKER:-missing}""#
                        .to_string(),
                ],
                working_dir: Some(PathBuf::from("backend")),
                env: BTreeMap::from([("BACKEND_MARKER".to_string(), "gemini-ok".to_string())]),
            },
            /*healthcheck*/ None,
            /*healthcheck_timeout*/ None,
            /*turn_timeout*/ None,
            /*max_retries*/ 0,
            /*supports_resume*/ false,
            /*supports_interrupt*/ false,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );

        handle
            .send_input(text_input("ignored"))
            .await
            .expect("send input");

        let mut status_rx = handle.subscribe_status().await.expect("subscribe");
        let status = timeout(Duration::from_secs(5), async {
            loop {
                let current = status_rx.borrow().clone();
                if matches!(current, AgentStatus::Completed(_)) {
                    break current;
                }
                status_rx.changed().await.expect("status update");
            }
        })
        .await
        .expect("wait for completion");

        let AgentStatus::Completed(Some(message)) = status else {
            panic!("expected completed status");
        };
        assert!(message.contains("cwd="), "{message}");
        assert!(message.ends_with("backend env=gemini-ok"), "{message}");
    }

    #[tokio::test]
    async fn command_backend_retries_retryable_structured_errors() {
        let tempdir = TempDir::new().expect("tempdir");
        let attempt_file = tempdir.path().join("retry-attempt.txt");

        let handle = CommandSpawnedAgentHandle::new(
            Weak::new(),
            ThreadId::new(),
            "gemini_bridge".to_string(),
            test_snapshot(tempdir.path().to_path_buf()),
            None,
            CommandBackendCommand {
                program: "bash".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!(
                        r#"attempt_file='{}'; attempt=$(cat "$attempt_file" 2>/dev/null || printf 0); attempt=$((attempt + 1)); printf '%s' "$attempt" > "$attempt_file"; if [ "$attempt" -eq 1 ]; then printf '%s' '{{"error":{{"message":"warming up","code":"backend_warming","retryable":true}}}}'; exit 1; fi; printf '%s' '{{"message":"retry ok"}}'"#,
                        attempt_file.display()
                    ),
                ],
                working_dir: None,
                env: BTreeMap::new(),
            },
            /*healthcheck*/ None,
            /*healthcheck_timeout*/ None,
            /*turn_timeout*/ None,
            /*max_retries*/ 1,
            /*supports_resume*/ false,
            /*supports_interrupt*/ false,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );

        handle
            .send_input(text_input("retry me"))
            .await
            .expect("send input");

        let status = wait_for_final_status(&handle).await;
        assert_eq!(status, AgentStatus::Completed(Some("retry ok".to_string())));
        assert_eq!(std::fs::read_to_string(&attempt_file).expect("attempt file"), "2");
    }

    #[tokio::test]
    async fn command_backend_turn_timeout_retries_when_configured() {
        let tempdir = TempDir::new().expect("tempdir");
        let first_attempt_marker = tempdir.path().join("timeout-first-attempt");

        let handle = CommandSpawnedAgentHandle::new(
            Weak::new(),
            ThreadId::new(),
            "gemini_bridge".to_string(),
            test_snapshot(tempdir.path().to_path_buf()),
            None,
            CommandBackendCommand {
                program: "bash".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!(
                        r#"marker='{}'; if [ ! -f "$marker" ]; then touch "$marker"; sleep 4; fi; printf '%s' '{{"message":"timeout retry ok"}}'"#,
                        first_attempt_marker.display()
                    ),
                ],
                working_dir: None,
                env: BTreeMap::new(),
            },
            /*healthcheck*/ None,
            /*healthcheck_timeout*/ None,
            /*turn_timeout*/ Some(Duration::from_secs(2)),
            /*max_retries*/ 1,
            /*supports_resume*/ false,
            /*supports_interrupt*/ false,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );

        handle
            .send_input(text_input("please retry on timeout"))
            .await
            .expect("send input");

        let status = wait_for_final_status(&handle).await;
        assert_eq!(
            status,
            AgentStatus::Completed(Some("timeout retry ok".to_string()))
        );
        assert!(first_attempt_marker.exists(), "first attempt marker missing");
    }

    #[tokio::test]
    async fn command_backend_runs_healthcheck_before_turn() {
        let tempdir = TempDir::new().expect("tempdir");
        let backend_dir = tempdir.path().join("backend");
        std::fs::create_dir_all(&backend_dir).expect("backend dir");
        let healthcheck_stamp = backend_dir.join("healthcheck.ok");

        let handle = CommandSpawnedAgentHandle::new(
            Weak::new(),
            ThreadId::new(),
            "gemini_bridge".to_string(),
            test_snapshot(tempdir.path().to_path_buf()),
            None,
            CommandBackendCommand {
                program: "bash".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!(
                        r#"stamp='{}'; if [ -f "$stamp" ]; then printf '%s' '{{"message":"healthcheck ok"}}'; else printf '%s' '{{"error":{{"message":"missing healthcheck stamp"}}}}'; exit 1; fi"#,
                        healthcheck_stamp.display()
                    ),
                ],
                working_dir: Some(PathBuf::from("backend")),
                env: BTreeMap::new(),
            },
            /*healthcheck*/ Some(CommandBackendCommand {
                program: "bash".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!(
                        r#"touch '{}'; printf '%s' '{{"status":"ok"}}'"#,
                        healthcheck_stamp.display()
                    ),
                ],
                working_dir: Some(PathBuf::from("backend")),
                env: BTreeMap::new(),
            }),
            /*healthcheck_timeout*/ Some(Duration::from_secs(2)),
            /*turn_timeout*/ None,
            /*max_retries*/ 0,
            /*supports_resume*/ false,
            /*supports_interrupt*/ false,
            AgentStatus::PendingInit,
            /*session_id*/ None,
            /*total_token_usage*/ None,
        );

        handle
            .send_input(text_input("healthcheck first"))
            .await
            .expect("send input");

        let status = wait_for_final_status(&handle).await;
        assert_eq!(
            status,
            AgentStatus::Completed(Some("healthcheck ok".to_string()))
        );
        assert!(healthcheck_stamp.exists(), "healthcheck stamp missing");
    }

    #[test]
    fn parse_backend_accepts_known_aliases() {
        assert_eq!(
            SpawnedAgentBackendKind::parse(Some("claude-code")).expect("backend"),
            SpawnedAgentBackendKind::ClaudeCode
        );
        assert_eq!(
            SpawnedAgentBackendKind::parse(Some("claude_code")).expect("backend"),
            SpawnedAgentBackendKind::ClaudeCode
        );
        assert_eq!(
            SpawnedAgentBackendKind::parse(/*value*/ None).expect("backend"),
            SpawnedAgentBackendKind::Codex
        );
    }
}
