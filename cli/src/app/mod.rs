pub mod actions;
pub mod effects;
mod parse;

use crate::app::actions::{execute_server_action, handle_tui_input};
use crate::app::parse::{ParsedInput, parse_line};
use crate::state::{ApprovalState, ConsoleState, RunState, TranscriptState};
use crate::state::reducer::{TurnDispatch, apply_server_message};
use crate::transport::client::create_client;
use crate::terminal::{TerminalGuard, UiEvent, spawn_tui_event_loop};
use crate::ui::screen::render_app;
use crate::ui::widgets::chat_composer::ComposerAction;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use crate::ui::widgets::input_pane::{InputPane, InputPaneAction};
use agent_protocol::{AppClientCommand, AppServerMessage, FrontendMode, TurnItemKind};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use std::ffi::OsString;
use std::io::{self, IsTerminal as _};
use std::sync::Arc;

#[derive(Clone)]
pub struct ConsoleConfig {
    pub session_id: String,
    pub auto_approve: bool,
    pub auto_approve_reason: Option<String>,
    pub connection: ConsoleConnection,
}

#[derive(Clone)]
pub enum ConsoleConnection {
    InProcess {
        runtime: Arc<AgentRuntime>,
    },
    Stdio {
        program: OsString,
        args: Vec<OsString>,
    },
}

impl ConsoleConnection {
    fn label(&self) -> &'static str {
        match self {
            Self::InProcess { .. } => "in-process",
            Self::Stdio { .. } => "stdio-bridge",
        }
    }
}

pub(crate) struct TuiApp {
    pub(crate) session_id: String,
    pub(crate) connection_label: String,
    pub(crate) console_state: ConsoleState,
    pub(crate) approval_state: ApprovalState,
    pub(crate) transcript_state: TranscriptState,
    pub(crate) run_state: RunState,
    pub(crate) input_pane: InputPane,
}

impl TuiApp {
    fn new(session_id: String, connection_label: &str) -> Self {
        Self {
            session_id,
            connection_label: connection_label.to_string(),
            console_state: ConsoleState::new(),
            approval_state: ApprovalState::default(),
            transcript_state: TranscriptState::default(),
            run_state: RunState::new(connection_label),
            input_pane: InputPane::new(),
        }
    }

    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        self.preserve_scroll_on_content_change(|this| {
            this.transcript_state.transcript.push(cell);
        });
    }

    pub(crate) fn reset_local_view(&mut self) {
        self.console_state = ConsoleState::new();
        self.approval_state = ApprovalState::default();
        self.transcript_state = TranscriptState::default();
        self.run_state = RunState::new(&self.connection_label);
        self.run_state.history_loaded = true;
        self.input_pane.clear_views();
    }

    pub(crate) fn set_mode(&mut self, mode: FrontendMode) {
        self.console_state.mode = mode;
        if mode != FrontendMode::WaitingForApproval {
            self.input_pane.clear_approval();
        }
    }

    fn handle_server_message(&mut self, message: &AppServerMessage) {
        let reduced = apply_server_message(message);
        for action in reduced.actions {
            execute_server_action(self, action);
        }
    }

    pub(crate) fn apply_turn_dispatch(&mut self, dispatch: TurnDispatch) {
        self.flush_active_cell_to_transcript();
        match dispatch {
            TurnDispatch::Completed => {}
            TurnDispatch::Failed { error } => {
                self.push_cell(HistoryCell::from_message(
                    "turn",
                    format!("failed: {error}"),
                    HistoryTone::Error,
                ));
            }
            TurnDispatch::Cancelled { reason } => {
                self.push_cell(HistoryCell::from_message(
                    "turn",
                    reason,
                    HistoryTone::Warning,
                ));
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<ParsedInput> {
        if matches!(key.kind, KeyEventKind::Press) {
            let page_step = self.page_scroll_step();
            match key.code {
                KeyCode::PageUp => {
                    self.transcript_state.scroll = self
                        .transcript_state
                        .scroll
                        .saturating_add(page_step)
                        .min(self.max_transcript_scroll(self.transcript_state.viewport_height));
                    return None;
                }
                KeyCode::PageDown => {
                    self.transcript_state.scroll = self.transcript_state.scroll.saturating_sub(page_step);
                    return None;
                }
                KeyCode::Home => {
                    self.transcript_state.scroll =
                        self.max_transcript_scroll(self.transcript_state.viewport_height);
                    return None;
                }
                KeyCode::End => {
                    self.transcript_state.scroll = 0;
                    return None;
                }
                _ => {}
            }
        }

        if matches!(key.kind, KeyEventKind::Press) && self.input_pane.composer_is_empty() {
            match key.code {
                KeyCode::Up => {
                    self.transcript_state.scroll = self
                        .transcript_state
                        .scroll
                        .saturating_add(1)
                        .min(self.max_transcript_scroll(self.transcript_state.viewport_height));
                    return None;
                }
                KeyCode::Down => {
                    self.transcript_state.scroll = self.transcript_state.scroll.saturating_sub(1);
                    return None;
                }
                _ => {}
            }
        }

        match self.input_pane.handle_key(key)? {
            InputPaneAction::Composer(ComposerAction::Submit(text)) => {
                Some(parse_line(&text, &self.session_id, self.console_state.mode))
            }
            InputPaneAction::Composer(ComposerAction::Interrupt) => {
                Some(ParsedInput::Command(AppClientCommand::InterruptTurn {
                    session_id: self.session_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerAction::Exit) => {
                self.run_state.should_exit = true;
                Some(ParsedInput::Command(AppClientCommand::Exit))
            }
            InputPaneAction::Composer(ComposerAction::Reset) => {
                Some(ParsedInput::Command(AppClientCommand::ResetSession {
                    session_id: self.session_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerAction::None) => None,
            InputPaneAction::ApprovalSubmit { approved, reason } => {
                Some(ParsedInput::ApprovalAnswer { approved, reason })
            }
        }
    }

    fn handle_mouse_scroll(&mut self, up: bool) {
        let step = 3usize;
        if up {
            self.transcript_state.scroll = self
                .transcript_state
                .scroll
                .saturating_add(step)
                .min(self.max_transcript_scroll(self.transcript_state.viewport_height));
        } else {
            self.transcript_state.scroll = self.transcript_state.scroll.saturating_sub(step);
        }
        self.clamp_transcript_scroll();
    }

    fn render(&mut self, frame: &mut Frame) {
        render_app(self, frame);
    }

    fn max_transcript_scroll(&self, viewport_height: usize) -> usize {
        let content_width = self.transcript_state.viewport_width.max(20);
        let total = self
            .transcript_state
            .transcript
            .total_lines_with_tail(content_width, self.transcript_state.active_cell.as_ref());
        total.saturating_sub(viewport_height)
    }

    fn total_transcript_lines(&self) -> usize {
        self.transcript_state
            .transcript
            .total_lines_with_tail(self.transcript_state.viewport_width.max(20), self.transcript_state.active_cell.as_ref())
    }

    pub(crate) fn clamp_transcript_scroll(&mut self) {
        self.transcript_state.scroll = self
            .transcript_state
            .scroll
            .min(self.max_transcript_scroll(self.transcript_state.viewport_height));
    }

    fn preserve_scroll_on_content_change<F>(&mut self, mutate: F)
    where
        F: FnOnce(&mut Self),
    {
        let was_scrolling_history = self.transcript_state.scroll > 0;
        let before_lines = if was_scrolling_history {
            self.total_transcript_lines()
        } else {
            0
        };
        mutate(self);
        if was_scrolling_history {
            let after_lines = self.total_transcript_lines();
            let appended_lines = after_lines.saturating_sub(before_lines);
            self.transcript_state.scroll = self.transcript_state.scroll.saturating_add(appended_lines);
        } else {
            self.transcript_state.scroll = 0;
        }
        self.clamp_transcript_scroll();
    }

    fn page_scroll_step(&self) -> usize {
        self.transcript_state.viewport_height
            .saturating_sub(2)
            .clamp(6, 18)
    }

    pub(crate) fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        let _ = turn_id;
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(TurnItemKind::AssistantMessage);
        self.transcript_state.active_cell = Some(HistoryCell::from_message(
            "cloudagent",
            String::new(),
            HistoryTone::Agent,
        ));
    }

    pub(crate) fn handle_assistant_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id)
            || self.transcript_state.active_item_kind != Some(TurnItemKind::AssistantMessage)
        {
            return;
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.append_body(delta);
        }
    }

    pub(crate) fn handle_assistant_item_completed(&mut self, item_id: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id)
            || self.transcript_state.active_item_kind != Some(TurnItemKind::AssistantMessage)
        {
            return;
        }
        let has_text = self
            .transcript_state
            .active_cell
            .as_ref()
            .is_some_and(|cell| !cell.body.trim().is_empty());
        if has_text {
            self.transcript_state.last_copyable_output =
                self.transcript_state.active_cell.as_ref().map(|c| c.body.clone());
            self.run_state.last_message_count = self.run_state.last_message_count.saturating_add(1);
            self.flush_active_cell_to_transcript();
        } else {
            self.clear_active_cell();
        }
    }

    pub(crate) fn handle_tool_item_started(&mut self, item_id: &str, title: &str) {
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(TurnItemKind::ToolCall);
        self.transcript_state.active_cell = Some(HistoryCell::from_message(
            title.to_string(),
            String::new(),
            HistoryTone::Tool,
        ));
        self.run_state.last_tool_name = Some(title.to_string());
    }

    pub(crate) fn handle_tool_item_completed(&mut self, item_id: &str) {
        if self.transcript_state.active_item_id.as_deref() == Some(item_id) {
            self.flush_active_cell_to_transcript();
        }
        self.run_state.last_tool_name = None;
    }

    fn clear_active_cell(&mut self) {
        self.transcript_state.active_item_id = None;
        self.transcript_state.active_item_kind = None;
        self.transcript_state.active_cell = None;
    }

    fn flush_active_cell_to_transcript(&mut self) {
        let Some(cell) = self.transcript_state.active_cell.take() else {
            self.clear_active_cell();
            return;
        };
        if !cell.body.trim().is_empty() {
            self.preserve_scroll_on_content_change(|this| {
                this.transcript_state.transcript.push(cell);
            });
        }
        self.clear_active_cell();
    }
}

pub async fn run_console(config: ConsoleConfig) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("cloudagent cli requires an interactive terminal");
    }
    run_tui_console(config).await
}

async fn run_tui_console(config: ConsoleConfig) -> Result<()> {
    let session_id = config.session_id.clone();
    let mut client = create_client(&config, session_id.clone()).await?;
    let mut app = TuiApp::new(session_id.clone(), config.connection.label());
    client.send_command(AppClientCommand::RequestHistory {
        session_id: session_id.clone(),
    })?;
    let mut terminal = TerminalGuard::new()?;
    let mut events = spawn_tui_event_loop();

    loop {
        terminal.terminal.draw(|frame| app.render(frame))?;

        tokio::select! {
            Some(message) = client.next_message() => {
                app.handle_server_message(&message);
            }
            Some(event) = events.recv() => {
                match event {
                    UiEvent::Key(key) => {
                        if let Some(input) = app.handle_key(key) {
                            if handle_tui_input(&session_id, &mut app, &client, input)? {
                                break;
                            }
                        }
                    }
                    UiEvent::MouseScroll { up } => {
                        app.handle_mouse_scroll(up);
                    }
                    UiEvent::Tick => {
                        // Active-cell rendering is event-driven; periodic ticks only keep UI responsive.
                    }
                }
            }
            else => break,
        }

        if app.run_state.should_exit {
            break;
        }
    }

    client.shutdown().await
}
