use super::ConfigNamespace;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

pub fn default_config_namespace() -> ConfigNamespace {
    ConfigNamespace::CodexCompatible
}

pub fn namespace_for_godex_home_flag(use_godex_home: bool) -> ConfigNamespace {
    if use_godex_home {
        ConfigNamespace::GodexIsolated
    } else {
        default_config_namespace()
    }
}

pub fn find_codex_home() -> std::io::Result<PathBuf> {
    find_home(default_config_namespace())
}

pub fn find_home(namespace: ConfigNamespace) -> std::io::Result<PathBuf> {
    codex_utils_home_dir::find_home(namespace)
}

pub fn resolve_config_home(
    codex_home: Option<PathBuf>,
    config_namespace: ConfigNamespace,
) -> std::io::Result<PathBuf> {
    codex_home.map_or_else(|| find_home(config_namespace), Ok)
}

pub fn infer_config_namespace(codex_home: &Path) -> ConfigNamespace {
    match codex_home.file_name().and_then(|name| name.to_str()) {
        Some(".godex") => ConfigNamespace::GodexIsolated,
        _ => default_config_namespace(),
    }
}

pub fn maybe_configure_isolated_godex_home_from_args() -> std::io::Result<()> {
    maybe_configure_isolated_godex_home_from_iter(std::env::args_os().skip(1))
}

fn maybe_configure_isolated_godex_home_from_iter<I, S>(args: I) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let use_godex_home = args.into_iter().any(|arg| is_godex_home_flag(arg.as_ref()));
    if !use_godex_home {
        return Ok(());
    }

    let godex_home = find_home(ConfigNamespace::GodexIsolated)?;
    std::fs::create_dir_all(&godex_home).map_err(|err| {
        std::io::Error::new(
            err.kind(),
            format!(
                "failed to initialize isolated godex home at {}: {err}",
                godex_home.display()
            ),
        )
    })?;

    unsafe {
        std::env::set_var("GODEX_HOME", &godex_home);
        std::env::set_var("CODEX_HOME", &godex_home);
    }
    Ok(())
}

fn is_godex_home_flag(arg: &OsStr) -> bool {
    arg == "-g" || arg == "--godex-home"
}

#[cfg(test)]
mod tests {
    use super::ConfigNamespace;
    use super::default_config_namespace;
    use super::infer_config_namespace;
    use super::is_godex_home_flag;
    use super::namespace_for_godex_home_flag;
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn default_namespace_is_codex_compatible() {
        assert_eq!(default_config_namespace(), ConfigNamespace::CodexCompatible);
    }

    #[test]
    fn godex_home_flag_switches_namespace() {
        assert_eq!(
            namespace_for_godex_home_flag(true),
            ConfigNamespace::GodexIsolated
        );
        assert_eq!(
            namespace_for_godex_home_flag(false),
            ConfigNamespace::CodexCompatible
        );
    }

    #[test]
    fn infer_namespace_detects_dot_godex_home() {
        assert_eq!(
            infer_config_namespace(Path::new("/tmp/.godex")),
            ConfigNamespace::GodexIsolated
        );
        assert_eq!(
            infer_config_namespace(Path::new("/tmp/.codex")),
            ConfigNamespace::CodexCompatible
        );
    }

    #[test]
    fn godex_home_flag_matcher_accepts_short_and_long_flags() {
        assert!(is_godex_home_flag(OsStr::new("-g")));
        assert!(is_godex_home_flag(OsStr::new("--godex-home")));
        assert!(!is_godex_home_flag(OsStr::new("--version")));
    }
}
