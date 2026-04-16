use super::App;
#[cfg(not(debug_assertions))]
use super::AppRunControl;
#[cfg(not(debug_assertions))]
use super::ExitReason;
#[cfg(not(debug_assertions))]
use crate::app_event::AppEvent;
#[cfg(not(debug_assertions))]
use crate::app_server_session::AppServerSession;
use crate::bottom_pane::StatusLineItem;
use crate::bottom_pane::TerminalTitleItem;
use crate::history_cell;
use crate::history_cell::HistoryCell;
#[cfg(not(debug_assertions))]
use crate::history_cell::UpdateAvailableHistoryCell;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use crate::runtime_ui_copy::browser_open_failed_message;
use crate::runtime_ui_copy::browser_opened_message;
use crate::runtime_ui_copy::permissions_updated_message;
use crate::runtime_ui_copy::status_line_save_failed_message;
use crate::runtime_ui_copy::terminal_title_save_failed_message;
use crate::tui;
#[cfg(not(debug_assertions))]
use crate::updates::GodexUpdateNotice;
#[cfg(not(debug_assertions))]
use crate::updates::UpstreamReleaseGapNotice;
use crate::version::CODEX_CLI_VERSION;
use color_eyre::eyre::Result;
use ratatui::text::Line;

impl App {
    #[cfg(not(debug_assertions))]
    async fn insert_startup_history_cell(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        cell: Box<dyn HistoryCell>,
    ) -> Result<Option<ExitReason>> {
        let control = self
            .handle_event(tui, app_server, AppEvent::InsertHistoryCell(cell))
            .await?;
        Ok(match control {
            AppRunControl::Continue => None,
            AppRunControl::Exit(reason) => Some(reason),
        })
    }

    #[cfg(not(debug_assertions))]
    pub(super) async fn show_startup_notices(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        godex_update_notice: Option<GodexUpdateNotice>,
        upstream_release_gap_notice: Option<UpstreamReleaseGapNotice>,
    ) -> Result<Option<ExitReason>> {
        if let Some(notice) = godex_update_notice {
            let update_action = crate::update_action::get_update_action(&self.config);
            if let Some(reason) = self
                .insert_startup_history_cell(
                    tui,
                    app_server,
                    Box::new(UpdateAvailableHistoryCell::new(
                        notice.current_version,
                        notice.latest_version,
                        notice.release_notes_url,
                        update_action,
                    )),
                )
                .await?
            {
                return Ok(Some(reason));
            }
        }

        if let Some(notice) = upstream_release_gap_notice {
            return self
                .insert_startup_history_cell(
                    tui,
                    app_server,
                    Box::new(history_cell::UpstreamVersionGapHistoryCell::new(
                        notice.current_version,
                        notice.latest_version,
                        notice.releases_ahead,
                        notice.release_notes_url,
                    )),
                )
                .await;
        }

        Ok(None)
    }

    pub(super) fn show_permissions_updated_message(&mut self, label: &str) {
        self.chat_widget
            .add_info_message(permissions_updated_message(label), /*hint*/ None);
    }

    pub(super) fn open_url_in_browser(&mut self, url: String) {
        if let Err(err) = webbrowser::open(&url) {
            self.chat_widget
                .add_error_message(browser_open_failed_message(&url, &err.to_string()));
            return;
        }

        self.chat_widget
            .add_info_message(browser_opened_message(&url), /*hint*/ None);
    }

    pub(super) fn clear_ui_header_lines_with_version(
        &self,
        width: u16,
        version: &'static str,
    ) -> Vec<Line<'static>> {
        history_cell::SessionHeaderHistoryCell::new(
            self.chat_widget.current_model().to_string(),
            self.chat_widget.current_reasoning_effort(),
            self.chat_widget.should_show_fast_status(
                self.chat_widget.current_model(),
                self.chat_widget.current_service_tier(),
            ),
            self.config.cwd.to_path_buf(),
            version,
        )
        .display_lines(width)
    }

    pub(super) fn clear_ui_header_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.clear_ui_header_lines_with_version(width, CODEX_CLI_VERSION)
    }

    pub(super) fn queue_clear_ui_header(&mut self, tui: &mut tui::Tui) {
        let width = tui.terminal.last_known_screen_size.width;
        let header_lines = self.clear_ui_header_lines(width);
        if !header_lines.is_empty() {
            tui.insert_history_lines(header_lines);
            self.has_emitted_history_lines = true;
        }
    }

    pub(super) fn clear_terminal_ui(
        &mut self,
        tui: &mut tui::Tui,
        redraw_header: bool,
    ) -> Result<()> {
        let is_alt_screen_active = tui.is_alt_screen_active();

        // Drop queued history insertions so stale transcript lines cannot be flushed after /clear.
        tui.clear_pending_history_lines();

        if is_alt_screen_active {
            tui.terminal.clear_visible_screen()?;
        } else {
            // Some terminals (Terminal.app, Warp) do not reliably drop scrollback when purge and
            // clear are emitted as separate backend commands. Prefer a single ANSI sequence.
            tui.terminal.clear_scrollback_and_visible_screen_ansi()?;
        }

        let mut area = tui.terminal.viewport_area;
        if area.y > 0 {
            // After a full clear, anchor the inline viewport at the top and redraw a fresh header
            // box. `insert_history_lines()` will shift the viewport down by the rendered height.
            area.y = 0;
            tui.terminal.set_viewport_area(area);
        }
        self.has_emitted_history_lines = false;

        if redraw_header {
            self.queue_clear_ui_header(tui);
        }
        Ok(())
    }

    pub(super) async fn apply_status_line_setup(&mut self, items: Vec<StatusLineItem>) {
        let ids = items.iter().map(ToString::to_string).collect::<Vec<_>>();
        let edit = crate::legacy_core::config::edit::status_line_items_edit(&ids);
        let apply_result = ConfigEditsBuilder::new(&self.config.codex_home)
            .with_edits([edit])
            .apply()
            .await;
        match apply_result {
            Ok(()) => {
                self.config.tui_status_line = Some(ids);
                self.chat_widget.setup_status_line(items);
            }
            Err(err) => {
                tracing::error!(
                    error = %err,
                    "failed to persist status line items; keeping previous selection"
                );
                self.chat_widget
                    .add_error_message(status_line_save_failed_message(&err.to_string()));
            }
        }
    }

    pub(super) async fn apply_terminal_title_setup(&mut self, items: Vec<TerminalTitleItem>) {
        let ids = items.iter().map(ToString::to_string).collect::<Vec<_>>();
        let edit = crate::legacy_core::config::edit::terminal_title_items_edit(&ids);
        let apply_result = ConfigEditsBuilder::new(&self.config.codex_home)
            .with_edits([edit])
            .apply()
            .await;
        match apply_result {
            Ok(()) => {
                self.config.tui_terminal_title = Some(ids);
                self.chat_widget.setup_terminal_title(items);
            }
            Err(err) => {
                tracing::error!(
                    error = %err,
                    "failed to persist terminal title items; keeping previous selection"
                );
                self.chat_widget.revert_terminal_title_setup_preview();
                self.chat_widget
                    .add_error_message(terminal_title_save_failed_message(&err.to_string()));
            }
        }
    }
}
