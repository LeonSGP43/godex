use super::*;
use crate::config::types::MemoriesConfig;
use crate::models_manager::model_info::model_info_from_slug;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::fs as tokio_fs;

#[test]
fn build_stage_one_input_message_truncates_rollout_using_model_context_window() {
    let input = format!("{}{}{}", "a".repeat(700_000), "middle", "z".repeat(700_000));
    let mut model_info = model_info_from_slug("gpt-5.2-codex");
    model_info.context_window = Some(123_000);
    let expected_rollout_token_limit = usize::try_from(
        ((123_000_i64 * model_info.effective_context_window_percent) / 100)
            * phase_one::CONTEXT_WINDOW_PERCENT
            / 100,
    )
    .unwrap();
    let expected_truncated = truncate_text(
        &input,
        TruncationPolicy::Tokens(expected_rollout_token_limit),
    );
    let message = build_stage_one_input_message(
        &model_info,
        Path::new("/tmp/rollout.jsonl"),
        Path::new("/tmp"),
        &input,
    )
    .unwrap();

    assert!(expected_truncated.contains("tokens truncated"));
    assert!(expected_truncated.starts_with('a'));
    assert!(expected_truncated.ends_with('z'));
    assert!(message.contains(&expected_truncated));
}

#[test]
fn build_stage_one_input_message_uses_default_limit_when_model_context_window_missing() {
    let input = format!("{}{}{}", "a".repeat(700_000), "middle", "z".repeat(700_000));
    let mut model_info = model_info_from_slug("gpt-5.2-codex");
    model_info.context_window = None;
    let expected_truncated = truncate_text(
        &input,
        TruncationPolicy::Tokens(phase_one::DEFAULT_STAGE_ONE_ROLLOUT_TOKEN_LIMIT),
    );
    let message = build_stage_one_input_message(
        &model_info,
        Path::new("/tmp/rollout.jsonl"),
        Path::new("/tmp"),
        &input,
    )
    .unwrap();

    assert!(message.contains(&expected_truncated));
}

#[test]
fn build_consolidation_prompt_renders_embedded_template() {
    let prompt =
        build_consolidation_prompt(Path::new("/tmp/memories"), &Phase2InputSelection::default());

    assert!(prompt.contains("Folder structure (under /tmp/memories/):"));
    assert!(prompt.contains("memory_index.qmd"));
    assert!(prompt.contains("vector_index.json"));
    assert!(prompt.contains("**Diff since last consolidation:**"));
    assert!(prompt.contains("- selected inputs this run: 0"));
}

#[tokio::test]
async fn build_memory_tool_developer_instructions_renders_embedded_template() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path();
    let memories_dir = codex_home.join("memories");
    tokio_fs::create_dir_all(&memories_dir).await.unwrap();
    tokio_fs::write(
        memories_dir.join("memory_summary.md"),
        "Short memory summary for tests.",
    )
    .await
    .unwrap();

    let instructions = build_memory_tool_developer_instructions(
        codex_home,
        &MemoriesConfig::default(),
        /*turn_query*/ None,
    )
    .await
    .unwrap();

    assert!(instructions.contains(&format!(
        "- {}/memory_summary.md (already provided below; do NOT open again)",
        memories_dir.display()
    )));
    assert!(instructions.contains("Short memory summary for tests."));
    assert_eq!(
        instructions
            .matches("========= MEMORY_SUMMARY BEGINS =========")
            .count(),
        1
    );
}

#[tokio::test]
async fn build_memory_tool_developer_instructions_appends_semantic_recall_hints() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path();
    let memories_dir = codex_home.join("memories");
    tokio_fs::create_dir_all(&memories_dir).await.unwrap();
    tokio_fs::write(
        memories_dir.join("memory_summary.md"),
        "Short memory summary for tests.",
    )
    .await
    .unwrap();

    let vector_index = serde_json::json!({
        "version": 1,
        "generated_at": "2026-01-01T00:00:00Z",
        "dimension": 256,
        "metric": "cosine",
        "entries": [
            {
                "thread_id": "0194f5a6-89ab-7cde-8123-456789abcdef",
                "source_updated_at": "2026-01-01T00:00:00Z",
                "rollout_summary_file": "rollout_summaries/2026-01-01T00-00-00-abcd-test.md",
                "cwd": "/tmp/workspace",
                "git_branch": "feat/memory",
                "keywords": ["memory", "migration"],
                "summary_preview": "migration summary",
                "embedding": vec![0.0625_f32; 256]
            }
        ]
    });
    tokio_fs::write(
        memories_dir.join("vector_index.json"),
        serde_json::to_string_pretty(&vector_index).unwrap(),
    )
    .await
    .unwrap();

    let instructions = build_memory_tool_developer_instructions(
        codex_home,
        &MemoriesConfig::default(),
        Some("memory migration failure in stage2"),
    )
    .await
    .unwrap();

    assert!(instructions.contains(&format!(
        "- {}/memory_summary.md (already provided below; do NOT open again)",
        memories_dir.display()
    )));
    assert!(instructions.contains("## Semantic Recall Hints"));
    assert!(instructions.contains("memory migration failure in stage2"));
    assert!(instructions.contains("rollout_summaries/2026-01-01T00-00-00-abcd-test.md"));
    assert!(instructions.contains("signals="));
}

#[tokio::test]
async fn build_memory_tool_developer_instructions_skips_semantic_hints_when_disabled() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path();
    let memories_dir = codex_home.join("memories");
    tokio_fs::create_dir_all(&memories_dir).await.unwrap();
    tokio_fs::write(
        memories_dir.join("memory_summary.md"),
        "Short memory summary for tests.",
    )
    .await
    .unwrap();
    tokio_fs::write(
        memories_dir.join("vector_index.json"),
        r#"{"dimension":256,"entries":[]}"#,
    )
    .await
    .unwrap();

    let config = MemoriesConfig {
        semantic_index_enabled: false,
        ..MemoriesConfig::default()
    };
    let instructions = build_memory_tool_developer_instructions(
        codex_home,
        &config,
        Some("memory migration failure in stage2"),
    )
    .await
    .unwrap();

    assert!(!instructions.contains("## Semantic Recall Hints"));
}
