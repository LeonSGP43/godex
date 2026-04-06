use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use toml::Value as TomlValue;

#[tokio::test]
async fn launch_overrides_resolve_distinct_memory_roots() -> std::io::Result<()> {
    let codex_home = tempdir().expect("tempdir");
    let project_dir = codex_home.path().join("workspace/project-alpha");
    let nested_cwd = project_dir.join("src/bin");
    std::fs::create_dir_all(project_dir.join(".git"))?;
    std::fs::create_dir_all(&nested_cwd)?;

    let cwd_override = ConfigOverrides {
        cwd: Some(nested_cwd.clone()),
        ..Default::default()
    };

    let global_config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .cli_overrides(vec![(
            "memories.scope".to_string(),
            TomlValue::String("global".to_string()),
        )])
        .harness_overrides(cwd_override.clone())
        .build()
        .await?;

    let project_config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .cli_overrides(vec![(
            "memories.scope".to_string(),
            TomlValue::String("project".to_string()),
        )])
        .harness_overrides(cwd_override)
        .build()
        .await?;

    let repeated_project_config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .cli_overrides(vec![(
            "memories.scope".to_string(),
            TomlValue::String("project".to_string()),
        )])
        .harness_overrides(ConfigOverrides {
            cwd: Some(nested_cwd.clone()),
            ..Default::default()
        })
        .build()
        .await?;

    let global_root = global_config.codex_home.join("memories");
    let project_root = project_config
        .codex_home
        .join("memories/scopes/project")
        .join(project_scope_dir_name(
            project_config.memory_scope_key.as_str(),
        ));
    let repeated_project_root = repeated_project_config
        .codex_home
        .join("memories/scopes/project")
        .join(project_scope_dir_name(
            repeated_project_config.memory_scope_key.as_str(),
        ));

    assert_eq!(global_config.memory_scope_kind, "global");
    assert_eq!(global_config.memory_scope_key, "global");
    assert_eq!(global_root, codex_home.path().join("memories"));

    assert_eq!(project_config.memory_scope_kind, "project");
    assert_eq!(
        project_config.memory_scope_key,
        project_dir.to_string_lossy().to_string()
    );
    assert!(
        project_root.starts_with(codex_home.path().join("memories/scopes/project")),
        "unexpected project root: {}",
        project_root.display()
    );
    assert_ne!(project_root, global_root);
    assert_eq!(project_root, repeated_project_root);

    std::fs::create_dir_all(&global_root)?;
    std::fs::create_dir_all(&project_root)?;
    std::fs::write(global_root.join("MEMORY.md"), "global only\n")?;
    std::fs::write(project_root.join("MEMORY.md"), "project only\n")?;
    assert_eq!(
        std::fs::read_to_string(global_root.join("MEMORY.md"))?,
        "global only\n"
    );
    assert_eq!(
        std::fs::read_to_string(project_root.join("MEMORY.md"))?,
        "project only\n"
    );

    Ok(())
}

fn project_scope_dir_name(scope_key: &str) -> String {
    let leaf = std::path::Path::new(scope_key)
        .file_name()
        .and_then(|component| component.to_str())
        .unwrap_or("scope");
    let slug = sanitize_scope_component(leaf);
    format!("{slug}-{:016x}", stable_fnv1a64(scope_key.as_bytes()))
}

fn sanitize_scope_component(component: &str) -> String {
    let sanitized = component
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if sanitized.is_empty() {
        "scope".to_string()
    } else {
        sanitized
    }
}

fn stable_fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
