#[cfg(not(debug_assertions))]
use codex_core::config::Config;

/// Update action the CLI should perform after the TUI exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g @leonsgp43/godex@latest`.
    NpmGlobalLatest,
    /// Update via `bun install -g @leonsgp43/godex@latest`.
    BunGlobalLatest,
    /// Update via `brew upgrade --cask godex`.
    BrewUpgrade,
    /// Update a source-built godex checkout by syncing its configured upstream.
    SourceRepoSync,
}

impl UpdateAction {
    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest => (
                "npm",
                &[
                    "install",
                    "-g",
                    codex_core::branding::APP_PACKAGE_INSTALL_SPEC,
                ],
            ),
            UpdateAction::BunGlobalLatest => (
                "bun",
                &[
                    "install",
                    "-g",
                    codex_core::branding::APP_PACKAGE_INSTALL_SPEC,
                ],
            ),
            UpdateAction::BrewUpgrade => (
                "brew",
                &[
                    "upgrade",
                    "--cask",
                    codex_core::branding::APP_BREW_PACKAGE_NAME,
                ],
            ),
            UpdateAction::SourceRepoSync => (
                codex_core::branding::APP_EXECUTABLE_NAME,
                &["sync-upstream"],
            ),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn get_update_action(config: &Config) -> Option<UpdateAction> {
    let _ = config;
    let exe = std::env::current_exe().unwrap_or_default();
    let managed_by_npm = std::env::var_os("CODEX_MANAGED_BY_NPM").is_some();
    let managed_by_bun = std::env::var_os("CODEX_MANAGED_BY_BUN").is_some();

    detect_update_action(
        cfg!(target_os = "macos"),
        &exe,
        managed_by_npm,
        managed_by_bun,
    )
}

#[cfg(any(not(debug_assertions), test))]
fn detect_update_action(
    is_macos: bool,
    current_exe: &std::path::Path,
    managed_by_npm: bool,
    managed_by_bun: bool,
) -> Option<UpdateAction> {
    if managed_by_npm {
        Some(UpdateAction::NpmGlobalLatest)
    } else if managed_by_bun {
        Some(UpdateAction::BunGlobalLatest)
    } else if is_macos
        && (current_exe.starts_with("/opt/homebrew") || current_exe.starts_with("/usr/local"))
    {
        Some(UpdateAction::BrewUpgrade)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_update_action_without_env_mutation() {
        assert_eq!(
            detect_update_action(
                /*is_macos*/ false,
                std::path::Path::new("/any/path"),
                /*managed_by_npm*/ false,
                /*managed_by_bun*/ false
            ),
            None
        );
        assert_eq!(
            detect_update_action(
                /*is_macos*/ false,
                std::path::Path::new("/any/path"),
                /*managed_by_npm*/ true,
                /*managed_by_bun*/ false
            ),
            Some(UpdateAction::NpmGlobalLatest)
        );
        assert_eq!(
            detect_update_action(
                /*is_macos*/ false,
                std::path::Path::new("/any/path"),
                /*managed_by_npm*/ false,
                /*managed_by_bun*/ true
            ),
            Some(UpdateAction::BunGlobalLatest)
        );
        assert_eq!(
            detect_update_action(
                /*is_macos*/ true,
                std::path::Path::new("/opt/homebrew/bin/godex"),
                /*managed_by_npm*/ false,
                /*managed_by_bun*/ false
            ),
            Some(UpdateAction::BrewUpgrade)
        );
        assert_eq!(
            detect_update_action(
                /*is_macos*/ true,
                std::path::Path::new("/usr/local/bin/godex"),
                /*managed_by_npm*/ false,
                /*managed_by_bun*/ false
            ),
            Some(UpdateAction::BrewUpgrade)
        );
    }
}
