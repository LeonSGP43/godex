use codex_config::types::MemoriesConfig;
use codex_config::types::MemoryScopeMode;
use std::path::Path;
use std::path::PathBuf;

pub(crate) const GLOBAL_MEMORY_SCOPE_KIND: &str = "global";
pub(crate) const GLOBAL_MEMORY_SCOPE_KEY: &str = "global";
const SCOPES_SUBDIR: &str = "scopes";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryScope {
    pub(crate) kind: String,
    pub(crate) key: String,
}

pub(crate) fn resolve_memory_scope(
    cwd: &Path,
    project_root_markers: &[String],
    memories: &MemoriesConfig,
) -> MemoryScope {
    match memories.scope {
        MemoryScopeMode::Global => global_memory_scope(),
        MemoryScopeMode::Project => {
            let root = find_project_root(cwd, project_root_markers).unwrap_or(cwd);
            MemoryScope {
                kind: "project".to_string(),
                key: root.display().to_string(),
            }
        }
    }
}

pub(crate) fn scoped_memory_root(codex_home: &Path, scope: &MemoryScope) -> PathBuf {
    let root = super::memory_root(codex_home);
    if scope.kind == GLOBAL_MEMORY_SCOPE_KIND {
        return root;
    }

    root.join(SCOPES_SUBDIR)
        .join(scope.kind.as_str())
        .join(scope_directory_name(scope.key.as_str()))
}

fn global_memory_scope() -> MemoryScope {
    MemoryScope {
        kind: GLOBAL_MEMORY_SCOPE_KIND.to_string(),
        key: GLOBAL_MEMORY_SCOPE_KEY.to_string(),
    }
}

fn find_project_root<'a>(cwd: &'a Path, project_root_markers: &[String]) -> Option<&'a Path> {
    if project_root_markers.is_empty() {
        return Some(cwd);
    }

    for ancestor in cwd.ancestors() {
        for marker in project_root_markers {
            if ancestor.join(marker).exists() {
                return Some(ancestor);
            }
        }
    }

    Some(cwd)
}

fn scope_directory_name(scope_key: &str) -> String {
    let leaf = Path::new(scope_key)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_scope_uses_legacy_memory_root() {
        let scope = MemoryScope {
            kind: GLOBAL_MEMORY_SCOPE_KIND.to_string(),
            key: GLOBAL_MEMORY_SCOPE_KEY.to_string(),
        };
        assert_eq!(
            scoped_memory_root(Path::new("/tmp/codex"), &scope),
            PathBuf::from("/tmp/codex/memories")
        );
    }

    #[test]
    fn project_scope_uses_partitioned_memory_root() {
        let scope = MemoryScope {
            kind: "project".to_string(),
            key: "/tmp/workspaces/codex".to_string(),
        };
        let root = scoped_memory_root(Path::new("/tmp/codex"), &scope);
        assert!(
            root.starts_with("/tmp/codex/memories/scopes/project"),
            "unexpected root: {}",
            root.display()
        );
        assert!(
            root.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("codex-")),
            "unexpected scope directory: {}",
            root.display()
        );
    }
}
