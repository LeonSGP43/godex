use std::sync::LazyLock;

use codex_utils_template::Template;

const ANSI_BLUE: &str = "\x1b[94m";
const ANSI_GRAY: &str = "\x1b[90m";
const ANSI_RESET: &str = "\x1b[0m";

static LOGIN_ERROR_PAGE_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    Template::parse(include_str!("assets/error.html"))
        .unwrap_or_else(|err| panic!("login error page template must parse: {err}"))
});

pub(crate) fn device_code_not_enabled_message() -> &'static str {
    "device code login is not enabled for this godex server. Use the browser login or verify the server URL."
}

pub(crate) fn format_device_code_prompt(verification_url: &str, code: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "\nWelcome to godex [v{ANSI_GRAY}{version}{ANSI_RESET}]\n{ANSI_GRAY}Your Codex-compatible command-line coding agent{ANSI_RESET}\n\
\nFollow these steps to sign in with ChatGPT using device code authorization:\n\
\n1. Open this link in your browser and sign in to your account\n   {ANSI_BLUE}{verification_url}{ANSI_RESET}\n\
\n2. Enter this one-time code {ANSI_GRAY}(expires in 15 minutes){ANSI_RESET}\n   {ANSI_BLUE}{code}{ANSI_RESET}\n\
\n{ANSI_GRAY}Device codes are a common phishing target. Never share this code.{ANSI_RESET}\n",
    )
}

pub(crate) fn missing_codex_entitlement_message() -> &'static str {
    "godex is not enabled for your workspace. Contact your workspace administrator to request access to godex."
}

pub(crate) fn missing_authorization_code_message() -> &'static str {
    "Missing authorization code. Sign-in could not be completed."
}

pub(crate) fn persist_failed_message() -> &'static str {
    "Sign-in completed but credentials could not be saved locally."
}

pub(crate) fn redirect_failed_message() -> &'static str {
    "Sign-in completed but redirecting back to godex failed."
}

pub(crate) fn login_cancelled_message() -> &'static str {
    "Login cancelled"
}

pub(crate) fn render_login_error_page(
    message: &str,
    error_code: Option<&str>,
    error_description: Option<&str>,
    missing_entitlement: bool,
) -> Vec<u8> {
    let code = error_code.unwrap_or("unknown_error");
    let (title, display_message, display_description, help_text) = if missing_entitlement {
        (
            "You do not have access to godex".to_string(),
            "This account is not currently authorized to use godex in this workspace."
                .to_string(),
            "Contact your workspace administrator to request access to godex.".to_string(),
            "Contact your workspace administrator to get access to godex, then return to godex and try again."
                .to_string(),
        )
    } else {
        (
            "Sign-in could not be completed".to_string(),
            message.to_string(),
            error_description.unwrap_or(message).to_string(),
            "Return to godex to retry, switch accounts, or contact your workspace admin if access is restricted."
                .to_string(),
        )
    };

    LOGIN_ERROR_PAGE_TEMPLATE
        .render([
            ("error_title", html_escape(&title)),
            ("error_message", html_escape(&display_message)),
            ("error_code", html_escape(code)),
            ("error_description", html_escape(&display_description)),
            ("error_help", html_escape(&help_text)),
        ])
        .unwrap_or_else(|err| panic!("login error page template must render: {err}"))
        .into_bytes()
}

pub(crate) fn success_page_body() -> Vec<u8> {
    include_str!("assets/success.html").as_bytes().to_vec()
}

pub(crate) fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
