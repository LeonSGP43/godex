use crate::agent::AgentStatus;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::thread_manager::ThreadManagerState;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::user_input::UserInput;
use serde_json::Value as JsonValue;
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
}

impl SpawnedAgentBackendKind {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Codex),
            Some("codex") => Ok(Self::Codex),
            Some("claude_code" | "claude-code" | "claudecode") => Ok(Self::ClaudeCode),
            Some(other) => Err(format!(
                "unknown backend '{other}'; expected one of: codex, claude_code"
            )),
        }
    }
}

#[derive(Clone)]
/// Per-agent runtime handle for spawned agents.
pub(crate) enum SpawnedAgentHandle {
    Codex(CodexSpawnedAgentHandle),
    ClaudeCode(ClaudeCodeSpawnedAgentHandle),
}

impl SpawnedAgentHandle {
    pub(crate) fn codex(manager: Weak<ThreadManagerState>, agent_id: ThreadId) -> Self {
        Self::Codex(CodexSpawnedAgentHandle::new(manager, agent_id))
    }

    pub(crate) async fn claude_code(
        agent_id: ThreadId,
        config_snapshot: ThreadConfigSnapshot,
        developer_instructions: Option<String>,
        items: Vec<UserInput>,
        command: ClaudeCodeCommand,
    ) -> CodexResult<Self> {
        let handle = ClaudeCodeSpawnedAgentHandle::new(
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

    pub(crate) fn resumed_claude_code(state: ClaudeCodeResumeState) -> Self {
        Self::ClaudeCode(ClaudeCodeSpawnedAgentHandle::from_resume_state(state))
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
            agent_id,
            config_snapshot,
            developer_instructions,
            command,
            AgentStatus::PendingInit,
            None,
            None,
        );
        handle.send_input(items).await?;
        Ok(Self::ClaudeCode(handle))
    }

    pub(crate) fn agent_id(&self) -> ThreadId {
        match self {
            Self::Codex(handle) => handle.agent_id,
            Self::ClaudeCode(handle) => handle.agent_id(),
        }
    }

    pub(crate) async fn send_input(&self, items: Vec<UserInput>) -> CodexResult<String> {
        match self {
            Self::Codex(handle) => handle.send_input(items).await,
            Self::ClaudeCode(handle) => handle.send_input(items).await,
        }
    }

    pub(crate) async fn interrupt(&self) -> CodexResult<String> {
        match self {
            Self::Codex(handle) => handle.interrupt().await,
            Self::ClaudeCode(handle) => handle.interrupt().await,
        }
    }

    pub(crate) async fn cleanup_after_internal_death(&self) {
        match self {
            Self::Codex(handle) => handle.cleanup_after_internal_death().await,
            Self::ClaudeCode(handle) => handle.cleanup_after_internal_death().await,
        }
    }

    pub(crate) async fn shutdown_live(&self) -> CodexResult<String> {
        match self {
            Self::Codex(handle) => handle.shutdown_live().await,
            Self::ClaudeCode(handle) => handle.shutdown_live().await,
        }
    }

    pub(crate) async fn status(&self) -> AgentStatus {
        match self {
            Self::Codex(handle) => handle.status().await,
            Self::ClaudeCode(handle) => handle.status().await,
        }
    }

    pub(crate) async fn config_snapshot(&self) -> Option<ThreadConfigSnapshot> {
        match self {
            Self::Codex(handle) => handle.config_snapshot().await,
            Self::ClaudeCode(handle) => handle.config_snapshot().await,
        }
    }

    pub(crate) async fn subscribe_status(&self) -> CodexResult<watch::Receiver<AgentStatus>> {
        match self {
            Self::Codex(handle) => handle.subscribe_status().await,
            Self::ClaudeCode(handle) => handle.subscribe_status().await,
        }
    }

    pub(crate) async fn total_token_usage(&self) -> Option<TokenUsage> {
        match self {
            Self::Codex(handle) => handle.total_token_usage().await,
            Self::ClaudeCode(handle) => handle.total_token_usage().await,
        }
    }

    pub(crate) async fn claude_code_resume_state(&self) -> Option<ClaudeCodeResumeState> {
        match self {
            Self::ClaudeCode(handle) => Some(handle.resume_state().await),
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

    fn from_resume_state(state: ClaudeCodeResumeState) -> Self {
        Self::new(
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
        let prompt = render_claude_prompt(&items);
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

struct ClaudeParsedOutput {
    message: Option<String>,
    token_usage: Option<TokenUsage>,
    session_id: Option<String>,
    stderr: String,
}

impl ClaudeParsedOutput {
    fn error_message(&self, exit_status: ExitStatus) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_string();
        }
        match &self.message {
            Some(message) if !message.trim().is_empty() => message.clone(),
            _ => format!("Claude Code exited with status {exit_status}"),
        }
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

fn render_claude_prompt(items: &[UserInput]) -> String {
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

fn claude_effort_arg(reasoning_effort: Option<ReasoningEffort>) -> Option<&'static str> {
    match reasoning_effort.unwrap_or(ReasoningEffort::Medium) {
        ReasoningEffort::None | ReasoningEffort::Minimal | ReasoningEffort::Low => Some("low"),
        ReasoningEffort::Medium => Some("medium"),
        ReasoningEffort::High => Some("high"),
        ReasoningEffort::XHigh => Some("max"),
    }
}

fn parse_claude_json_output(stdout: &str, stderr: &str) -> ClaudeParsedOutput {
    let parsed_json = serde_json::from_str::<JsonValue>(stdout).ok();
    let message = parsed_json
        .as_ref()
        .and_then(find_claude_message)
        .or_else(|| non_empty_trimmed(stdout).map(ToOwned::to_owned));
    let token_usage = parsed_json.as_ref().and_then(find_claude_token_usage);
    let session_id = parsed_json.as_ref().and_then(find_claude_session_id);

    ClaudeParsedOutput {
        message,
        token_usage,
        session_id,
        stderr: stderr.to_string(),
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
            None,
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
            SpawnedAgentBackendKind::parse(None).expect("backend"),
            SpawnedAgentBackendKind::Codex
        );
    }
}
