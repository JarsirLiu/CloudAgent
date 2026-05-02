pub mod actions;
pub mod effects;
mod parse;

use crate::app::actions::{execute_server_action, handle_tui_input};
use crate::app::parse::{ParsedInput, parse_line};
use crate::input::intent::ComposerIntent;
use crate::state::reducer::{TurnDispatch, apply_server_message};
use crate::state::{ConsoleState, RunState, ServerRequestState, TranscriptState};
use crate::terminal::{Frame, ScrollbackSurface, TerminalGuard, UiEvent, spawn_tui_event_loop};
use crate::transport::client::create_client;
use crate::ui::chat_surface::ChatSurface;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use crate::ui::widgets::input_pane::{InputPane, InputPaneAction};
use agent_app_server_client::AppServerEvent;
use agent_protocol::{AppClientCommand, AppServerMessage, FrontendMode, TurnItemKind};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use crossterm::event::KeyEvent;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, IsTerminal as _};
use std::sync::Arc;

#[derive(Clone)]
pub struct ConsoleConfig {
    pub conversation_id: String,
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
    pub(crate) conversation_id: String,
    pub(crate) connection_label: String,
    pub(crate) console_state: ConsoleState,
    pub(crate) server_request_state: ServerRequestState,
    pub(crate) transcript_state: TranscriptState,
    pub(crate) run_state: RunState,
    pub(crate) input_pane: InputPane,
    pub(crate) welcome_animation_frame: u64,
    welcome_animation_pause_ticks: u8,
    pending_history_cells: VecDeque<HistoryCell>,
    pending_history_rebuild: bool,
}

impl TuiApp {
    fn new(conversation_id: String, connection_label: &str) -> Self {
        Self {
            conversation_id,
            connection_label: connection_label.to_string(),
            console_state: ConsoleState::new(),
            server_request_state: ServerRequestState::default(),
            transcript_state: TranscriptState::default(),
            run_state: RunState::new(connection_label),
            input_pane: InputPane::new(),
            welcome_animation_frame: 0,
            welcome_animation_pause_ticks: 0,
            pending_history_cells: VecDeque::new(),
            pending_history_rebuild: false,
        }
    }

    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        self.transcript_state.transcript.push(cell.clone());
        self.pending_history_cells.push_back(cell);
    }

    pub(crate) fn replace_history_cells(&mut self, cells: Vec<HistoryCell>) {
        self.transcript_state
            .transcript
            .replace_cells(cells.clone());
        self.pending_history_cells = cells.into();
        self.pending_history_rebuild = true;
    }

    pub(crate) fn drain_pending_history_cells(&mut self) -> Vec<HistoryCell> {
        self.pending_history_cells.drain(..).collect()
    }

    pub(crate) fn clear_pending_history_cells(&mut self) {
        self.pending_history_cells.clear();
    }

    pub(crate) fn take_pending_history_rebuild(&mut self) -> bool {
        std::mem::take(&mut self.pending_history_rebuild)
    }

    pub(crate) fn history_cells(&self) -> &[HistoryCell] {
        self.transcript_state.transcript.cells()
    }

    pub(crate) fn reset_local_view(&mut self) {
        self.console_state = ConsoleState::new();
        self.server_request_state = ServerRequestState::default();
        self.transcript_state = TranscriptState::default();
        self.run_state = RunState::new(&self.connection_label);
        self.run_state.history_loaded = true;
        self.input_pane.clear_views();
        self.welcome_animation_frame = 0;
        self.welcome_animation_pause_ticks = 0;
        self.pending_history_cells.clear();
        self.pending_history_rebuild = false;
    }

    pub(crate) fn set_mode(&mut self, mode: FrontendMode) {
        self.console_state.mode = mode;
    }

    fn handle_server_message(&mut self, message: &AppServerMessage) {
        let reduced = apply_server_message(message);
        for action in reduced.actions {
            execute_server_action(self, action);
        }
    }

    fn handle_client_event(&mut self, event: AppServerEvent) {
        match event {
            AppServerEvent::Message(message) => self.handle_server_message(&message),
            AppServerEvent::Lagged { skipped } => {
                self.run_state.status_notice = Some(format!(
                    "UI skipped {skipped} non-critical events while catching up"
                ));
            }
            AppServerEvent::Disconnected { message } => {
                self.push_cell(HistoryCell::from_message(
                    "conversation",
                    message,
                    HistoryTone::Error,
                ));
                self.run_state.should_exit = true;
            }
        }
    }

    pub(crate) fn apply_turn_dispatch(&mut self, dispatch: TurnDispatch) {
        match dispatch {
            TurnDispatch::Completed => {
                self.flush_active_cell_to_transcript();
            }
            TurnDispatch::Failed { error } => {
                self.flush_active_cell_to_transcript();
                self.push_cell(HistoryCell::from_message(
                    "turn",
                    format!("failed: {error}"),
                    HistoryTone::Error,
                ));
            }
            TurnDispatch::Cancelled { reason } => {
                self.flush_active_cell_to_transcript();
                self.push_cell(HistoryCell::from_message(
                    "turn",
                    reason,
                    HistoryTone::Warning,
                ));
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<ParsedInput> {
        match self.input_pane.handle_key(key)? {
            InputPaneAction::Composer(ComposerIntent::Submit(text)) => Some(parse_line(
                &text,
                &self.conversation_id,
                self.console_state.mode,
            )),
            InputPaneAction::Composer(ComposerIntent::Interrupt) => {
                Some(ParsedInput::Command(AppClientCommand::InterruptTurn {
                    conversation_id: self.conversation_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerIntent::Compact) => Some(ParsedInput::Command(
                AppClientCommand::CompactConversation {
                    conversation_id: self.conversation_id.clone(),
                },
            )),
            InputPaneAction::Composer(ComposerIntent::Copy) => Some(ParsedInput::LocalCopy),
            InputPaneAction::Composer(ComposerIntent::Help) => Some(ParsedInput::LocalHelp),
            InputPaneAction::Composer(ComposerIntent::UnknownCommand(command)) => {
                Some(ParsedInput::LocalInputError(format!(
                    "Unrecognized command '/{command}'. Type '/' for available commands."
                )))
            }
            InputPaneAction::Composer(ComposerIntent::Exit) => {
                self.run_state.should_exit = true;
                Some(ParsedInput::Command(AppClientCommand::Exit))
            }
            InputPaneAction::Composer(ComposerIntent::Reset) => {
                Some(ParsedInput::Command(AppClientCommand::ResetConversation {
                    conversation_id: self.conversation_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerIntent::None) => None,
            InputPaneAction::ServerRequestSubmit {
                request_id,
                decision,
                reason,
            } => Some(ParsedInput::ServerRequestAnswer {
                request_id,
                decision,
                reason,
            }),
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        ChatSurface::render(self, frame);
    }

    fn needs_animation_frame(&self) -> bool {
        self.transcript_state.transcript.is_empty()
            && self.run_state.history_loaded
            && self.input_pane.composer_is_empty()
            && self.welcome_animation_pause_ticks == 0
    }

    fn advance_animation_frame(&mut self) {
        if self.transcript_state.transcript.is_empty()
            && self.run_state.history_loaded
            && self.input_pane.composer_is_empty()
            && self.welcome_animation_pause_ticks == 0
        {
            self.welcome_animation_frame = self.welcome_animation_frame.wrapping_add(1);
        }
    }

    fn pause_welcome_animation_for_input(&mut self) {
        self.welcome_animation_pause_ticks = 8;
    }

    fn handle_animation_tick(&mut self) -> bool {
        if self.welcome_animation_pause_ticks > 0 {
            self.welcome_animation_pause_ticks -= 1;
            return false;
        }
        if self.needs_animation_frame() {
            self.advance_animation_frame();
            return true;
        }
        false
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

    pub(crate) fn handle_assistant_item_completed(&mut self, item_id: &str, output: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id)
            || self.transcript_state.active_item_kind != Some(TurnItemKind::AssistantMessage)
        {
            self.flush_active_cell_to_transcript();
            self.transcript_state.active_item_id = Some(item_id.to_string());
            self.transcript_state.active_item_kind = Some(TurnItemKind::AssistantMessage);
            self.transcript_state.active_cell = Some(HistoryCell::from_message(
                "cloudagent",
                String::new(),
                HistoryTone::Agent,
            ));
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.replace_body(output);
        }
        let has_text = self
            .transcript_state
            .active_cell
            .as_ref()
            .is_some_and(|cell| !cell.body.trim().is_empty());
        if has_text {
            self.transcript_state.last_copyable_output = self
                .transcript_state
                .active_cell
                .as_ref()
                .map(|c| c.body.clone());
            self.flush_active_cell_to_transcript();
        } else {
            self.clear_active_cell();
        }
    }

    fn handle_secondary_item_started(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
        tone: HistoryTone,
    ) {
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(kind);
        self.transcript_state.active_cell = Some(HistoryCell::from_message(
            title.to_string(),
            String::new(),
            tone,
        ));
        self.run_state.last_tool_name = Some(title.to_string());
    }

    fn handle_secondary_item_completed(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
        output: &str,
        tone: HistoryTone,
    ) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id) {
            self.flush_active_cell_to_transcript();
            self.transcript_state.active_item_id = Some(item_id.to_string());
            self.transcript_state.active_item_kind = Some(kind);
            self.transcript_state.active_cell = Some(HistoryCell::from_message(
                title.to_string(),
                String::new(),
                tone,
            ));
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.replace_body(output);
        }
        if self.transcript_state.active_item_id.as_deref() == Some(item_id) {
            self.flush_active_cell_to_transcript();
        }
        self.run_state.last_tool_name = None;
    }

    fn append_active_secondary_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id) {
            return;
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            if !cell.body.is_empty() {
                cell.append_body("\n");
            }
            cell.append_body(delta);
        }
    }

    pub(crate) fn handle_reasoning_item_started(&mut self, item_id: &str, title: &str) {
        self.handle_secondary_item_started(
            item_id,
            TurnItemKind::Reasoning,
            title,
            HistoryTone::Reasoning,
        );
    }

    pub(crate) fn handle_reasoning_item_completed(
        &mut self,
        item_id: &str,
        title: &str,
        output: &str,
    ) {
        self.handle_secondary_item_completed(
            item_id,
            TurnItemKind::Reasoning,
            title,
            output,
            HistoryTone::Reasoning,
        );
    }

    pub(crate) fn handle_reasoning_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_item_kind == Some(TurnItemKind::Reasoning) {
            self.append_active_secondary_item_delta(item_id, delta);
        }
    }

    pub(crate) fn handle_control_item_started(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
    ) {
        self.handle_secondary_item_started(item_id, kind, title, HistoryTone::Control);
    }

    pub(crate) fn handle_control_item_completed(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
        output: &str,
    ) {
        self.handle_secondary_item_completed(item_id, kind, title, output, HistoryTone::Control);
    }

    pub(crate) fn handle_control_item_delta(&mut self, item_id: &str, delta: &str) {
        if matches!(
            self.transcript_state.active_item_kind,
            Some(TurnItemKind::ToolCall)
                | Some(TurnItemKind::ToolResult)
                | Some(TurnItemKind::CommandExecution)
                | Some(TurnItemKind::FileChange)
        ) {
            self.append_active_secondary_item_delta(item_id, delta);
        }
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
            self.push_cell(cell);
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
    let conversation_id = config.conversation_id.clone();
    let mut client = create_client(&config, conversation_id.clone()).await?;
    let mut app = TuiApp::new(conversation_id.clone(), config.connection.label());
    client.send_command(AppClientCommand::RequestConversationHistory {
        conversation_id: conversation_id.clone(),
    })?;
    let mut terminal = TerminalGuard::new()?;
    let mut surface = ScrollbackSurface::new();
    let mut events = spawn_tui_event_loop();
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            if app.take_pending_history_rebuild() {
                surface.replace_all(&mut terminal, app.history_cells())?;
                app.clear_pending_history_cells();
            } else {
                surface.reflow_if_width_changed(&mut terminal, app.history_cells())?;
            }
            let pending_history_lines =
                surface.pending_lines(&terminal, app.drain_pending_history_cells())?;
            let height = ChatSurface::desired_height(&app, terminal.terminal.size()?.width).max(1);
            terminal.draw_with_history(height, pending_history_lines, |frame| app.render(frame))?;
        }
        let redraw_after_event = tokio::select! {
            Some(event) = client.next_event() => {
                app.handle_client_event(event);
                true
            }
            Some(event) = events.recv() => {
                match event {
                    UiEvent::Key(key) => {
                        app.pause_welcome_animation_for_input();
                        if let Some(input) = app.handle_key(key) {
                            if handle_tui_input(&conversation_id, &mut app, &client, input)? {
                                break;
                            }
                        }
                        true
                    }
                    UiEvent::Paste(text) => {
                        app.pause_welcome_animation_for_input();
                        let _ = app.input_pane.handle_paste(&text);
                        true
                    }
                    UiEvent::Resize => true,
                    UiEvent::Tick => {
                        app.handle_animation_tick()
                    }
                }
            }
            else => break,
        };
        needs_redraw = redraw_after_event;

        if app.run_state.should_exit {
            break;
        }
    }

    client.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::TuiApp;
    use crate::app::actions::{execute_server_action, handle_tui_input};
    use crate::app::parse::ParsedInput;
    use crate::state::reducer::{ServerAction, TurnDispatch};
    use agent_app_server_client::{AppServerClient, AppServerEvent, InProcessClientConfig};
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, CommandExecutionStatus,
        ConversationStatus, ConversationTurn, ServerRequestDecisionKind, StructuredToolResult,
        TranscriptItem, TurnItemKind,
    };
    use agent_runtime::AgentRuntime;
    use config::{AgentConfig, LlmConfig, RuntimeConfig, ToolConfig};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::OnceLock;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::time::timeout;

    fn flatten_turns(turns: Vec<ConversationTurn>) -> Vec<TranscriptItem> {
        turns
            .into_iter()
            .flat_map(|turn| turn.items.into_iter())
            .collect()
    }

    #[test]
    fn mode_changes_do_not_clear_active_approval_view() {
        let mut app = TuiApp::new("default".to_string(), "test");
        app.input_pane.set_server_request(
            crate::ui::widgets::input_pane::ServerRequestInlineState {
                request_id: agent_protocol::RequestId::String("req-1".to_string()),
                title: "Run command?".to_string(),
                detail: "shell_command".to_string(),
            },
        );

        app.set_mode(agent_protocol::FrontendMode::Running);

        assert!(app.input_pane.requires_action());
        assert_eq!(
            app.input_pane.active_server_request_id(),
            Some(agent_protocol::RequestId::String("req-1".to_string()))
        );
    }

    #[test]
    fn assistant_delta_requires_item_started_before_streaming() {
        let mut app = TuiApp::new("default".to_string(), "test");

        app.handle_assistant_item_delta("assistant:1", "partial");
        assert!(app.transcript_state.active_cell.is_none());

        app.handle_assistant_item_started("turn-1", "assistant:1");
        app.handle_assistant_item_completed("assistant:1", "complete answer");

        let cells = app.transcript_state.transcript.cells();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].body, "complete answer");
    }

    #[test]
    fn tool_delta_requires_item_started_before_streaming() {
        let mut app = TuiApp::new("default".to_string(), "test");

        app.handle_control_item_delta("tool:1", "half");
        assert!(app.transcript_state.active_cell.is_none());

        app.handle_control_item_started("tool:1", TurnItemKind::CommandExecution, "pwd");
        app.handle_control_item_completed(
            "tool:1",
            TurnItemKind::CommandExecution,
            "pwd",
            "current directory is D:\\learn\\gifti\\cloudagent",
        );

        let cells = app.transcript_state.transcript.cells();
        assert_eq!(cells.len(), 1);
        assert_eq!(
            cells[0].body,
            "current directory is D:\\learn\\gifti\\cloudagent"
        );
        assert_eq!(
            cells[0].tone,
            crate::ui::widgets::history_cell::HistoryTone::Control
        );
    }

    #[test]
    fn reasoning_and_control_cells_use_distinct_tones() {
        let mut app = TuiApp::new("default".to_string(), "test");

        app.handle_reasoning_item_started("reasoning:1", "reasoning");
        app.handle_reasoning_item_delta("reasoning:1", "thinking");
        app.handle_reasoning_item_completed("reasoning:1", "reasoning", "thinking complete");
        app.handle_control_item_started("tool:1", TurnItemKind::CommandExecution, "pwd");
        app.handle_control_item_delta("tool:1", "pwd");
        app.handle_control_item_completed(
            "tool:1",
            TurnItemKind::CommandExecution,
            "pwd",
            "D:\\learn\\gifti\\cloudagent",
        );

        let cells = app.transcript_state.transcript.cells();
        assert_eq!(cells.len(), 2);
        assert_eq!(
            cells[0].tone,
            crate::ui::widgets::history_cell::HistoryTone::Reasoning
        );
        assert_eq!(
            cells[1].tone,
            crate::ui::widgets::history_cell::HistoryTone::Control
        );
    }

    #[test]
    fn snapshot_history_replaces_transcript_without_event_replay() {
        let mut app = TuiApp::new("default".to_string(), "test");

        execute_server_action(
            &mut app,
            ServerAction::ReplaceHistory(vec![
                ConversationTurn {
                    id: "turn-old".to_string(),
                    state: agent_protocol::TurnState::Completed,
                    rollout_start_index: 0,
                    rollout_end_index: 1,
                    items: vec![
                        TranscriptItem::UserMessage {
                            id: "user:old".to_string(),
                            text: "old question".to_string(),
                        },
                        TranscriptItem::AgentMessage {
                            id: "assistant:old".to_string(),
                            text: "old answer".to_string(),
                        },
                    ],
                },
                ConversationTurn {
                    id: "turn-where".to_string(),
                    state: agent_protocol::TurnState::Completed,
                    rollout_start_index: 2,
                    rollout_end_index: 4,
                    items: vec![
                        TranscriptItem::UserMessage {
                            id: "user:where".to_string(),
                            text: "where am i".to_string(),
                        },
                        TranscriptItem::ToolResult {
                            id: "call-1".to_string(),
                            tool_name: "shell_command".to_string(),
                            content: "D:\\learn\\gifti\\cloudagent".to_string(),
                            summary: "D:\\learn\\gifti\\cloudagent".to_string(),
                            structured: Some(StructuredToolResult::CommandExecution {
                                command: "pwd".to_string(),
                                current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
                                status: CommandExecutionStatus::Completed,
                                exit_code: Some(0),
                                success: Some(true),
                                stdout: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                                stderr: Some(String::new()),
                                aggregated_output: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                                duration_ms: Some(1),
                            }),
                        },
                        TranscriptItem::AgentMessage {
                            id: "assistant:cwd".to_string(),
                            text: "current directory is D:\\learn\\gifti\\cloudagent".to_string(),
                        },
                    ],
                },
            ]),
        );

        let cells = app.transcript_state.transcript.cells();
        let bodies: Vec<&str> = cells.iter().map(|cell| cell.body.as_str()).collect();
        assert!(bodies.contains(&"old question"));
        assert!(bodies.contains(&"old answer"));
        assert!(bodies.contains(&"where am i"));
        assert!(bodies.contains(&"current directory is D:\\learn\\gifti\\cloudagent"));
    }

    #[test]
    fn turn_dispatch_completed_flushes_active_assistant_cell() {
        let mut app = TuiApp::new("default".to_string(), "test");
        app.handle_assistant_item_started("turn-1", "assistant:flush");
        app.handle_assistant_item_delta("assistant:flush", "hello");
        app.apply_turn_dispatch(TurnDispatch::Completed);

        let cells = app.transcript_state.transcript.cells();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].body, "hello");
    }

    #[tokio::test]
    async fn end_to_end_turn_roundtrips_live_and_rebuilds_after_restart() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let expected_path = fixture.workspace.display().to_string();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_1",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"pwd\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![
                json!({
                    "model": "fake-model",
                    "choices": [{
                        "delta": {
                            "content": "current directory is "
                        }
                    }]
                }),
                json!({
                    "model": "fake-model",
                    "choices": [{
                        "delta": {
                            "content": expected_path
                        }
                    }]
                }),
            ]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));

        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime.clone(),
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new("default".to_string(), "in-process");

        handle_tui_input(
            "default",
            &mut app,
            &client,
            ParsedInput::Command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "可以看到当前在哪个目录下吗".to_string(),
                },
            )),
        )
        .expect("submit turn");

        let mut saw_server_request = false;
        let mut saw_turn_completed = false;
        while !saw_turn_completed {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            let request_id = match &event {
                AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) => Some(request_id.clone()),
                _ => None,
            };
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. }
                ))
            ) {
                saw_turn_completed = true;
            }
            app.handle_client_event(event);
            if let Some(request_id) = request_id {
                saw_server_request = true;
                handle_tui_input(
                    "default",
                    &mut app,
                    &client,
                    ParsedInput::ServerRequestAnswer {
                        request_id,
                        decision: ServerRequestDecisionKind::Accept,
                        reason: "ok".to_string(),
                    },
                )
                .expect("approve request");
            }
        }
        assert!(saw_server_request, "expected a tool approval request");

        let live_cells = app.transcript_state.transcript.cells();
        assert!(
            live_cells
                .iter()
                .any(|cell| cell.body == "可以看到当前在哪个目录下吗")
        );
        assert!(live_cells.iter().any(|cell| cell.body == "approved"));
        assert!(live_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Control
                && (cell.body.contains("shell command finished with exit code 0")
                    || cell.body.contains("exit_code: 0"))
        }));
        assert!(live_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
                && cell.body.starts_with("current directory is ")
                && cell.body.ends_with("\\workspace")
        }));

        client
            .send_command(AppClientCommand::RequestConversationStatus {
                conversation_id: "default".to_string(),
            })
            .expect("request status");
        client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history");

        let mut history = None;
        let mut status_idle = false;
        while history.is_none() || !status_idle {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for history")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => history = Some(flatten_turns(turns)),
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationStatus { snapshot, .. },
                )) => {
                    status_idle = matches!(snapshot.conversation_status, ConversationStatus::Idle)
                        && snapshot.active_turn.is_none();
                }
                other => app.handle_client_event(other),
            }
        }
        client.shutdown().await.expect("shutdown client");

        let rollout_log = std::fs::read_to_string(fixture.store.join("default.rollout.jsonl"))
            .expect("read rollout log");
        assert!(
            rollout_log.contains("\"type\":\"event_msg\""),
            "rollout should persist EventMsg entries"
        );
        assert!(
            rollout_log.contains("\"type\":\"response_item\""),
            "rollout should persist ResponseItem entries"
        );

        let history = history.expect("history snapshot");
        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "可以看到当前在哪个目录下吗"
        )));
        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::CommandExecution {
                tool_name,
                command,
                ..
            } if tool_name == "shell_command" && command == "pwd"
        )));
        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::AgentMessage { text, .. }
            if text.starts_with("current directory is ") && text.ends_with("\\workspace")
        )));

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut restarted_app = TuiApp::new("default".to_string(), "in-process");
        restarted_client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history after restart");

        let mut restarted_history_loaded = false;
        while !restarted_history_loaded {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting after restart")
                .expect("client event after restart");
            match &event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { .. },
                )) => restarted_history_loaded = true,
                _ => {}
            }
            restarted_app.handle_client_event(event);
        }
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        let rebuilt_cells = restarted_app.transcript_state.transcript.cells();
        assert!(
            rebuilt_cells
                .iter()
                .any(|cell| cell.body == "可以看到当前在哪个目录下吗")
        );
        assert!(rebuilt_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Control
                && cell.body.starts_with("completed: pwd (exit 0) @ ")
                && cell.body.ends_with("\\workspace")
        }));
        assert!(rebuilt_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
                && cell.body.starts_with("current directory is ")
                && cell.body.ends_with("\\workspace")
        }));

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 2);
        assert!(recorded_requests[0].contains("\"stream\":true"));
        assert!(recorded_requests[1].contains("\"role\":\"tool\""));
        assert!(recorded_requests[1].contains("\"shell_command\""));
    }

    #[tokio::test]
    async fn interrupted_server_request_turn_rebuilds_tail_after_restart() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![sse_body(vec![json!({
            "model": "fake-model",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_interrupt",
                        "function": {
                            "name": "shell_command",
                            "arguments": "{\"command\":\"pwd\"}"
                        }
                    }]
                }
            }]
        })])];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new("default".to_string(), "in-process");

        handle_tui_input(
            "default",
            &mut app,
            &client,
            ParsedInput::Command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "帮我看看当前目录".to_string(),
                },
            )),
        )
        .expect("submit turn");

        let mut saw_server_request = false;
        let mut saw_server_request_cancelled = false;
        let mut saw_turn_cancelled = false;
        while !saw_turn_cancelled {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Request(_))
            ) {
                saw_server_request = true;
                let input = app
                    .handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL))
                    .expect("ctrl+k should produce interrupt input");
                handle_tui_input("default", &mut app, &client, input)
                    .expect("ctrl+k interrupt turn");
            }
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ServerRequestResolved {
                        decision,
                        ..
                    }
                )) if decision.decision == agent_protocol::ServerRequestDecisionKind::Cancel
            ) {
                saw_server_request_cancelled = true;
            }
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCancelled { .. }
                ))
            ) {
                saw_turn_cancelled = true;
            }
            app.handle_client_event(event);
        }
        assert!(
            saw_server_request,
            "expected pending server request before interrupt"
        );
        assert!(
            saw_server_request_cancelled,
            "expected interrupt to cancel the pending server request"
        );

        client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history");

        let history = loop {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for history")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => break flatten_turns(turns),
                other => app.handle_client_event(other),
            }
        };
        client.shutdown().await.expect("shutdown client");

        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "帮我看看当前目录"
        )));

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut restarted_app = TuiApp::new("default".to_string(), "in-process");
        restarted_client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history after restart");

        let mut restarted_history_loaded = false;
        while !restarted_history_loaded {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting after restart")
                .expect("client event after restart");
            match &event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { .. },
                )) => restarted_history_loaded = true,
                _ => {}
            }
            restarted_app.handle_client_event(event);
        }
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        let rebuilt_cells = restarted_app.transcript_state.transcript.cells();
        let debug_cells = rebuilt_cells
            .iter()
            .map(|cell| (cell.label.as_str(), cell.body.as_str()))
            .collect::<Vec<_>>();
        assert!(
            rebuilt_cells
                .iter()
                .any(|cell| cell.body == "帮我看看当前目录")
        );
        assert_eq!(
            rebuilt_cells
                .iter()
                .filter(|cell| cell.body == "帮我看看当前目录")
                .count(),
            1
        );
        assert!(
            rebuilt_cells
                .iter()
                .any(|cell| cell.label == "tool" && cell.body.contains("failed: pwd")),
            "rebuilt cells: {debug_cells:?}"
        );
        assert!(!rebuilt_cells.iter().any(|cell| cell.label == "request"));

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 1);
    }

    #[tokio::test]
    async fn consecutive_tool_turns_preserve_history_across_restart() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_one",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"pwd\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": "current directory is " } }]
                }),
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": fixture.workspace.display().to_string() } }]
                }),
            ]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_two",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"pwd\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": "again current directory is " } }]
                }),
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": fixture.workspace.display().to_string() } }]
                }),
            ]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new("default".to_string(), "in-process");

        for content in ["第一轮看看目录", "第二轮再看一次目录"] {
            client
                .send_command(AppClientCommand::SubmitTurn(
                    agent_protocol::UserTurnInput {
                        conversation_id: "default".to_string(),
                        content: content.to_string(),
                    },
                ))
                .expect("submit turn");

            let mut saw_server_request = false;
            let mut saw_turn_completed = false;
            let mut saw_idle = false;
            while !saw_turn_completed || !saw_idle {
                let event = timeout(Duration::from_secs(10), client.next_event())
                    .await
                    .expect("timed out waiting for client event")
                    .expect("client event");
                if let AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) = &event
                {
                    saw_server_request = true;
                    client
                        .send_command(AppClientCommand::ResolveServerRequest {
                            conversation_id: "default".to_string(),
                            request_id: request_id.clone(),
                            decision: agent_protocol::ServerRequestDecision::accept(Some(
                                "ok".to_string(),
                            )),
                        })
                        .expect("approve request");
                }
                if matches!(
                    &event,
                    AppServerEvent::Message(AppServerMessage::Notification(
                        AppServerNotification::TurnCompleted { .. }
                    ))
                ) {
                    saw_turn_completed = true;
                }
                if matches!(
                    &event,
                    AppServerEvent::Message(AppServerMessage::Notification(
                        AppServerNotification::FrontendStateChanged {
                            mode: agent_protocol::FrontendMode::Idle,
                            ..
                        }
                    ))
                ) {
                    saw_idle = true;
                }
                app.handle_client_event(event);
            }
            assert!(saw_server_request, "expected server request for tool turn");
        }

        client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request live history");
        let live_history = loop {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for history")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => break flatten_turns(turns),
                other => app.handle_client_event(other),
            }
        };
        client.shutdown().await.expect("shutdown client");

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        restarted_client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history after restart");
        let restarted_history = loop {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting after restart")
                .expect("client event after restart");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => break flatten_turns(turns),
                _ => {}
            }
        };
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        assert_eq!(restarted_history.len(), live_history.len());
        assert!(restarted_history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "第一轮看看目录"
        )));
        assert!(restarted_history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "第二轮再看一次目录"
        )));
        assert!(restarted_history.iter().filter(|entry| matches!(
            entry,
            TranscriptItem::AgentMessage { text, .. } if text.starts_with("current directory is ")
        )).count() >= 1);
        assert!(restarted_history.iter().filter(|entry| matches!(
            entry,
            TranscriptItem::AgentMessage { text, .. } if text.starts_with("again current directory is ")
        )).count() >= 1);

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 4);
    }

    #[tokio::test]
    async fn denied_tool_in_multi_tool_batch_still_records_all_tool_results() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_denied",
                                "function": {
                                    "name": "shell_command",
                                    "arguments": "{\"command\":\"pwd\"}"
                                }
                            },
                            {
                                "index": 1,
                                "id": "call_allowed",
                                "function": {
                                    "name": "shell_command",
                                    "arguments": "{\"command\":\"pwd\"}"
                                }
                            }
                        ]
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{ "delta": { "content": "done" } }]
            })]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });

        client
            .send_command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "run two commands".to_string(),
                },
            ))
            .expect("submit turn");

        let mut request_count = 0usize;
        let mut saw_completed = false;
        while !saw_completed {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) => {
                    request_count += 1;
                    let decision = if request_count == 1 {
                        agent_protocol::ServerRequestDecision::decline(Some(
                            "skip first".to_string(),
                        ))
                    } else {
                        agent_protocol::ServerRequestDecision::accept(Some("ok".to_string()))
                    };
                    client
                        .send_command(AppClientCommand::ResolveServerRequest {
                            conversation_id: "default".to_string(),
                            request_id,
                            decision,
                        })
                        .expect("resolve request");
                }
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. },
                )) => {
                    saw_completed = true;
                }
                _ => {}
            }
        }
        client.shutdown().await.expect("shutdown client");

        assert_eq!(request_count, 2);
        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 2);
        assert!(recorded_requests[1].contains("\"tool_call_id\":\"call_denied\""));
        assert!(recorded_requests[1].contains("\"tool_call_id\":\"call_allowed\""));
    }

    #[tokio::test]
    async fn repeated_denied_tool_request_does_not_prompt_again() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_denied_once",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"df -h\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_denied_repeat",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"df -h\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{ "delta": { "content": "I cannot inspect disk usage because permission was denied." } }]
            })]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });

        client
            .send_command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "check disk".to_string(),
                },
            ))
            .expect("submit turn");

        let mut request_count = 0usize;
        let mut saw_completed = false;
        while !saw_completed {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) => {
                    request_count += 1;
                    client
                        .send_command(AppClientCommand::ResolveServerRequest {
                            conversation_id: "default".to_string(),
                            request_id,
                            decision: agent_protocol::ServerRequestDecision::decline(Some(
                                String::new(),
                            )),
                        })
                        .expect("deny request");
                }
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. },
                )) => {
                    saw_completed = true;
                }
                _ => {}
            }
        }
        client.shutdown().await.expect("shutdown client");

        assert_eq!(request_count, 1);
        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 3);
        assert!(recorded_requests[1].contains("\"tool_call_id\":\"call_denied_once\""));
        assert!(recorded_requests[1].contains("exec command rejected by user"));
        assert!(recorded_requests[2].contains("\"tool_call_id\":\"call_denied_repeat\""));
        assert!(recorded_requests[2].contains("exec command rejected by user"));
        assert!(recorded_requests[2].contains("same tool request was already denied in this turn"));
    }

    fn test_config(
        workspace_root: PathBuf,
        conversation_store_dir: PathBuf,
        base_url: String,
    ) -> AgentConfig {
        AgentConfig {
            workspace_root,
            llm: LlmConfig {
                base_url,
                api_key: "test-key".to_string(),
                model: "fake-model".to_string(),
                temperature: 0.0,
            },
            runtime: RuntimeConfig {
                default_conversation_id: "default".to_string(),
                system_prompt: "You are a test agent.".to_string(),
                max_tool_roundtrips: 4,
                conversation_store_dir,
                model_context_window: 128_000,
                context_compaction_trigger_ratio: 0.85,
                context_compaction_target_tokens: 36_000,
                context_compaction_request_overhead_tokens: 28_000,
                context_compaction_preserved_user_turns: 3,
                context_compaction_preserved_tail_tokens: 12_000,
                context_compaction_summary_source_tokens: 24_000,
            },
            tools: ToolConfig {
                default_shell_timeout_ms: 5_000,
                max_read_chars: 8_192,
            },
        }
    }

    fn sse_body(chunks: Vec<serde_json::Value>) -> String {
        let mut body = String::new();
        for chunk in chunks {
            body.push_str("data: ");
            body.push_str(&serde_json::to_string(&chunk).expect("sse chunk"));
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    fn spawn_fake_llm_server(
        responses: Vec<String>,
    ) -> (String, thread::JoinHandle<std::io::Result<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake llm server");
        let base_url = format!("http://{}", listener.local_addr().expect("listener addr"));
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let request_body = read_http_request_body(&mut stream)?;
                requests.push(request_body);
                let http_response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    response.len(),
                    response
                );
                stream.write_all(http_response.as_bytes())?;
                stream.flush()?;
            }
            Ok(requests)
        });
        (base_url, handle)
    }

    fn read_http_request_body(stream: &mut TcpStream) -> std::io::Result<String> {
        let mut buffer = Vec::new();
        let mut scratch = [0u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut scratch)?;
            if read == 0 {
                return Ok(String::new());
            }
            buffer.extend_from_slice(&scratch[..read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut scratch)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&scratch[..read]);
        }
        body.truncate(content_length);
        Ok(String::from_utf8_lossy(&body).to_string())
    }

    struct TempFixture {
        root: PathBuf,
        workspace: PathBuf,
        store: PathBuf,
    }

    impl TempFixture {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("cloudagent-cli-test-{unique}"));
            let workspace = root.join("workspace");
            let store = root.join("conversations");
            std::fs::create_dir_all(&workspace).expect("create workspace");
            std::fs::create_dir_all(&store).expect("create conversation store");
            Self {
                root,
                workspace,
                store,
            }
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn cli_e2e_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }
}
