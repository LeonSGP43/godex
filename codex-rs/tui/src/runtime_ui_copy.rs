use ratatui::prelude::Line;
use ratatui::prelude::Span;
use ratatui::style::Stylize;

pub(crate) fn branded_title_line(indent: &str, version: &str) -> Line<'static> {
    Line::from(vec![
        Span::from(format!("{indent}>_ ")).dim(),
        Span::from(codex_core::branding::APP_DISPLAY_NAME).bold(),
        Span::from(" ").dim(),
        Span::from(format!("(v{version})")).dim(),
    ])
}

pub(crate) fn session_header_help_lines() -> Vec<Line<'static>> {
    vec![
        "  To get started, describe a task or try one of these commands:"
            .dim()
            .into(),
        Line::from(""),
        Line::from(vec![
            "  ".into(),
            "/init".into(),
            " - create an AGENTS.md file with instructions for godex".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            "/status".into(),
            " - show current session configuration".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            "/permissions".into(),
            " - choose what godex is allowed to do".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            "/model".into(),
            " - choose what model and reasoning effort to use".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            "/review".into(),
            " - review any changes and find issues".dim(),
        ]),
    ]
}

pub(crate) fn browser_open_failed_message(url: &str, err: &str) -> String {
    format!("Failed to open browser for {url}: {err}")
}

pub(crate) fn browser_opened_message(url: &str) -> String {
    format!("Opened {url} in your browser.")
}
