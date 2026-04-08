use crate::config::types::MemoriesConfig;
use crate::memories;
use crate::memories::SemanticRecallMatch;
use crate::memories::SemanticRecallOptions;
use crate::memories::semantic_recall;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use codex_utils_template::Template;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use tokio::fs;

const ROLLOUT_SUMMARIES_SUBDIR: &str = "rollout_summaries";
const RAW_MEMORIES_FILENAME: &str = "raw_memories.md";
const MEMORY_INDEX_FILENAME: &str = "MEMORY.md";
const MEMORY_SUMMARY_FILENAME: &str = "memory_summary.md";
const MEMORY_QMD_FILENAME: &str = "memory_index.qmd";
const VECTOR_INDEX_FILENAME: &str = "vector_index.json";
const SKILLS_SUBDIR: &str = "skills";
const MEMORY_TOOL_QUERY_PREVIEW_CHAR_LIMIT: usize = 240;

static MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../../templates/memories/read_path.md"),
        "memories/read_path.md",
    )
});

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

pub(crate) fn empty_consolidation_cleanup_files(root: &Path) -> [PathBuf; 2] {
    [memory_index_file(root), memory_summary_file(root)]
}

pub(crate) fn empty_consolidation_cleanup_dirs(root: &Path) -> [PathBuf; 1] {
    [skills_dir(root)]
}

pub(crate) async fn build_memory_context_fragment(
    codex_home: &Path,
    memories: &MemoriesConfig,
    memory_scope_kind: &str,
    memory_scope_key: &str,
    turn_query: Option<&str>,
) -> Option<String> {
    let base_path = scoped_artifact_root(codex_home, memory_scope_kind, memory_scope_key);
    let memory_summary_path = memory_summary_file(&base_path);
    let memory_summary = fs::read_to_string(&memory_summary_path)
        .await
        .ok()?
        .trim()
        .to_string();
    let memory_summary = truncate_text(
        &memory_summary,
        TruncationPolicy::Tokens(memories.summary_token_limit),
    );
    if memory_summary.is_empty() {
        return None;
    }

    let base_path_display = base_path.display().to_string();
    let mut rendered = MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_TEMPLATE
        .render([
            ("base_path", base_path_display.as_str()),
            ("memory_summary", memory_summary.as_str()),
        ])
        .ok()?;

    append_semantic_recall_hints_if_enabled(&mut rendered, memories, &base_path, turn_query).await;

    Some(rendered)
}

fn parse_embedded_template(source: &'static str, template_name: &str) -> Template {
    match Template::parse(source) {
        Ok(template) => template,
        Err(err) => panic!("embedded template {template_name} is invalid: {err}"),
    }
}

async fn append_semantic_recall_hints_if_enabled(
    buffer: &mut String,
    memories: &MemoriesConfig,
    base_path: &Path,
    turn_query: Option<&str>,
) {
    if !memories.semantic_index_enabled {
        return;
    }

    let Some(normalized_query) = normalize_turn_query(turn_query) else {
        return;
    };

    let matches: Vec<SemanticRecallMatch> = match semantic_recall(
        base_path,
        &normalized_query,
        SemanticRecallOptions {
            limit: memories.semantic_recall_limit,
            hybrid_enabled: memories.qmd_hybrid_enabled,
            query_expansion_enabled: memories.qmd_query_expansion_enabled,
            rerank_limit: memories.qmd_rerank_limit,
        },
    )
    .await
    {
        Ok(matches) => matches,
        Err(_) => return,
    };

    if matches.is_empty() {
        return;
    }

    append_semantic_recall_hints(buffer, &normalized_query, &matches);
}

fn normalize_turn_query(turn_query: Option<&str>) -> Option<String> {
    let query = turn_query?.trim();
    if query.is_empty() {
        return None;
    }

    Some(truncate_query_preview(
        &query.split_whitespace().collect::<Vec<_>>().join(" "),
    ))
}

fn truncate_query_preview(query: &str) -> String {
    if query.chars().count() <= MEMORY_TOOL_QUERY_PREVIEW_CHAR_LIMIT {
        return query.to_string();
    }

    let truncated = query
        .chars()
        .take(MEMORY_TOOL_QUERY_PREVIEW_CHAR_LIMIT)
        .collect::<String>();
    format!("{truncated}...")
}

fn append_semantic_recall_hints(
    buffer: &mut String,
    normalized_query: &str,
    matches: &[SemanticRecallMatch],
) {
    buffer.push_str("\n\n## Semantic Recall Hints\n");
    buffer.push_str(
        "Use this shortlist as a semantic fallback before broad scans when MEMORY.md keyword hits are weak.\n",
    );
    buffer.push_str(&format!("- query: {normalized_query}\n"));

    for (idx, item) in matches.iter().enumerate() {
        let keywords = if item.keywords.is_empty() {
            "n/a".to_string()
        } else {
            item.keywords.join(", ")
        };
        let signals = if item.signals.is_empty() {
            "vector".to_string()
        } else {
            item.signals.join("+")
        };
        let summary_preview = if item.summary_preview.trim().is_empty() {
            "(empty summary)".to_string()
        } else {
            item.summary_preview.replace('\n', " ")
        };
        buffer.push_str(&format!(
            "- [{}] score={:.3}, signals={}, thread_id={}, file={}, keywords={}\n",
            idx + 1,
            item.score,
            signals,
            item.thread_id,
            item.rollout_summary_file,
            keywords
        ));
        buffer.push_str(&format!("  summary: {summary_preview}\n"));
    }
}
