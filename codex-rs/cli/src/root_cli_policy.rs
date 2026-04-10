use anyhow::Result;
use codex_core::config::ConfigNamespace;
use codex_core::config::find_home;
use codex_core::config::namespace_for_godex_home_flag;
use codex_tui::Cli as TuiCli;
use codex_utils_cli::CliConfigOverrides;
use std::path::PathBuf;

use crate::FeatureToggles;
use crate::MemoryScopeCliArg;

#[derive(Debug, Clone)]
pub(crate) struct RootCliPolicy {
    pub(crate) config_namespace: ConfigNamespace,
    root_config_overrides: CliConfigOverrides,
    use_godex_home: bool,
}

impl RootCliPolicy {
    pub(crate) fn from_root_args(
        use_godex_home: bool,
        memory_scope: Option<MemoryScopeCliArg>,
        feature_toggles: FeatureToggles,
        mut root_config_overrides: CliConfigOverrides,
    ) -> Result<Self> {
        let config_namespace = namespace_for_godex_home_flag(use_godex_home);

        // Fold root-only flags into the same override stream so downstream
        // subcommands inherit identical config semantics.
        root_config_overrides
            .raw_overrides
            .extend(feature_toggles.to_overrides()?);
        if let Some(memory_scope) = memory_scope {
            root_config_overrides
                .raw_overrides
                .push(memory_scope.raw_override());
        }

        Ok(Self {
            config_namespace,
            root_config_overrides,
            use_godex_home,
        })
    }

    pub(crate) fn apply_to_interactive(&self, interactive: &mut TuiCli) {
        interactive.use_godex_home = self.use_godex_home;
    }

    pub(crate) fn prepend_root_config_overrides(
        &self,
        subcommand_config_overrides: &mut CliConfigOverrides,
    ) {
        subcommand_config_overrides
            .raw_overrides
            .splice(0..0, self.root_config_overrides.raw_overrides.clone());
    }

    pub(crate) fn cloned_root_config_overrides(&self) -> CliConfigOverrides {
        self.root_config_overrides.clone()
    }

    pub(crate) fn resolve_config_home(&self) -> Result<PathBuf> {
        Ok(find_home(self.config_namespace)?)
    }
}

#[cfg(test)]
mod tests {
    use super::RootCliPolicy;
    use crate::FeatureToggles;
    use crate::MemoryScopeCliArg;
    use clap::Parser;
    use codex_core::config::ConfigNamespace;
    use codex_tui::Cli as TuiCli;
    use codex_utils_cli::CliConfigOverrides;

    #[test]
    fn root_policy_applies_namespace_and_root_overrides() {
        let policy = RootCliPolicy::from_root_args(
            true,
            Some(MemoryScopeCliArg::Project),
            FeatureToggles::default(),
            CliConfigOverrides {
                raw_overrides: vec!["model=\"o3\"".to_string()],
            },
        )
        .expect("build root policy");

        assert_eq!(policy.config_namespace, ConfigNamespace::GodexIsolated);
        assert_eq!(
            policy.cloned_root_config_overrides().raw_overrides,
            vec![
                "model=\"o3\"".to_string(),
                "memories.scope=\"project\"".to_string(),
            ]
        );
    }

    #[test]
    fn root_policy_prepend_preserves_subcommand_precedence() {
        let policy = RootCliPolicy::from_root_args(
            false,
            None,
            FeatureToggles {
                enable: vec!["unified_exec".to_string()],
                disable: Vec::new(),
            },
            CliConfigOverrides {
                raw_overrides: vec!["model=\"o3\"".to_string()],
            },
        )
        .expect("build root policy");

        let mut subcommand_overrides = CliConfigOverrides {
            raw_overrides: vec!["model=\"gpt-5\"".to_string()],
        };
        policy.prepend_root_config_overrides(&mut subcommand_overrides);

        assert_eq!(
            subcommand_overrides.raw_overrides,
            vec![
                "model=\"o3\"".to_string(),
                "features.unified_exec=true".to_string(),
                "model=\"gpt-5\"".to_string(),
            ]
        );
    }

    #[test]
    fn root_policy_marks_interactive_cli_for_godex_home() {
        let policy = RootCliPolicy::from_root_args(
            true,
            None,
            FeatureToggles::default(),
            CliConfigOverrides::default(),
        )
        .expect("build root policy");
        let mut interactive = TuiCli::try_parse_from(["godex"]).expect("parse TUI cli");

        policy.apply_to_interactive(&mut interactive);

        assert!(interactive.use_godex_home);
    }
}
