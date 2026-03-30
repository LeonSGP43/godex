use crate::config::types::MemoriesConfig;
use crate::memories::memory_root;
use crate::memories::phase_one;
use crate::memories::semantic_index::SemanticRecallMatch;
use crate::memories::semantic_index::semantic_recall;
use crate::memories::storage::rollout_summary_file_stem_from_parts;
use codex_protocol::openai_models::ModelInfo;
use codex_state::Phase2InputSelection;
use codex_state::Stage1Output;
use codex_state::Stage1OutputRef;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use codex_utils_template::Template;
use std::path::Path;
use std::sync::LazyLock;
use tokio::fs;
use tracing::warn;

static CONSOLIDATION_PROMPT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../../templates/memories/consolidation.md"),
        "memories/consolidation.md",
    )
});
static STAGE_ONE_INPUT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../../templates/memories/stage_one_input.md"),
        "memories/stage_one_input.md",
    )
});
static MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../../templates/memories/read_path.md"),
        "memories/read_path.md",
    )
});
const MEMORY_TOOL_QUERY_PREVIEW_CHAR_LIMIT: usize = 240;

fn parse_embedded_template(source: &'static str, template_name: &str) -> Template {
    match Template::parse(source) {
        Ok(template) => template,
        Err(err) => panic!("embedded template {template_name} is invalid: {err}"),
    }
}

/// Builds the consolidation subagent prompt for a specific memory root.
pub(super) fn build_consolidation_prompt(
    memory_root: &Path,
    selection: &Phase2InputSelection,
) -> String {
    let memory_root = memory_root.display().to_string();
    let phase2_input_selection = render_phase2_input_selection(selection);
    CONSOLIDATION_PROMPT_TEMPLATE
        .render([
            ("memory_root", memory_root.as_str()),
            ("phase2_input_selection", phase2_input_selection.as_str()),
        ])
        .unwrap_or_else(|err| {
        warn!("failed to render memories consolidation prompt template: {err}");
        format!(
            "## Memory Phase 2 (Consolidation)\nConsolidate Codex memories in: {memory_root}\n\n{phase2_input_selection}"
        )
    })
}

fn render_phase2_input_selection(selection: &Phase2InputSelection) -> String {
    let retained = selection.retained_thread_ids.len();
    let added = selection.selected.len().saturating_sub(retained);
    let selected = if selection.selected.is_empty() {
        "- none".to_string()
    } else {
        selection
            .selected
            .iter()
            .map(|item| {
                render_selected_input_line(
                    item,
                    selection.retained_thread_ids.contains(&item.thread_id),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let removed = if selection.removed.is_empty() {
        "- none".to_string()
    } else {
        selection
            .removed
            .iter()
            .map(render_removed_input_line)
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "- selected inputs this run: {}\n- newly added since the last successful Phase 2 run: {added}\n- retained from the last successful Phase 2 run: {retained}\n- removed from the last successful Phase 2 run: {}\n\nCurrent selected Phase 1 inputs:\n{selected}\n\nRemoved from the last successful Phase 2 selection:\n{removed}\n",
        selection.selected.len(),
        selection.removed.len(),
    )
}

fn render_selected_input_line(item: &Stage1Output, retained: bool) -> String {
    let status = if retained { "retained" } else { "added" };
    let rollout_summary_file = format!(
        "rollout_summaries/{}.md",
        rollout_summary_file_stem_from_parts(
            item.thread_id,
            item.source_updated_at,
            item.rollout_slug.as_deref(),
        )
    );
    format!(
        "- [{status}] thread_id={}, rollout_summary_file={rollout_summary_file}",
        item.thread_id
    )
}

fn render_removed_input_line(item: &Stage1OutputRef) -> String {
    let rollout_summary_file = format!(
        "rollout_summaries/{}.md",
        rollout_summary_file_stem_from_parts(
            item.thread_id,
            item.source_updated_at,
            item.rollout_slug.as_deref(),
        )
    );
    format!(
        "- thread_id={}, rollout_summary_file={rollout_summary_file}",
        item.thread_id
    )
}

/// Builds the stage-1 user message containing rollout metadata and content.
///
/// Large rollout payloads are truncated to 70% of the active model's effective
/// input window token budget while keeping both head and tail context.
pub(super) fn build_stage_one_input_message(
    model_info: &ModelInfo,
    rollout_path: &Path,
    rollout_cwd: &Path,
    rollout_contents: &str,
) -> anyhow::Result<String> {
    let rollout_token_limit = model_info
        .context_window
        .and_then(|limit| (limit > 0).then_some(limit))
        .map(|limit| limit.saturating_mul(model_info.effective_context_window_percent) / 100)
        .map(|limit| (limit.saturating_mul(phase_one::CONTEXT_WINDOW_PERCENT) / 100).max(1))
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(phase_one::DEFAULT_STAGE_ONE_ROLLOUT_TOKEN_LIMIT);
    let truncated_rollout_contents = truncate_text(
        rollout_contents,
        TruncationPolicy::Tokens(rollout_token_limit),
    );

    let rollout_path = rollout_path.display().to_string();
    let rollout_cwd = rollout_cwd.display().to_string();
    Ok(STAGE_ONE_INPUT_TEMPLATE.render([
        ("rollout_path", rollout_path.as_str()),
        ("rollout_cwd", rollout_cwd.as_str()),
        ("rollout_contents", truncated_rollout_contents.as_str()),
    ])?)
}

/// Build prompt used for read path. This prompt must be added to the developer instructions. In
/// case of large memory files, the `memory_summary.md` is truncated at
/// [phase_one::MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_SUMMARY_TOKEN_LIMIT].
pub(crate) async fn build_memory_tool_developer_instructions(
    codex_home: &Path,
    memories: &MemoriesConfig,
    turn_query: Option<&str>,
) -> Option<String> {
    let base_path = memory_root(codex_home);
    let memory_summary_path = base_path.join("memory_summary.md");
    let memory_summary = fs::read_to_string(&memory_summary_path)
        .await
        .ok()?
        .trim()
        .to_string();
    let memory_summary = truncate_text(
        &memory_summary,
        TruncationPolicy::Tokens(phase_one::MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_SUMMARY_TOKEN_LIMIT),
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

    if memories.semantic_index_enabled
        && let Some(normalized_query) = normalize_turn_query(turn_query)
        && let Ok(matches) = semantic_recall(
            &base_path,
            &normalized_query,
            memories.semantic_recall_limit,
        )
        .await
        && !matches.is_empty()
    {
        append_semantic_recall_hints(&mut rendered, &normalized_query, &matches);
    }

    Some(rendered)
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
        let summary_preview = if item.summary_preview.trim().is_empty() {
            "(empty summary)".to_string()
        } else {
            item.summary_preview.replace('\n', " ")
        };
        buffer.push_str(&format!(
            "- [{}] score={:.3}, thread_id={}, file={}, keywords={}\n",
            idx + 1,
            item.score,
            item.thread_id,
            item.rollout_summary_file,
            keywords
        ));
        buffer.push_str(&format!("  summary: {summary_preview}\n"));
    }
}

#[cfg(test)]
#[path = "prompts_tests.rs"]
mod tests;
