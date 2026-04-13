use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::unified_exec::ExecCommandArgs;
use codex_protocol::models::ShellCommandToolCallParams;
use codex_protocol::models::ShellToolCallParams;
use codex_protocol::parse_command::ParsedCommand;
use codex_shell_command::is_safe_command::is_known_safe_command;
use codex_shell_command::parse_command::parse_command;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

const MEMORIES_USAGE_METRIC: &str = "codex.memories.usage";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MemoriesUsageKind {
    MemoryMd,
    MemorySummary,
    RawMemories,
    RolloutSummaries,
    Skills,
}

impl MemoriesUsageKind {
    fn as_tag(self) -> &'static str {
        match self {
            Self::MemoryMd => "memory_md",
            Self::MemorySummary => "memory_summary",
            Self::RawMemories => "raw_memories",
            Self::RolloutSummaries => "rollout_summaries",
            Self::Skills => "skills",
        }
    }
}

pub(crate) async fn emit_metric_for_tool_read(invocation: &ToolInvocation, success: bool) {
    let kinds = memories_usage_kinds_from_invocation(invocation).await;
    if kinds.is_empty() {
        return;
    }

    let success = if success { "true" } else { "false" };
    for kind in kinds {
        invocation.turn.session_telemetry.counter(
            MEMORIES_USAGE_METRIC,
            /*inc*/ 1,
            &[
                ("kind", kind.as_tag()),
                ("tool", invocation.tool_name.as_str()),
                ("success", success),
            ],
        );
    }
}

async fn memories_usage_kinds_from_invocation(
    invocation: &ToolInvocation,
) -> Vec<MemoriesUsageKind> {
    let Some((command, _)) = shell_command_for_invocation(invocation) else {
        return Vec::new();
    };
    if !is_known_safe_command(&command) {
        return Vec::new();
    }

    let parsed_commands = parse_command(&command);
    parsed_commands
        .into_iter()
        .filter_map(|command| match command {
            ParsedCommand::Read { path, .. } => get_memory_kind(path.display().to_string()),
            ParsedCommand::Search { path, .. } => path.and_then(get_memory_kind),
            ParsedCommand::ListFiles { .. } | ParsedCommand::Unknown { .. } => None,
        })
        .collect()
}

fn shell_command_for_invocation(invocation: &ToolInvocation) -> Option<(Vec<String>, PathBuf)> {
    let ToolPayload::Function { arguments } = &invocation.payload else {
        return None;
    };

    match invocation.tool_name.as_str() {
        "shell" => serde_json::from_str::<ShellToolCallParams>(arguments)
            .ok()
            .map(|params| {
                (
                    params.command,
                    invocation.turn.resolve_path(params.workdir).to_path_buf(),
                )
            }),
        "shell_command" => serde_json::from_str::<ShellCommandToolCallParams>(arguments)
            .ok()
            .map(|params| {
                if !invocation.turn.tools_config.allow_login_shell && params.login == Some(true) {
                    return (
                        Vec::new(),
                        invocation.turn.resolve_path(params.workdir).to_path_buf(),
                    );
                }
                let use_login_shell = params
                    .login
                    .unwrap_or(invocation.turn.tools_config.allow_login_shell);
                let command = invocation
                    .session
                    .user_shell()
                    .derive_exec_args(&params.command, use_login_shell);
                (
                    command,
                    invocation.turn.resolve_path(params.workdir).to_path_buf(),
                )
            }),
        "exec_command" => serde_json::from_str::<ExecCommandArgs>(arguments)
            .ok()
            .and_then(|params| {
                let command = crate::tools::handlers::unified_exec::get_command(
                    &params,
                    invocation.session.user_shell(),
                    &invocation.turn.tools_config.unified_exec_shell_mode,
                    invocation.turn.tools_config.allow_login_shell,
                )
                .ok()?;
                Some((
                    command,
                    invocation.turn.resolve_path(params.workdir).to_path_buf(),
                ))
            }),
        _ => None,
    }
}

fn get_memory_kind(path: String) -> Option<MemoriesUsageKind> {
    let components = Path::new(path.as_str())
        .components()
        .filter_map(|component| match component {
            Component::Normal(component) => component.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();

    let memories_idx = components
        .iter()
        .position(|component| *component == "memories")?;
    let artifact = match components.get(memories_idx + 1).copied()? {
        "scopes" => components.get(memories_idx + 4).copied()?,
        artifact => artifact,
    };

    match artifact {
        "MEMORY.md" => Some(MemoriesUsageKind::MemoryMd),
        "memory_summary.md" => Some(MemoriesUsageKind::MemorySummary),
        "raw_memories.md" => Some(MemoriesUsageKind::RawMemories),
        "rollout_summaries" => Some(MemoriesUsageKind::RolloutSummaries),
        "skills" => Some(MemoriesUsageKind::Skills),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::MemoriesUsageKind;
    use super::get_memory_kind;

    #[test]
    fn classifies_global_memory_artifacts() {
        assert_eq!(
            get_memory_kind("/tmp/.codex/memories/MEMORY.md".to_string()),
            Some(MemoriesUsageKind::MemoryMd)
        );
        assert_eq!(
            get_memory_kind("/tmp/.codex/memories/memory_summary.md".to_string()),
            Some(MemoriesUsageKind::MemorySummary)
        );
        assert_eq!(
            get_memory_kind("/tmp/.codex/memories/raw_memories.md".to_string()),
            Some(MemoriesUsageKind::RawMemories)
        );
        assert_eq!(
            get_memory_kind("/tmp/.codex/memories/rollout_summaries/one.md".to_string()),
            Some(MemoriesUsageKind::RolloutSummaries)
        );
        assert_eq!(
            get_memory_kind("/tmp/.codex/memories/skills/foo/SKILL.md".to_string()),
            Some(MemoriesUsageKind::Skills)
        );
    }

    #[test]
    fn classifies_scoped_memory_artifacts() {
        let scoped_root = "/tmp/.codex/memories/scopes/project/codex-deadbeef";
        assert_eq!(
            get_memory_kind(format!("{scoped_root}/MEMORY.md")),
            Some(MemoriesUsageKind::MemoryMd)
        );
        assert_eq!(
            get_memory_kind(format!("{scoped_root}/memory_summary.md")),
            Some(MemoriesUsageKind::MemorySummary)
        );
        assert_eq!(
            get_memory_kind(format!("{scoped_root}/raw_memories.md")),
            Some(MemoriesUsageKind::RawMemories)
        );
        assert_eq!(
            get_memory_kind(format!("{scoped_root}/rollout_summaries/one.md")),
            Some(MemoriesUsageKind::RolloutSummaries)
        );
        assert_eq!(
            get_memory_kind(format!("{scoped_root}/skills/foo/SKILL.md")),
            Some(MemoriesUsageKind::Skills)
        );
    }
}
