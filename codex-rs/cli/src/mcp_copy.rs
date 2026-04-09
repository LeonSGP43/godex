use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use codex_core::branding::APP_EXECUTABLE_NAME;
use codex_core::config::Config;
use codex_core::config::ConfigNamespace;
use codex_core::config::find_home;
use codex_core::config::load_global_mcp_servers;
use codex_core::config::types::McpServerConfig;
use codex_utils_cli::CliConfigOverrides;

pub(crate) const MCP_ADD_OVERRIDE_USAGE: &str =
    "godex mcp add [OPTIONS] <NAME> (--url <URL> | -- <COMMAND>...)";

pub(crate) fn config_namespace_for_home_flag(use_godex_home: bool) -> ConfigNamespace {
    if use_godex_home {
        ConfigNamespace::GodexIsolated
    } else {
        ConfigNamespace::CodexCompatible
    }
}

pub(crate) fn unknown_oauth_login_message(name: &str) -> String {
    format!(
        "MCP server may or may not require login. Run `{APP_EXECUTABLE_NAME} mcp login {name}` to login."
    )
}

pub(crate) fn no_configured_servers_message() -> String {
    format!(
        "No MCP servers configured yet. Try `{APP_EXECUTABLE_NAME} mcp add my-tool -- my-command`."
    )
}

pub(crate) fn remove_server_hint(name: &str) -> String {
    format!("  remove: {APP_EXECUTABLE_NAME} mcp remove {name}")
}

pub(crate) fn validate_cli_overrides(config_overrides: &CliConfigOverrides) -> Result<()> {
    config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

pub(crate) async fn load_cli_config(config_overrides: &CliConfigOverrides) -> Result<Config> {
    let overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    Config::load_with_cli_overrides(overrides)
        .await
        .context("failed to load configuration")
}

pub(crate) async fn load_global_mcp_store(
    config_namespace: ConfigNamespace,
) -> Result<(PathBuf, BTreeMap<String, McpServerConfig>)> {
    let codex_home = find_home(config_namespace).context("failed to resolve config home")?;
    let servers = load_global_mcp_servers(&codex_home)
        .await
        .with_context(|| format!("failed to load MCP servers from {}", codex_home.display()))?;
    Ok((codex_home, servers))
}
