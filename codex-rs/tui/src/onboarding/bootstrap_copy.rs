use crate::legacy_core::branding::APP_DISPLAY_NAME;

pub(crate) const OFFICIAL_CODEX_DOCS_LABEL: &str = "official Codex docs";
pub(crate) const WELCOME_DESCRIPTION: &str = "your Codex-compatible command-line coding agent";
pub(crate) const API_KEY_BILLING_DESCRIPTION: &str =
    "Codex will use usage-based billing with your API key.";

pub(crate) fn paid_plan_login_prompt() -> String {
    format!("Sign in with ChatGPT to use {APP_DISPLAY_NAME} as part of your paid plan")
}

pub(crate) fn autonomy_prompt() -> String {
    format!("Decide how much autonomy you want to grant {APP_DISPLAY_NAME}")
}

pub(crate) fn mistakes_warning() -> String {
    format!("{APP_DISPLAY_NAME} can make mistakes")
}
