use codex_core::branding::APP_EXECUTABLE_NAME;

pub(crate) const CHATGPT_LOGIN_DISABLED_MESSAGE: &str =
    "ChatGPT login is disabled. Use API key login instead.";
pub(crate) const API_KEY_LOGIN_DISABLED_MESSAGE: &str =
    "API key login is disabled. Use ChatGPT login instead.";
pub(crate) const LOGIN_SUCCESS_MESSAGE: &str = "Successfully logged in";
pub const WITH_API_KEY_HELP: &str =
    "Read the API key from stdin (e.g. `printenv OPENAI_API_KEY | godex login --with-api-key`)";
pub const API_KEY_FLAG_DEPRECATED_HELP: &str =
    "(deprecated) Previously accepted the API key directly; now exits with guidance to use --with-api-key";

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

pub fn deprecated_api_key_flag_guidance() -> String {
    format!(
        "The --api-key flag is no longer supported. Pipe the key instead, e.g. `printenv OPENAI_API_KEY | {APP_EXECUTABLE_NAME} login --with-api-key`."
    )
}
