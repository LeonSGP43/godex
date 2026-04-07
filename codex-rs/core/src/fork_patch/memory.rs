use crate::config::types::MemoriesConfig;
use crate::memories;
use std::path::Path;

pub(crate) fn resolve_memory_scope(
    cwd: &Path,
    project_root_markers: &[String],
    memories: &MemoriesConfig,
) -> memories::MemoryScope {
    memories::resolve_memory_scope(cwd, project_root_markers, memories)
}

pub(crate) async fn build_memory_context_fragment(
    codex_home: &Path,
    memories: &MemoriesConfig,
    memory_scope_kind: &str,
    memory_scope_key: &str,
    turn_query: Option<&str>,
) -> Option<String> {
    crate::memories::prompts::build_memory_tool_developer_instructions(
        codex_home,
        memories,
        memory_scope_kind,
        memory_scope_key,
        turn_query,
    )
    .await
}
