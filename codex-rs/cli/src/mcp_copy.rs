use codex_core::branding::APP_EXECUTABLE_NAME;
use codex_core::config::ConfigNamespace;

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
