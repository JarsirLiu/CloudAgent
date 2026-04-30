pub mod actions;
pub mod effects;
mod parse;

use crate::app::actions::{execute_server_action, handle_tui_input};
use crate::app::parse::{ParsedInput, parse_line};
use crate::state::reducer::{TurnDispatch, apply_server_message};
use crate::state::{ConsoleState, RunState, ServerRequestState, TranscriptState};
use crate::terminal::{TerminalGuard, UiEvent, spawn_tui_event_loop};
use crate::transport::client::create_client;
use crate::ui::screen::render_app;
use crate::ui::widgets::chat_composer::ComposerAction;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use crate::ui::widgets::input_pane::{InputPane, InputPaneAction};
use agent_app_server_client::AppServerEvent;
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
        }
    }

    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        self.preserve_scroll_on_content_change(|this| {
            this.transcript_state.transcript.push(cell);
        });
    }

    pub(crate) fn reset_local_view(&mut self) {
        self.console_state = ConsoleState::new();
        self.server_request_state = ServerRequestState::default();
        self.transcript_state = TranscriptState::default();
        self.run_state = RunState::new(&self.connection_label);
        self.run_state.history_loaded = true;
        self.input_pane.clear_views();
    }

    pub(crate) fn set_mode(&mut self, mode: FrontendMode) {
        self.console_state.mode = mode;
        if mode != FrontendMode::WaitingForServerRequest {
            self.input_pane.clear_server_request();
        }
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
                    self.transcript_state.scroll =
                        self.transcript_state.scroll.saturating_sub(page_step);
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
            InputPaneAction::Composer(ComposerAction::Submit(text)) => Some(parse_line(
                &text,
                &self.conversation_id,
                self.console_state.mode,
            )),
            InputPaneAction::Composer(ComposerAction::Interrupt) => {
                Some(ParsedInput::Command(AppClientCommand::InterruptTurn {
                    conversation_id: self.conversation_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerAction::Exit) => {
                self.run_state.should_exit = true;
                Some(ParsedInput::Command(AppClientCommand::Exit))
            }
            InputPaneAction::Composer(ComposerAction::Reset) => {
                Some(ParsedInput::Command(AppClientCommand::ResetConversation {
                    conversation_id: self.conversation_id.clone(),
                }))
            }
            InputPaneAction::Composer(ComposerAction::None) => None,
            InputPaneAction::ServerRequestSubmit { approved, reason } => {
                Some(ParsedInput::ServerRequestAnswer { approved, reason })
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
        self.transcript_state.transcript.total_lines_with_tail(
            self.transcript_state.viewport_width.max(20),
            self.transcript_state.active_cell.as_ref(),
        )
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
            self.transcript_state.scroll =
                self.transcript_state.scroll.saturating_add(appended_lines);
        } else {
            self.transcript_state.scroll = 0;
        }
        self.clamp_transcript_scroll();
    }

    fn page_scroll_step(&self) -> usize {
        self.transcript_state
            .viewport_height
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
            self.run_state.last_message_count = self.run_state.last_message_count.saturating_add(1);
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
    let conversation_id = config.conversation_id.clone();
    let mut client = create_client(&config, conversation_id.clone()).await?;
    let mut app = TuiApp::new(conversation_id.clone(), config.connection.label());
    client.send_command(AppClientCommand::RequestConversationHistory {
        conversation_id: conversation_id.clone(),
    })?;
    let mut terminal = TerminalGuard::new()?;
    let mut events = spawn_tui_event_loop();

    loop {
        terminal.terminal.draw(|frame| app.render(frame))?;

        tokio::select! {
            Some(event) = client.next_event() => {
                app.handle_client_event(event);
            }
            Some(event) = events.recv() => {
                match event {
                    UiEvent::Key(key) => {
                        if let Some(input) = app.handle_key(key) {
                            if handle_tui_input(&conversation_id, &mut app, &client, input)? {
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

#[cfg(test)]
mod tests {
    use super::TuiApp;
    use crate::app::actions::{execute_server_action, handle_tui_input};
    use crate::app::parse::ParsedInput;
    use crate::state::reducer::{ServerAction, TurnDispatch};
    use agent_app_server_client::{AppServerClient, AppServerEvent, InProcessClientConfig};
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, CommandExecutionStatus,
        ConversationStatus, ConversationTurn, StructuredToolResult, TranscriptItem, TurnItemKind,
    };
    use agent_runtime::AgentRuntime;
    use config::{AgentConfig, LlmConfig, RuntimeConfig, ToolConfig};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::Arc;
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
            let is_request = matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Request(_))
            );
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. }
                ))
            ) {
                saw_turn_completed = true;
            }
            app.handle_client_event(event);
            if is_request {
                saw_server_request = true;
                handle_tui_input(
                    "default",
                    &mut app,
                    &client,
                    ParsedInput::ServerRequestAnswer {
                        approved: true,
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
                && cell.body.starts_with("current directory is ")
                && cell.body.ends_with("\\workspace")
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
                client
                    .send_command(AppClientCommand::InterruptTurn {
                        conversation_id: "default".to_string(),
                    })
                    .expect("interrupt turn");
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
        assert!(!rebuilt_cells.iter().any(|cell| cell.label == "request"));

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 1);
    }

    #[tokio::test]
    async fn consecutive_tool_turns_preserve_history_across_restart() {
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
                            approved: true,
                            reason: Some("ok".to_string()),
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
}
