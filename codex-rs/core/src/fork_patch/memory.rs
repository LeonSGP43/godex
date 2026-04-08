use crate::config::types::MemoriesConfig;
use crate::memories;
use std::path::Path;
use std::path::PathBuf;

const ROLLOUT_SUMMARIES_SUBDIR: &str = "rollout_summaries";
const RAW_MEMORIES_FILENAME: &str = "raw_memories.md";
const MEMORY_INDEX_FILENAME: &str = "MEMORY.md";
const MEMORY_SUMMARY_FILENAME: &str = "memory_summary.md";
const MEMORY_QMD_FILENAME: &str = "memory_index.qmd";
const VECTOR_INDEX_FILENAME: &str = "vector_index.json";
const SKILLS_SUBDIR: &str = "skills";

pub(crate) fn resolve_memory_scope(
    cwd: &Path,
    project_root_markers: &[String],
    memories: &MemoriesConfig,
) -> memories::MemoryScope {
    memories::resolve_memory_scope(cwd, project_root_markers, memories)
}

pub(crate) fn scoped_artifact_root(
    codex_home: &Path,
    memory_scope_kind: &str,
    memory_scope_key: &str,
) -> PathBuf {
    let memory_scope = memories::MemoryScope {
        kind: memory_scope_kind.to_string(),
        key: memory_scope_key.to_string(),
    };
    memories::scoped_memory_root(codex_home, &memory_scope)
}

pub(crate) fn rollout_summaries_dir(root: &Path) -> PathBuf {
    root.join(ROLLOUT_SUMMARIES_SUBDIR)
}

pub(crate) fn rollout_summary_path(root: &Path, file_stem: &str) -> PathBuf {
    rollout_summaries_dir(root).join(format!("{file_stem}.md"))
}

pub(crate) fn raw_memories_file(root: &Path) -> PathBuf {
    root.join(RAW_MEMORIES_FILENAME)
}

pub(crate) fn memory_index_file(root: &Path) -> PathBuf {
    root.join(MEMORY_INDEX_FILENAME)
}

pub(crate) fn memory_summary_file(root: &Path) -> PathBuf {
    root.join(MEMORY_SUMMARY_FILENAME)
}

pub(crate) fn memory_qmd_file(root: &Path) -> PathBuf {
    root.join(MEMORY_QMD_FILENAME)
}

pub(crate) fn vector_index_file(root: &Path) -> PathBuf {
    root.join(VECTOR_INDEX_FILENAME)
}

pub(crate) fn skills_dir(root: &Path) -> PathBuf {
    root.join(SKILLS_SUBDIR)
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
