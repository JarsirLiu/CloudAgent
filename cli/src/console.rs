use crate::chat_composer::ComposerAction;
use crate::history_cell::{
    HistoryCell, HistoryTone, Transcript, TranscriptRenderState,
};
use crate::input_pane::{ApprovalInlineState, InputPane, InputPaneAction, InputPaneViewState};
use crate::welcome::WelcomeScreen;
use agent_app_server_client::{AppServerClient, InProcessClientConfig, StdioClientConfig};
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode,
    RequestId, TurnItemKind, UserTurnInput,
};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, IsTerminal as _};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct ConsoleBanner {
    pub title: String,
    pub commands: String,
    pub idle_prompt: String,
    pub approval_prompt: String,
}

impl ConsoleBanner {
    pub fn cli(session_id: &str) -> Self {
        Self {
            title: format!("cloudagent session `{session_id}`"),
            commands:
                "Ctrl+J submit  Ctrl+C/Ctrl+Q exit  Ctrl+K interrupt  /clear clear session"
                    .to_string(),
            idle_prompt: "message".to_string(),
            approval_prompt: "approval".to_string(),
        }
    }

    pub fn daemon(session_id: &str) -> Self {
        Self {
            title: format!("agentd console attached to session `{session_id}`"),
            commands: "Ctrl+J submit  Ctrl+C/Ctrl+Q exit  Ctrl+K interrupt".to_string(),
            idle_prompt: "daemon-message".to_string(),
            approval_prompt: "daemon-approval".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct ConsoleConfig {
    pub session_id: String,
    pub banner: ConsoleBanner,
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

#[derive(Clone, Debug)]
struct ConsoleState {
    mode: FrontendMode,
    pending_approval_request_id: Option<RequestId>,
}

impl ConsoleState {
    fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
            pending_approval_request_id: None,
        }
    }

    fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }

    fn update_from_message(&mut self, message: &AppServerMessage) {
        match message {
            AppServerMessage::Notification(notification) => match notification {
                AppServerNotification::FrontendStateChanged { mode, .. } => self.mode = *mode,
                AppServerNotification::TurnCompleted { .. }
                | AppServerNotification::TurnFailed { .. }
                | AppServerNotification::TurnCancelled { .. } => {
                    self.mode = FrontendMode::Idle;
                    self.pending_approval_request_id = None;
                }
                _ => {}
            },
            AppServerMessage::Request(AppServerRequest::Approval { request_id, .. }) => {
                self.mode = FrontendMode::WaitingForApproval;
                self.pending_approval_request_id = Some(request_id.clone());
            }
        }
    }
}

enum ParsedInput {
    Command(AppClientCommand),
    ApprovalAnswer { approved: bool, reason: String },
    LocalCopy,
}

struct TuiApp {
    session_id: String,
    connection_label: String,
    console_state: ConsoleState,
    transcript: Transcript,
    transcript_scroll: usize,
    transcript_viewport_height: usize,
    transcript_viewport_width: usize,
    compact_tools: bool,
    expanded_tool_cells: HashSet<usize>,
    tool_cell_indices: Vec<usize>,
    selected_tool_index: Option<usize>,
    history_loaded: bool,
    status_text: String,
    last_message_count: usize,
    last_tool_name: Option<String>,
    streaming_turn_id: Option<String>,
    streaming_item_id: Option<String>,
    active_turn_id: Option<String>,
    streaming_buffer: String,
    streaming_cell_index: Option<usize>,
    streaming_dirty: bool,
    tool_item_cells: HashMap<String, usize>,
    tool_item_buffers: HashMap<String, String>,
    item_kinds: HashMap<String, TurnItemKind>,
    last_copyable_output: Option<String>,
    input_pane: InputPane,
    should_exit: bool,
}

impl TuiApp {
    fn new(session_id: String, _banner: ConsoleBanner, connection_label: &str) -> Self {
        Self {
            session_id,
            connection_label: connection_label.to_string(),
            console_state: ConsoleState::new(),
            transcript: Transcript::default(),
            transcript_scroll: 0,
            transcript_viewport_height: 0,
            transcript_viewport_width: 0,
            compact_tools: false,
            expanded_tool_cells: HashSet::new(),
            tool_cell_indices: Vec::new(),
            selected_tool_index: None,
            history_loaded: false,
            status_text: format!("Connected via {connection_label}"),
            last_message_count: 0,
            last_tool_name: None,
            streaming_turn_id: None,
            streaming_item_id: None,
            active_turn_id: None,
            streaming_buffer: String::new(),
            streaming_cell_index: None,
            streaming_dirty: false,
            tool_item_cells: HashMap::new(),
            tool_item_buffers: HashMap::new(),
            item_kinds: HashMap::new(),
            last_copyable_output: None,
            input_pane: InputPane::new(),
            should_exit: false,
        }
    }

    fn push_cell(&mut self, cell: HistoryCell) {
        self.preserve_scroll_on_content_change(|this| {
            this.transcript.push(cell);
            this.refresh_tool_focus();
        });
    }

    fn reset_local_view(&mut self) {
        self.console_state = ConsoleState::new();
        self.transcript = Transcript::default();
        self.transcript_scroll = 0;
        self.transcript_viewport_height = 0;
        self.transcript_viewport_width = 0;
        self.compact_tools = false;
        self.expanded_tool_cells.clear();
        self.tool_cell_indices.clear();
        self.selected_tool_index = None;
        self.history_loaded = true;
        self.status_text = format!("Connected via {}", self.connection_label);
        self.last_message_count = 0;
        self.last_tool_name = None;
        self.streaming_turn_id = None;
        self.streaming_item_id = None;
        self.active_turn_id = None;
        self.streaming_buffer.clear();
        self.streaming_cell_index = None;
        self.streaming_dirty = false;
        self.tool_item_cells.clear();
        self.tool_item_buffers.clear();
        self.item_kinds.clear();
        self.last_copyable_output = None;
        self.input_pane.clear_views();
    }

    fn set_mode(&mut self, mode: FrontendMode) {
        self.console_state.mode = mode;
        if mode != FrontendMode::WaitingForApproval {
            self.input_pane.clear_approval();
        }
        self.status_text = match mode {
            FrontendMode::Idle => "Idle".to_string(),
            FrontendMode::Running => "Thinking".to_string(),
            FrontendMode::WaitingForApproval => "Waiting for approval".to_string(),
        };
    }

    fn handle_server_message(&mut self, message: &AppServerMessage) {
        self.console_state.update_from_message(message);
        match message {
            AppServerMessage::Notification(notification) => match notification {
                AppServerNotification::FrontendStateChanged { mode, .. } => self.set_mode(*mode),
                AppServerNotification::SessionStatus { snapshot, .. } => {
                    self.last_message_count = snapshot.message_count;
                    self.status_text = format!(
                        "{:?}  turn={:?}  messages={}",
                        snapshot.session_state, snapshot.turn_state, snapshot.message_count
                    );
                    self.input_pane.set_panel(Some(InputPaneViewState {
                        title: "Session status".to_string(),
                        lines: vec![
                            format!("state: {:?}", snapshot.session_state),
                            format!("active turn: {:?}", snapshot.active_turn),
                            format!("turn state: {:?}", snapshot.turn_state),
                            format!("message count: {}", snapshot.message_count),
                            "Esc closes this panel.".to_string(),
                        ],
                    }));
                }
                AppServerNotification::SessionHistory { messages, .. } => {
                    self.status_text = "Loaded history".to_string();
                    self.last_message_count = messages.len();
                    self.transcript.replace_with_history(messages);
                    self.refresh_tool_focus();
                    self.transcript_scroll = 0;
                    self.clamp_transcript_scroll();
                    self.history_loaded = true;
                }
                AppServerNotification::SubscriptionChanged {
                    session_id,
                    subscribed,
                } => self.push_cell(HistoryCell::from_message(
                    "session",
                    format!(
                        "{} {}",
                        if *subscribed {
                            "Subscribed to"
                        } else {
                            "Unsubscribed from"
                        },
                        session_id
                    ),
                    HistoryTone::Meta,
                )),
                AppServerNotification::Info { message, .. } => {
                    self.status_text = message.clone();
                }
                AppServerNotification::Error { message, .. } => {
                    self.status_text = message.clone();
                    self.input_pane.clear_views();
                    self.push_cell(HistoryCell::from_message(
                        "error",
                        message.clone(),
                        HistoryTone::Error,
                    ));
                }
                AppServerNotification::TurnStarted { .. } => {
                    if let AppServerNotification::TurnStarted { turn_id, .. } = notification {
                        self.active_turn_id = Some(turn_id.clone());
                    }
                    self.status_text = "Working".to_string();
                }
                AppServerNotification::ItemStarted {
                    turn_id,
                    item_id,
                    kind,
                    title,
                    ..
                } => {
                    self.item_kinds.insert(item_id.clone(), kind.clone());
                    if *kind == TurnItemKind::AssistantMessage {
                        self.handle_assistant_item_started(turn_id, item_id);
                    } else if *kind == TurnItemKind::ToolCall {
                        self.handle_tool_item_started(item_id, title.as_deref().unwrap_or("tool_call"));
                    } else if *kind == TurnItemKind::Reasoning {
                        self.handle_tool_item_started(item_id, title.as_deref().unwrap_or("reasoning"));
                    }
                }
                AppServerNotification::AgentMessageDelta { item_id, delta, .. } => {
                    if self.item_kinds.get(item_id) == Some(&TurnItemKind::AssistantMessage) {
                        self.handle_assistant_item_delta(item_id, delta);
                    }
                }
                AppServerNotification::PlanDelta { item_id, delta, .. }
                | AppServerNotification::ReasoningSummaryDelta { item_id, delta, .. }
                | AppServerNotification::ReasoningTextDelta { item_id, delta, .. } => {
                    if self.item_kinds.get(item_id) != Some(&TurnItemKind::AssistantMessage) {
                        self.handle_tool_item_delta(item_id, delta);
                    }
                }
                AppServerNotification::ToolCallDelta { item_id, delta, .. } => {
                    if self.item_kinds.get(item_id) == Some(&TurnItemKind::AssistantMessage) {
                        self.handle_assistant_item_delta(item_id, delta);
                    } else {
                        self.handle_tool_item_delta(item_id, delta);
                    }
                }
                AppServerNotification::ItemCompleted { item_id, .. } => {
                    self.handle_assistant_item_completed(item_id);
                    self.handle_tool_item_completed(item_id);
                    self.item_kinds.remove(item_id);
                    self.commit_streaming_delta();
                }
                AppServerNotification::TurnCompleted { .. } => {
                    self.commit_streaming_delta();
                    self.input_pane.clear_approval();
                    self.status_text = "Turn completed".to_string();
                    self.active_turn_id = None;
                }
                AppServerNotification::TurnFailed { error, .. } => {
                    self.commit_streaming_delta();
                    self.input_pane.clear_approval();
                    self.status_text = "Turn failed".to_string();
                    self.push_cell(HistoryCell::from_message(
                        "turn",
                        format!("failed: {error}"),
                        HistoryTone::Error,
                    ));
                    self.active_turn_id = None;
                }
                AppServerNotification::TurnCancelled { reason, .. } => {
                    self.commit_streaming_delta();
                    self.input_pane.clear_approval();
                    self.status_text = "Turn cancelled".to_string();
                    self.push_cell(HistoryCell::from_message(
                        "turn",
                        reason.clone(),
                        HistoryTone::Warning,
                    ));
                    self.active_turn_id = None;
                }
            },
            AppServerMessage::Request(AppServerRequest::Approval {
                request_id,
                request,
                ..
            }) => {
                self.console_state.pending_approval_request_id = Some(request_id.clone());
                self.set_mode(FrontendMode::WaitingForApproval);
                self.input_pane.set_approval(ApprovalInlineState {
                    title: format!("tool `{}` wants to run", request.tool_name),
                    detail: format!(
                        "reason: {}  args: {}",
                        request.reason, request.arguments_preview
                    ),
                });
                self.status_text = format!("Approval for {}", request.tool_name);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<ParsedInput> {
        if matches!(key.kind, KeyEventKind::Press) {
            let page_step = self.page_scroll_step();
            match key.code {
                KeyCode::PageUp => {
                    self.transcript_scroll = self
                        .transcript_scroll
                        .saturating_add(page_step)
                        .min(self.max_transcript_scroll(self.transcript_viewport_height));
                    return None;
                }
                KeyCode::PageDown => {
                    self.transcript_scroll = self.transcript_scroll.saturating_sub(page_step);
                    return None;
                }
                KeyCode::Home => {
                    self.transcript_scroll =
                        self.max_transcript_scroll(self.transcript_viewport_height);
                    return None;
                }
                KeyCode::End => {
                    self.transcript_scroll = 0;
                    return None;
                }
                _ => {}
            }
        }

        if matches!(key.kind, KeyEventKind::Press) && self.input_pane.composer_is_empty() {
            match key.code {
                KeyCode::Up => {
                    self.transcript_scroll = self
                        .transcript_scroll
                        .saturating_add(1)
                        .min(self.max_transcript_scroll(self.transcript_viewport_height));
                    return None;
                }
                KeyCode::Down => {
                    self.transcript_scroll = self.transcript_scroll.saturating_sub(1);
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
                self.should_exit = true;
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
            self.transcript_scroll = self
                .transcript_scroll
                .saturating_add(step)
                .min(self.max_transcript_scroll(self.transcript_viewport_height));
        } else {
            self.transcript_scroll = self.transcript_scroll.saturating_sub(step);
        }
        self.clamp_transcript_scroll();
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let content = centered_column(area, 112);
        let bottom_height = self
            .input_pane
            .desired_height(self.console_state.mode, content.width)
            .min(content.height.saturating_sub(10).max(6));
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(bottom_height),
            ])
            .split(content);

        frame.render_widget(self.header_block(), sections[0]);
        if self.transcript.is_empty() {
            self.render_welcome(frame, sections[1]);
        } else {
            self.transcript_viewport_height = sections[1].height.saturating_sub(0) as usize;
            self.transcript_viewport_width = sections[1].width.saturating_sub(4) as usize;
            self.clamp_transcript_scroll();
            frame.render_widget(self.transcript_panel(sections[1]), sections[1]);
        }

        let (bottom_widget, lines_before, _) = self.input_pane.render(
            self.console_state.mode,
            &self.status_text,
            &self.status_meta_text(),
            sections[2].width,
        );
        frame.render_widget(bottom_widget, sections[2]);

        let (x, y) =
            self.input_pane
                .cursor_position(sections[2], lines_before, self.console_state.mode);
        frame.set_cursor_position((x, y));
    }

    fn header_block(&self) -> Paragraph<'static> {
        let status = match self.console_state.mode {
            FrontendMode::Idle => ("IDLE", Color::Green),
            FrontendMode::Running => ("RUNNING", Color::Cyan),
            FrontendMode::WaitingForApproval => ("APPROVAL", Color::Yellow),
        };

        let scroll_hint = if self.transcript_scroll > 0 {
            format!("scroll +{}", self.transcript_scroll)
        } else {
            "live".to_string()
        };
        let tool_text = self
            .last_tool_name
            .as_ref()
            .map(|tool| format!("tool {tool}"));

        let mut spans = vec![
            Span::styled(
                "── CloudAgent",
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("session {}", self.session_id),
                Style::default().fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled(
                format!("[{}]", status.0),
                Style::default().fg(status.1).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("msgs {}", self.last_message_count),
                Style::default().fg(Color::Rgb(130, 140, 160)),
            ),
            Span::raw("  "),
            Span::styled(
                self.connection_label.clone(),
                Style::default().fg(Color::Rgb(90, 110, 140)),
            ),
            Span::raw("  "),
            Span::styled(scroll_hint, Style::default().fg(Color::DarkGray)),
        ];
        if let Some(tool_text) = tool_text {
            spans.splice(
                10..10,
                [
                    Span::raw("  "),
                    Span::styled(tool_text, Style::default().fg(Color::Rgb(130, 140, 160))),
                ],
            );
        }

        Paragraph::new(Text::from(vec![Line::from(spans)]))
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let outer = area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
            .split(outer);

        let left_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightRed));
        let right_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightRed));

        let left_inner = left_block.inner(cols[0]);
        let right_inner = right_block.inner(cols[1]);

        frame.render_widget(left_block, cols[0]);
        frame.render_widget(right_block, cols[1]);

        let recent = self.recent_activity_lines();
        let mut tips = vec![
            Line::from(Span::styled(
                "Tips for getting started",
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Run /init when you want a local AGENTS guide.",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Recent activity",
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        tips.extend(recent);
        tips.push(Line::from(""));
        tips.push(Line::from(Span::styled(
            "Try asking:",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )));
        tips.push(Line::from(Span::styled(
            "check disk pressure",
            Style::default().fg(Color::Gray),
        )));
        tips.push(Line::from(Span::styled(
            "inspect this repo and explain it",
            Style::default().fg(Color::Gray),
        )));
        tips.push(Line::from(Span::styled(
            "write a safe nginx restart script",
            Style::default().fg(Color::Gray),
        )));

        frame.render_widget(
            WelcomeScreen::new(self.history_loaded, self.status_text.clone()).render(left_inner),
            left_inner,
        );
        frame.render_widget(
            Paragraph::new(Text::from(tips)).wrap(Wrap { trim: false }),
            right_inner,
        );
    }

    fn transcript_panel(&self, area: Rect) -> Paragraph<'static> {
        let inner = area.inner(Margin {
            vertical: 0,
            horizontal: 2,
        });
        let render_state = self.transcript_render_state();
        let lines = self.transcript.render_lines(
            inner.width as usize,
            inner.height as usize,
            self.transcript_scroll,
            &render_state,
        );
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(Block::default())
    }

    fn recent_activity_lines(&self) -> Vec<Line<'static>> {
        if self.transcript.is_empty() {
            return vec![Line::from(Span::styled(
                "No recent activity",
                Style::default().fg(Color::Gray),
            ))];
        }

        vec![
            Line::from(Span::styled(
                "Session has recent conversation",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "Use F2 to inspect transcript history",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    }

    fn max_transcript_scroll(&self, viewport_height: usize) -> usize {
        let content_width = self.transcript_viewport_width.max(20);
        let total = self
            .transcript
            .total_lines_with_state(content_width, &self.transcript_render_state());
        total.saturating_sub(viewport_height)
    }

    fn total_transcript_lines(&self) -> usize {
        self.transcript
            .total_lines_with_state(self.transcript_viewport_width.max(20), &self.transcript_render_state())
    }

    fn clamp_transcript_scroll(&mut self) {
        self.transcript_scroll = self
            .transcript_scroll
            .min(self.max_transcript_scroll(self.transcript_viewport_height));
    }

    fn preserve_scroll_on_content_change<F>(&mut self, mutate: F)
    where
        F: FnOnce(&mut Self),
    {
        let was_scrolling_history = self.transcript_scroll > 0;
        let before_lines = if was_scrolling_history {
            self.total_transcript_lines()
        } else {
            0
        };
        mutate(self);
        if was_scrolling_history {
            let after_lines = self.total_transcript_lines();
            let appended_lines = after_lines.saturating_sub(before_lines);
            self.transcript_scroll = self.transcript_scroll.saturating_add(appended_lines);
        } else {
            self.transcript_scroll = 0;
        }
        self.clamp_transcript_scroll();
    }

    fn page_scroll_step(&self) -> usize {
        self.transcript_viewport_height
            .saturating_sub(2)
            .clamp(6, 18)
    }

    fn status_meta_text(&self) -> String {
        let mut parts = vec![
            format!("session {}", self.session_id),
            format!("messages {}", self.last_message_count),
        ];
        if let Some(tool) = &self.last_tool_name {
            parts.push(format!("tool {tool}"));
        }
        parts.push(self.connection_label.clone());
        parts.join("  ·  ")
    }

    fn refresh_tool_focus(&mut self) {
        self.tool_cell_indices = self.transcript.tool_cell_indices();
        self.selected_tool_index =
            match (self.selected_tool_index, self.tool_cell_indices.is_empty()) {
                (_, true) => None,
                (Some(current), false) => Some(current.min(self.tool_cell_indices.len() - 1)),
                (None, false) => Some(self.tool_cell_indices.len() - 1),
            };
        self.expanded_tool_cells
            .retain(|idx| self.tool_cell_indices.contains(idx));
    }

    fn transcript_render_state(&self) -> TranscriptRenderState {
        let selected_cell = self
            .selected_tool_index
            .and_then(|idx| self.tool_cell_indices.get(idx).copied());
        TranscriptRenderState {
            compact_tools: self.compact_tools,
            expanded_tool_cells: self.expanded_tool_cells.clone(),
            selected_cell,
            matched_cells: HashSet::new(),
        }
    }

    fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        self.streaming_turn_id = Some(turn_id.to_string());
        self.streaming_item_id = Some(item_id.to_string());
        self.streaming_buffer.clear();
        self.preserve_scroll_on_content_change(|this| {
            let idx = this.transcript.push(HistoryCell::from_message(
                "cloudagent",
                String::new(),
                HistoryTone::Agent,
            ));
            this.streaming_cell_index = Some(idx);
            this.streaming_dirty = true;
        });
    }

    fn handle_assistant_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.streaming_item_id.as_deref() != Some(item_id) {
            return;
        }
        self.streaming_buffer.push_str(delta);
        self.streaming_dirty = true;
        self.status_text = "Streaming response".to_string();
        self.commit_streaming_delta();
    }

    fn handle_assistant_item_completed(&mut self, item_id: &str) {
        if self.streaming_item_id.as_deref() != Some(item_id) {
            return;
        }
        if !self.streaming_buffer.trim().is_empty() {
            self.last_copyable_output = Some(self.streaming_buffer.clone());
            self.last_message_count = self.last_message_count.saturating_add(1);
        }
        self.streaming_turn_id = None;
        self.streaming_item_id = None;
        self.streaming_dirty = true;
        self.status_text = "Finalizing response".to_string();
    }

    fn handle_tool_item_started(&mut self, item_id: &str, title: &str) {
        self.preserve_scroll_on_content_change(|this| {
            let idx = this.transcript.push(HistoryCell::from_message(
                title.to_string(),
                "Started".to_string(),
                HistoryTone::Tool,
            ));
            this.tool_item_cells.insert(item_id.to_string(), idx);
            this.tool_item_buffers
                .insert(item_id.to_string(), "Started".to_string());
            this.last_tool_name = Some(title.to_string());
            this.refresh_tool_focus();
        });
    }

    fn handle_tool_item_delta(&mut self, item_id: &str, delta: &str) {
        let Some(idx) = self.tool_item_cells.get(item_id).copied() else {
            return;
        };
        self.preserve_scroll_on_content_change(|this| {
            let buffer = this
                .tool_item_buffers
                .entry(item_id.to_string())
                .or_default();
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(delta);
            let _ = this.transcript.update_cell_body(idx, buffer.clone());
            this.refresh_tool_focus();
        });
    }

    fn handle_tool_item_completed(&mut self, item_id: &str) {
        self.tool_item_cells.remove(item_id);
        self.tool_item_buffers.remove(item_id);
    }

    fn commit_streaming_delta(&mut self) {
        if !self.streaming_dirty {
            return;
        }
        self.preserve_scroll_on_content_change(|this| {
            if let Some(idx) = this.streaming_cell_index {
                let _ = this.transcript.update_cell_body(idx, this.streaming_buffer.clone());
                this.refresh_tool_focus();
            }
        });
        self.streaming_dirty = false;
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableAlternateScroll;

impl crossterm::Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1007h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<(), std::io::Error> {
        Err(std::io::Error::other(
            "EnableAlternateScroll requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableAlternateScroll;

impl crossterm::Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1007l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<(), std::io::Error> {
        Err(std::io::Error::other(
            "DisableAlternateScroll requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        let _ = execute!(stdout, EnableAlternateScroll);
        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(io::stdout(), DisableAlternateScroll);
        let _ = disable_raw_mode();
    }
}

enum UiEvent {
    Key(KeyEvent),
    MouseScroll { up: bool },
    Tick,
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
    let mut app = TuiApp::new(session_id.clone(), config.banner, config.connection.label());
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
                        app.commit_streaming_delta();
                    }
                }
            }
            else => break,
        }

        if app.should_exit {
            break;
        }
    }

    client.shutdown().await
}

async fn create_client(config: &ConsoleConfig, session_id: String) -> Result<AppServerClient> {
    match &config.connection {
        ConsoleConnection::InProcess { runtime } => {
            Ok(AppServerClient::in_process(InProcessClientConfig {
                runtime: runtime.clone(),
                session_id,
                auto_approve: config.auto_approve,
                auto_approve_reason: config.auto_approve_reason.clone(),
            }))
        }
        ConsoleConnection::Stdio { program, args } => {
            AppServerClient::stdio(StdioClientConfig {
                program: program.clone(),
                args: args.clone(),
            })
            .await
        }
    }
}

fn spawn_tui_event_loop() -> mpsc::UnboundedReceiver<UiEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(120)) {
                Ok(true) => match event::read() {
                    Ok(CEvent::Key(key)) => {
                        if tx.send(UiEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CEvent::Mouse(mouse)) => {
                        let scroll = match mouse.kind {
                            MouseEventKind::ScrollUp => Some(true),
                            MouseEventKind::ScrollDown => Some(false),
                            _ => None,
                        };
                        if let Some(up) = scroll {
                            if tx.send(UiEvent::MouseScroll { up }).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {
                    if tx.send(UiEvent::Tick).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

fn handle_tui_input(
    session_id: &str,
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalCopy => {
            let Some(text) = app.last_copyable_output.as_deref() else {
                app.push_cell(HistoryCell::from_message(
                    "session",
                    "`/copy` unavailable before first assistant output",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            };
            match copy_text_to_clipboard(text) {
                Ok(()) => {
                    app.status_text = "Copied latest assistant output".to_string();
                }
                Err(err) => {
                    app.push_cell(HistoryCell::from_message(
                        "error",
                        format!("failed to copy: {err}"),
                        HistoryTone::Error,
                    ));
                }
            }
        }
        ParsedInput::Command(command) => {
            if let AppClientCommand::Exit = command {
                if app.console_state.mode != FrontendMode::Idle {
                    client.send_command(AppClientCommand::InterruptTurn {
                        session_id: session_id.to_string(),
                    })?;
                }
                app.should_exit = true;
                return Ok(true);
            }

            if matches!(command, AppClientCommand::SubmitTurn(_))
                && !app.console_state.can_submit_turn()
            {
                app.push_cell(HistoryCell::from_message(
                    "session",
                    "turn already running; wait, answer approval, or interrupt first",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }

            if let AppClientCommand::ApprovalResponse { .. } = &command {
                app.console_state.mode = FrontendMode::Running;
                app.console_state.pending_approval_request_id = None;
                app.input_pane.clear_views();
            }
            if let AppClientCommand::ResetSession { .. } = &command {
                app.reset_local_view();
                client.send_command(command)?;
                return Ok(false);
            }
            if let AppClientCommand::SubmitTurn(UserTurnInput { content, .. }) = &command {
                app.console_state.mode = FrontendMode::Running;
                app.status_text = "Submitting turn".to_string();
                app.input_pane.clear_views();
                app.push_cell(HistoryCell::from_message(
                    "you",
                    content.clone(),
                    HistoryTone::User,
                ));
                app.last_message_count = app.last_message_count.saturating_add(1);
            }
            client.send_command(command)?;
        }
        ParsedInput::ApprovalAnswer { approved, reason } => {
            let Some(request_id) = app.console_state.pending_approval_request_id.clone() else {
                app.push_cell(HistoryCell::from_message(
                    "approval",
                    "no pending approval request",
                    HistoryTone::Error,
                ));
                return Ok(false);
            };
            app.console_state.mode = FrontendMode::Running;
            app.console_state.pending_approval_request_id = None;
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message(
                "approval",
                if approved { "approved" } else { "denied" },
                if approved {
                    HistoryTone::Agent
                } else {
                    HistoryTone::Warning
                },
            ));
            client.send_command(AppClientCommand::ApprovalResponse {
                session_id: session_id.to_string(),
                request_id,
                approved,
                reason: Some(reason),
            })?;
        }
    }
    Ok(false)
}

fn parse_line(line: &str, session_id: &str, mode: FrontendMode) -> ParsedInput {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedInput::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: String::new(),
        }));
    }

    let command = match trimmed {
        "/copy" => return ParsedInput::LocalCopy,
        "/exit" | "/quit" => AppClientCommand::Exit,
        "/clear" => AppClientCommand::ResetSession {
            session_id: session_id.to_string(),
        },
        "/interrupt" => AppClientCommand::InterruptTurn {
            session_id: session_id.to_string(),
        },
        _ if mode == FrontendMode::WaitingForApproval => {
            let approved = matches!(trimmed, "1" | "y" | "Y" | "yes" | "YES");
            return ParsedInput::ApprovalAnswer {
                approved,
                reason: if approved {
                    "approved by console operator".to_string()
                } else {
                    "denied by console operator".to_string()
                },
            };
        }
        _ => AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: trimmed.to_string(),
        }),
    };

    ParsedInput::Command(command)
}

fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| anyhow::anyhow!("clipboard unavailable: {err}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| anyhow::anyhow!("clipboard write failed: {err}"))?;
    Ok(())
}

fn centered_column(area: Rect, max_width: u16) -> Rect {
    let width = area.width.min(max_width);
    let horizontal_padding = area.width.saturating_sub(width) / 2;
    Rect {
        x: area.x + horizontal_padding,
        y: area.y,
        width,
        height: area.height,
    }
}
