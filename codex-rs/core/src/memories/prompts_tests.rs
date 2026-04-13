use super::*;
use crate::fork_patch::memory::build_memory_context_fragment;
use crate::fork_patch::memory::memory_summary_file;
use crate::fork_patch::memory::vector_index_file;
use codex_config::types::MemoriesConfig;
use codex_models_manager::model_info::model_info_from_slug;
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
    let temp = tempdir().unwrap();
    let memories_dir = temp.path().join("memories");

    let prompt = build_consolidation_prompt(&memories_dir, &Phase2InputSelection::default());

    assert!(prompt.contains(&format!(
        "Folder structure (under {}/):",
        memories_dir.display()
    )));
    assert!(prompt.contains("memory_index.qmd"));
    assert!(prompt.contains("vector_index.json"));
    assert!(prompt.contains("**Diff since last consolidation:**"));
    assert!(prompt.contains("- selected inputs this run: 0"));
}

#[tokio::test]
async fn build_consolidation_prompt_points_to_extensions_without_inlining_them() {
    let temp = tempdir().unwrap();
    let memories_dir = temp.path().join("memories");
    let extension_dir = temp.path().join("memories_extensions/tape_recorder");
    tokio_fs::create_dir_all(extension_dir.join("resources"))
        .await
        .unwrap();
    tokio_fs::write(
        extension_dir.join("instructions.md"),
        "source-specific instructions\n",
    )
    .await
    .unwrap();
    tokio_fs::write(
        extension_dir.join("resources/notes.md"),
        "source-specific resource\n",
    )
    .await
    .unwrap();

    let prompt = build_consolidation_prompt(&memories_dir, &Phase2InputSelection::default());
    let memory_extensions_dir = temp.path().join("memories_extensions");

    assert!(prompt.contains(&format!(
        "Memory extensions (under {}/)",
        memory_extensions_dir.display()
    )));
    assert!(prompt.contains(&format!("Under `{}/`:", memory_extensions_dir.display())));
    assert!(prompt.contains("<extension_name>/instructions.md"));
    assert!(prompt.contains("Optional source-specific inputs:"));
    assert!(!prompt.contains("source-specific instructions"));
    assert!(!prompt.contains("source-specific resource"));
}

#[tokio::test]
async fn build_memory_tool_developer_instructions_renders_embedded_template() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path();
    let memories_dir = codex_home.join("memories");
    tokio_fs::create_dir_all(&memories_dir).await.unwrap();
    tokio_fs::write(
        memory_summary_file(&memories_dir),
        "Short memory summary for tests.",
    )
    .await
    .unwrap();

    let instructions = build_memory_context_fragment(
        codex_home,
        &MemoriesConfig::default(),
        GLOBAL_MEMORY_SCOPE_KIND,
        GLOBAL_MEMORY_SCOPE_KEY,
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
        memory_summary_file(&memories_dir),
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
        vector_index_file(&memories_dir),
        serde_json::to_string_pretty(&vector_index).unwrap(),
    )
    .await
    .unwrap();

    let instructions = build_memory_context_fragment(
        codex_home,
        &MemoriesConfig::default(),
        GLOBAL_MEMORY_SCOPE_KIND,
        GLOBAL_MEMORY_SCOPE_KEY,
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
        memory_summary_file(&memories_dir),
        "Short memory summary for tests.",
    )
    .await
    .unwrap();
    tokio_fs::write(
        vector_index_file(&memories_dir),
        r#"{"dimension":256,"entries":[]}"#,
    )
    .await
    .unwrap();

    let config = MemoriesConfig {
        semantic_index_enabled: false,
        ..MemoriesConfig::default()
    };
    let instructions = build_memory_context_fragment(
        codex_home,
        &config,
        GLOBAL_MEMORY_SCOPE_KIND,
        GLOBAL_MEMORY_SCOPE_KEY,
        Some("memory migration failure in stage2"),
    )
    .await
    .unwrap();

    assert!(!instructions.contains("## Semantic Recall Hints"));
}
