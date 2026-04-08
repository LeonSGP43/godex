use crate::memories::phase_one;
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
use tracing::warn;

#[cfg(test)]
use crate::memories::GLOBAL_MEMORY_SCOPE_KEY;
#[cfg(test)]
use crate::memories::GLOBAL_MEMORY_SCOPE_KIND;

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
    let rollout_summary_file = crate::fork_patch::memory::rollout_summary_relative_path(
        &rollout_summary_file_stem_from_parts(
            item.thread_id,
            item.source_updated_at,
            item.rollout_slug.as_deref(),
        ),
    );
    format!(
        "- [{status}] thread_id={}, rollout_summary_file={rollout_summary_file}",
        item.thread_id
    )
}

fn render_removed_input_line(item: &Stage1OutputRef) -> String {
    let rollout_summary_file = crate::fork_patch::memory::rollout_summary_relative_path(
        &rollout_summary_file_stem_from_parts(
            item.thread_id,
            item.source_updated_at,
            item.rollout_slug.as_deref(),
        ),
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

#[cfg(test)]
#[path = "prompts_tests.rs"]
mod tests;
