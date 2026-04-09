use codex_core::branding::APP_EXECUTABLE_NAME;

pub(crate) const CHATGPT_LOGIN_DISABLED_MESSAGE: &str =
    "ChatGPT login is disabled. Use API key login instead.";
pub(crate) const API_KEY_LOGIN_DISABLED_MESSAGE: &str =
    "API key login is disabled. Use ChatGPT login instead.";
pub(crate) const LOGIN_SUCCESS_MESSAGE: &str = "Successfully logged in";

pub(crate) fn login_server_start_message(actual_port: u16, auth_url: &str) -> String {
    format!(
        "Starting local login server on http://localhost:{actual_port}.\nIf your browser did not open, navigate to this URL to authenticate:\n\n{auth_url}\n\nOn a remote or headless machine? Use `{APP_EXECUTABLE_NAME} login --device-auth` instead."
    )
}

pub(crate) fn api_key_stdin_guidance() -> String {
    format!(
        "--with-api-key expects the API key on stdin. Try piping it, e.g. `printenv OPENAI_API_KEY | {APP_EXECUTABLE_NAME} login --with-api-key`."
    )
}
