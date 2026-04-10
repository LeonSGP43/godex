use codex_login::CodexAuth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoginCommandExit {
    message: String,
    exit_code: i32,
}

impl LoginCommandExit {
    fn new(message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

pub(crate) fn exit_with_login_command_result(result: LoginCommandExit) -> ! {
    eprintln!("{}", result.message());
    std::process::exit(result.exit_code());
}

pub(crate) fn login_status_exit(result: std::io::Result<Option<CodexAuth>>) -> LoginCommandExit {
    match result {
        Ok(Some(auth)) => login_status_exit_for_auth(auth),
        Ok(None) => LoginCommandExit::new("Not logged in", 1),
        Err(err) => LoginCommandExit::new(format!("Error checking login status: {err}"), 1),
    }
}

pub(crate) fn logout_exit(result: std::io::Result<bool>) -> LoginCommandExit {
    match result {
        Ok(true) => LoginCommandExit::new("Successfully logged out", 0),
        Ok(false) => LoginCommandExit::new("Not logged in", 0),
        Err(err) => LoginCommandExit::new(format!("Error logging out: {err}"), 1),
    }
}

fn login_status_exit_for_auth(auth: CodexAuth) -> LoginCommandExit {
    if auth.is_api_key_auth() {
        return match auth.get_token() {
            Ok(api_key) => LoginCommandExit::new(
                format!("Logged in using an API key - {}", safe_format_key(&api_key)),
                0,
            ),
            Err(err) => {
                LoginCommandExit::new(format!("Unexpected error retrieving API key: {err}"), 1)
            }
        };
    }

    LoginCommandExit::new("Logged in using ChatGPT", 0)
}

fn safe_format_key(key: &str) -> String {
    if key.len() <= 13 {
        return "***".to_string();
    }
    let prefix = &key[..8];
    let suffix = &key[key.len() - 5..];
    format!("{prefix}***{suffix}")
}

#[cfg(test)]
mod tests {
    use super::login_status_exit;
    use super::logout_exit;
    use super::safe_format_key;
    use codex_login::CodexAuth;
    use std::io;

    #[test]
    fn formats_long_key() {
        let key = "sk-proj-1234567890ABCDE";
        assert_eq!(safe_format_key(key), "sk-proj-***ABCDE");
    }

    #[test]
    fn short_key_returns_stars() {
        let key = "sk-proj-12345";
        assert_eq!(safe_format_key(key), "***");
    }

    #[test]
    fn login_status_reports_api_key_auth() {
        let result =
            login_status_exit(Ok(Some(CodexAuth::from_api_key("sk-proj-1234567890ABCDE"))));
        assert_eq!(
            result.message(),
            "Logged in using an API key - sk-proj-***ABCDE"
        );
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn login_status_reports_chatgpt_auth() {
        let result =
            login_status_exit(Ok(Some(CodexAuth::create_dummy_chatgpt_auth_for_testing())));
        assert_eq!(result.message(), "Logged in using ChatGPT");
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn login_status_reports_not_logged_in() {
        let result = login_status_exit(Ok(None));
        assert_eq!(result.message(), "Not logged in");
        assert_eq!(result.exit_code(), 1);
    }

    #[test]
    fn login_status_reports_storage_error() {
        let result = login_status_exit(Err(io::Error::other("boom")));
        assert_eq!(result.message(), "Error checking login status: boom");
        assert_eq!(result.exit_code(), 1);
    }

    #[test]
    fn logout_reports_removed_credentials() {
        let result = logout_exit(Ok(true));
        assert_eq!(result.message(), "Successfully logged out");
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn logout_reports_not_logged_in_without_failure() {
        let result = logout_exit(Ok(false));
        assert_eq!(result.message(), "Not logged in");
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn logout_reports_errors() {
        let result = logout_exit(Err(io::Error::other("boom")));
        assert_eq!(result.message(), "Error logging out: boom");
        assert_eq!(result.exit_code(), 1);
    }
}
