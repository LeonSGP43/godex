use crate::branding::APP_EXECUTABLE_NAME;

pub(crate) fn github_mcp_personal_access_token_message(server_name: &str) -> String {
    format!(
        "GitHub MCP does not support OAuth. Log in by adding a personal access token (https://github.com/settings/personal-access-tokens) to your environment and config.toml:\n[mcp_servers.{server_name}]\nbearer_token_env_var = CODEX_GITHUB_PERSONAL_ACCESS_TOKEN"
    )
}

pub(crate) fn mcp_login_required_message(server_name: &str) -> String {
    format!(
        "The {server_name} MCP server is not logged in. Run `{APP_EXECUTABLE_NAME} mcp login {server_name}`."
    )
}

pub(crate) fn mcp_startup_timeout_message(
    server_name: &str,
    startup_timeout_secs: u64,
) -> String {
    format!(
        "MCP client for `{server_name}` timed out after {startup_timeout_secs} seconds. Add or adjust `startup_timeout_sec` in your config.toml:\n[mcp_servers.{server_name}]\nstartup_timeout_sec = XX"
    )
}
