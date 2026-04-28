use crate::chat_composer::ComposerAction;
use crate::history_cell::{
    HistoryCell, HistoryTone, Transcript, render_history_entry, render_turn_event,
};
use crate::input_pane::{ApprovalInlineState, InputPane, InputPaneAction, InputPaneViewState};
use crate::welcome::WelcomeScreen;
use agent_app_server_client::{AppServerClient, InProcessClientConfig, StdioClientConfig};
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode,
    RequestId, UserTurnInput,
};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
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
                "Ctrl+J submit  Ctrl+C/Ctrl+Q exit  Ctrl+K interrupt  F2 history  F3 status  F4 reset"
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
                AppServerNotification::TurnFinished { .. } => {
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
}

struct TuiApp {
    session_id: String,
    connection_label: String,
    console_state: ConsoleState,
    transcript: Transcript,
    transcript_scroll: usize,
    history_loaded: bool,
    show_history_panel_on_next_response: bool,
    status_text: String,
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
            history_loaded: false,
            show_history_panel_on_next_response: false,
            status_text: format!("Connected via {connection_label}"),
            input_pane: InputPane::new(),
            should_exit: false,
        }
    }

    fn push_cell(&mut self, cell: HistoryCell) {
        self.transcript.push(cell);
        self.transcript_scroll = 0;
    }

    fn reset_local_view(&mut self) {
        self.console_state = ConsoleState::new();
        self.transcript = Transcript::default();
        self.transcript_scroll = 0;
        self.history_loaded = true;
        self.show_history_panel_on_next_response = false;
        self.status_text = format!("Connected via {}", self.connection_label);
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
                    if !self.history_loaded || self.transcript.is_empty() {
                        self.transcript.replace_with_history(messages);
                        self.transcript_scroll = 0;
                    }
                    self.history_loaded = true;
                    if self.show_history_panel_on_next_response {
                        let history_lines = messages
                            .iter()
                            .map(|message| {
                                let cell = render_history_entry(message);
                                cell.to_lines(96)
                                    .into_iter()
                                    .map(|line| line.to_string())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .collect::<Vec<_>>();
                        self.input_pane.set_panel(Some(InputPaneViewState {
                            title: "Session history".to_string(),
                            lines: if history_lines.is_empty() {
                                vec![
                                    "No history yet.".to_string(),
                                    "Esc closes this panel.".to_string(),
                                ]
                            } else {
                                let mut lines = history_lines;
                                lines.push("Esc closes this panel.".to_string());
                                lines
                            },
                        }));
                        self.show_history_panel_on_next_response = false;
                    }
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
                AppServerNotification::TurnFinished { result, .. } => {
                    self.set_mode(FrontendMode::Idle);
                    self.input_pane.clear_views();
                    if let Some(error) = &result.error {
                        self.push_cell(HistoryCell::from_message(
                            "turn",
                            format!("failed: {error}"),
                            HistoryTone::Error,
                        ));
                    } else {
                        self.status_text = "Turn completed".to_string();
                    }
                }
                AppServerNotification::TurnEvent { event, .. } => {
                    match event {
                        agent_protocol::TurnEvent::ApprovalResolved { .. } => {
                            self.input_pane.clear_approval();
                        }
                        agent_protocol::TurnEvent::TurnCompleted { .. }
                        | agent_protocol::TurnEvent::TurnCancelled { .. }
                        | agent_protocol::TurnEvent::TurnFailed { .. } => {
                            self.input_pane.clear_approval();
                        }
                        _ => {}
                    }
                    let rendered = render_turn_event(event);
                    if let Some(status) = rendered.status {
                        self.status_text = status;
                    }
                    if let Some(cell) = rendered.log {
                        self.push_cell(cell);
                    }
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
            match key.code {
                KeyCode::PageUp | KeyCode::Up => {
                    self.transcript_scroll = self.transcript_scroll.saturating_add(6);
                    return None;
                }
                KeyCode::PageDown | KeyCode::Down => {
                    self.transcript_scroll = self.transcript_scroll.saturating_sub(6);
                    return None;
                }
                KeyCode::Home => {
                    self.transcript_scroll = self.max_transcript_scroll(0);
                    return None;
                }
                KeyCode::End => {
                    self.transcript_scroll = 0;
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
            InputPaneAction::Composer(ComposerAction::History) => {
                self.show_history_panel_on_next_response = true;
                Some(ParsedInput::Command(AppClientCommand::RequestHistory {
                    session_id: self.session_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerAction::Status) => Some(ParsedInput::Command(AppClientCommand::RequestStatus {
                session_id: self.session_id.clone(),
            })),
            InputPaneAction::Composer(ComposerAction::Reset) => Some(ParsedInput::Command(AppClientCommand::ResetSession {
                session_id: self.session_id.clone(),
            })),
            InputPaneAction::Composer(ComposerAction::None) => None,
            InputPaneAction::ApprovalSubmit { approved, reason } => Some(ParsedInput::ApprovalAnswer { approved, reason }),
        }
    }

    fn render(&self, frame: &mut Frame) {
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
            frame.render_widget(self.transcript_panel(sections[1]), sections[1]);
        }

        let (bottom_widget, lines_before, _) = self.input_pane.render(
            self.console_state.mode,
            &self.status_text,
            sections[2].width,
        );
        frame.render_widget(bottom_widget, sections[2]);

        let (x, y) = self.input_pane.cursor_position(
            sections[2],
            lines_before,
            self.console_state.mode,
        );
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

        Paragraph::new(Text::from(vec![Line::from(vec![
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
            Span::styled(scroll_hint, Style::default().fg(Color::DarkGray)),
        ])]))
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
        let lines = self.transcript.render_lines(
            inner.width as usize,
            inner.height as usize,
            self.transcript_scroll,
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
        let content_width = 108usize;
        let total = self.transcript.total_lines(content_width);
        total.saturating_sub(viewport_height)
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = disable_raw_mode();
    }
}

enum UiEvent {
    Key(KeyEvent),
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
                    UiEvent::Tick => {}
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
        return ParsedInput::Command(AppClientCommand::RequestStatus {
            session_id: session_id.to_string(),
        });
    }

    let command = match trimmed {
        "/exit" | "/quit" => AppClientCommand::Exit,
        "/reset" => AppClientCommand::ResetSession {
            session_id: session_id.to_string(),
        },
        "/history" => AppClientCommand::RequestHistory {
            session_id: session_id.to_string(),
        },
        "/status" => AppClientCommand::RequestStatus {
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
