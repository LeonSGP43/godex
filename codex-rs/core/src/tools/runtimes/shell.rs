/*
Runtime: shell

Executes shell requests under the orchestrator: asks for approval when needed,
builds sandbox transform inputs, and runs them under the current SandboxAttempt.
*/
#[cfg(unix)]
pub(crate) mod unix_escalation;
pub(crate) mod zsh_fork_backend;

use crate::command_canonicalization::canonicalize_command_for_approval;
use crate::exec::ExecCapturePolicy;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::review_approval_request;
use crate::sandboxing::ExecOptions;
use crate::sandboxing::SandboxPermissions;
use crate::sandboxing::execute_env;
use crate::shell::ShellType;
use crate::tools::network_approval::NetworkApprovalMode;
use crate::tools::network_approval::NetworkApprovalSpec;
use crate::tools::runtimes::build_sandbox_command;
use crate::tools::runtimes::maybe_wrap_shell_lc_with_snapshot;
use crate::tools::sandboxing::Approvable;
use crate::tools::sandboxing::ApprovalCtx;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::SandboxAttempt;
use crate::tools::sandboxing::SandboxOverride;
use crate::tools::sandboxing::Sandboxable;
use crate::tools::sandboxing::ToolCtx;
use crate::tools::sandboxing::ToolError;
use crate::tools::sandboxing::ToolRuntime;
use crate::tools::sandboxing::sandbox_override_for_first_attempt;
use codex_network_proxy::NetworkProxy;
use codex_protocol::approvals::ExecPolicyAmendment;
use codex_protocol::exec_output::ExecToolCallOutput;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::ReviewDecision;
use codex_sandboxing::SandboxablePreference;
use codex_shell_command::powershell::prefix_powershell_script_with_utf8;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ShellRequest {
    pub command: Vec<String>,
    pub cwd: AbsolutePathBuf,
    pub timeout_ms: Option<u64>,
    pub env: HashMap<String, String>,
    pub explicit_env_overrides: HashMap<String, String>,
    pub network: Option<NetworkProxy>,
    pub sandbox_permissions: SandboxPermissions,
    pub additional_permissions: Option<PermissionProfile>,
    #[cfg(unix)]
    pub additional_permissions_preapproved: bool,
    pub justification: Option<String>,
    pub exec_approval_requirement: ExecApprovalRequirement,
}

/// Selects `ShellRuntime` behavior for different callers.
///
/// Note: `Generic` is not the same as `ShellCommandClassic`.
/// `Generic` means "no `shell_command`-specific backend behavior" (used by the
/// generic `shell` tool path). The `ShellCommand*` variants are only for the
/// `shell_command` tool family.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ShellRuntimeBackend {
    /// Tool-agnostic/default runtime path.
    ///
    /// Uses the normal `ShellRuntime` execution flow without enabling any
    /// `shell_command`-specific backend selection.
    #[default]
    Generic,
    /// Legacy backend for the `shell_command` tool.
    ///
    /// Keeps `shell_command` on the standard shell runtime flow without the
    /// zsh-fork shell-escalation adapter.
    ShellCommandClassic,
    /// zsh-fork backend for the `shell_command` tool.
    ///
    /// On Unix, attempts to run via the zsh-fork + `codex-shell-escalation`
    /// adapter, with fallback to the standard shell runtime flow if
    /// prerequisites are not met.
    ShellCommandZshFork,
}

#[derive(Default)]
pub struct ShellRuntime {
    backend: ShellRuntimeBackend,
}

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ApprovalKey {
    command: Vec<String>,
    cwd: AbsolutePathBuf,
    sandbox_permissions: SandboxPermissions,
    additional_permissions: Option<PermissionProfile>,
}

impl ShellRuntime {
    pub fn new() -> Self {
        Self {
            backend: ShellRuntimeBackend::Generic,
        }
    }

    pub(crate) fn for_shell_command(backend: ShellRuntimeBackend) -> Self {
        Self { backend }
    }

    fn stdout_stream(ctx: &ToolCtx) -> Option<crate::exec::StdoutStream> {
        Some(crate::exec::StdoutStream {
            sub_id: ctx.turn.sub_id.clone(),
            call_id: ctx.call_id.clone(),
            tx_event: ctx.session.get_tx_event(),
        })
    }

    async fn review_request_owned(
        session: Arc<crate::codex::Session>,
        turn: Arc<crate::codex::TurnContext>,
        review_id: String,
        request: GuardianApprovalRequest,
        retry_reason: Option<String>,
    ) -> ReviewDecision {
        review_approval_request(session, turn, review_id, request, retry_reason).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request_command_approval_owned(
        session: Arc<crate::codex::Session>,
        turn: Arc<crate::codex::TurnContext>,
        call_id: String,
        command: Vec<String>,
        cwd: std::path::PathBuf,
        reason: Option<String>,
        network_approval_context: Option<codex_protocol::approvals::NetworkApprovalContext>,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
        additional_permissions: Option<PermissionProfile>,
        available_decisions: Option<Vec<ReviewDecision>>,
    ) -> ReviewDecision {
        session
            .request_command_approval_owned(
                turn,
                call_id,
                /*approval_id*/ None,
                command,
                cwd,
                reason,
                network_approval_context,
                proposed_execpolicy_amendment,
                additional_permissions,
                available_decisions,
            )
            .await
    }

    async fn already_approved_owned(
        session: Arc<crate::codex::Session>,
        keys: Vec<ApprovalKey>,
    ) -> bool {
        let store = session.services.tool_approvals.lock().await;
        keys.iter()
            .all(|key| matches!(store.get(key), Some(ReviewDecision::ApprovedForSession)))
    }

    async fn remember_approval_keys_owned(
        session: Arc<crate::codex::Session>,
        keys: Vec<ApprovalKey>,
    ) {
        let mut store = session.services.tool_approvals.lock().await;
        for key in keys {
            store.put(key, ReviewDecision::ApprovedForSession);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_approval_owned(
        session: Arc<crate::codex::Session>,
        turn: Arc<crate::codex::TurnContext>,
        keys: Vec<ApprovalKey>,
        call_id: String,
        command: Vec<String>,
        cwd: std::path::PathBuf,
        retry_reason: Option<String>,
        reason: Option<String>,
        guardian_review_id: Option<String>,
        network_approval_context: Option<codex_protocol::approvals::NetworkApprovalContext>,
        sandbox_permissions: SandboxPermissions,
        additional_permissions: Option<PermissionProfile>,
        justification: Option<String>,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
    ) -> ReviewDecision {
        if let Some(review_id) = guardian_review_id {
            return ShellRuntime::review_request_owned(
                session,
                turn,
                review_id,
                GuardianApprovalRequest::Shell {
                    id: call_id,
                    command,
                    cwd,
                    sandbox_permissions,
                    additional_permissions: additional_permissions.clone(),
                    justification,
                },
                retry_reason,
            )
            .await;
        }
        let already_approved =
            ShellRuntime::already_approved_owned(session.clone(), keys.clone()).await;
        if already_approved {
            return ReviewDecision::ApprovedForSession;
        }

        let available_decisions = None;
        let decision = ShellRuntime::request_command_approval_owned(
            session.clone(),
            turn,
            call_id,
            command,
            cwd,
            reason,
            network_approval_context,
            proposed_execpolicy_amendment,
            additional_permissions,
            available_decisions,
        )
        .await;
        session.services.session_telemetry.counter(
            "codex.approval.requested",
            /*inc*/ 1,
            &[("tool", "shell"), ("approved", decision.to_opaque_string())],
        );
        if matches!(decision, ReviewDecision::ApprovedForSession) {
            ShellRuntime::remember_approval_keys_owned(session, keys).await;
        }
        decision
    }
}

impl Sandboxable for ShellRuntime {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }
    fn escalate_on_failure(&self) -> bool {
        true
    }
}

impl Approvable<ShellRequest> for ShellRuntime {
    type ApprovalKey = ApprovalKey;

    fn approval_keys(&self, req: &ShellRequest) -> Vec<Self::ApprovalKey> {
        vec![ApprovalKey {
            command: canonicalize_command_for_approval(&req.command),
            cwd: req.cwd.clone(),
            sandbox_permissions: req.sandbox_permissions,
            additional_permissions: req.additional_permissions.clone(),
        }]
    }

    fn start_approval_async<'a>(
        &'a mut self,
        req: &'a ShellRequest,
        ctx: ApprovalCtx<'a>,
    ) -> BoxFuture<'a, ReviewDecision> {
        let keys = self.approval_keys(req);
        let command = req.command.clone();
        let cwd = req.cwd.to_path_buf();
        let retry_reason = ctx.retry_reason.clone();
        let reason = retry_reason.clone().or_else(|| req.justification.clone());
        let session = Arc::clone(ctx.session);
        let turn = Arc::clone(ctx.turn);
        let call_id = ctx.call_id.to_string();
        let guardian_review_id = ctx.guardian_review_id.clone();
        let network_approval_context = ctx.network_approval_context.clone();
        let sandbox_permissions = req.sandbox_permissions;
        let additional_permissions = req.additional_permissions.clone();
        let justification = req.justification.clone();
        let proposed_execpolicy_amendment = req
            .exec_approval_requirement
            .proposed_execpolicy_amendment()
            .cloned();
        Box::pin(async move {
            ShellRuntime::start_approval_owned(
                session,
                turn,
                keys,
                call_id,
                command,
                cwd,
                retry_reason,
                reason,
                guardian_review_id,
                network_approval_context,
                sandbox_permissions,
                additional_permissions,
                justification,
                proposed_execpolicy_amendment,
            )
            .await
        })
    }

    fn exec_approval_requirement(&self, req: &ShellRequest) -> Option<ExecApprovalRequirement> {
        Some(req.exec_approval_requirement.clone())
    }

    fn sandbox_mode_for_first_attempt(&self, req: &ShellRequest) -> SandboxOverride {
        sandbox_override_for_first_attempt(req.sandbox_permissions, &req.exec_approval_requirement)
    }
}

impl ToolRuntime<ShellRequest, ExecToolCallOutput> for ShellRuntime {
    fn network_approval_spec(
        &self,
        req: &ShellRequest,
        _ctx: &ToolCtx,
    ) -> Option<NetworkApprovalSpec> {
        req.network.as_ref()?;
        Some(NetworkApprovalSpec {
            network: req.network.clone(),
            mode: NetworkApprovalMode::Immediate,
        })
    }

    async fn run(
        &mut self,
        req: &ShellRequest,
        attempt: &SandboxAttempt<'_>,
        ctx: &ToolCtx,
    ) -> Result<ExecToolCallOutput, ToolError> {
        let session_shell = ctx.session.user_shell();
        let command = maybe_wrap_shell_lc_with_snapshot(
            &req.command,
            session_shell.as_ref(),
            &req.cwd,
            &req.explicit_env_overrides,
            &req.env,
        );
        let command = if matches!(session_shell.shell_type, ShellType::PowerShell) {
            prefix_powershell_script_with_utf8(&command)
        } else {
            command
        };

        if self.backend == ShellRuntimeBackend::ShellCommandZshFork {
            match zsh_fork_backend::maybe_run_shell_command(req, attempt, ctx, &command).await? {
                Some(out) => return Ok(out),
                None => {
                    tracing::warn!(
                        "ZshFork backend specified, but conditions for using it were not met, falling back to normal execution",
                    );
                }
            }
        }

        let command = build_sandbox_command(
            &command,
            &req.cwd,
            &req.env,
            req.additional_permissions.clone(),
        )?;
        let options = ExecOptions {
            expiration: req.timeout_ms.into(),
            capture_policy: ExecCapturePolicy::ShellTool,
        };
        let env = attempt
            .env_for(command, options, req.network.as_ref())
            .map_err(|err| ToolError::Codex(err.into()))?;
        let out = execute_env(env, Self::stdout_stream(ctx))
            .await
            .map_err(ToolError::Codex)?;
        Ok(out)
    }
}
