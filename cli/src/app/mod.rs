pub mod actions;
mod app_lifecycle;
mod conversation_facade;
mod event_router;
pub mod effects;
mod filter_toggle;
mod items;
mod parse;
mod runtime_loop;
mod runtime_updates;

use crate::app::filter_toggle::load_filter_enabled;
use crate::app::parse::{ParsedInput, parse_line};
use crate::input::intent::ComposerIntent;
use crate::state::reducer::TurnDispatch;
use crate::state::runtime_projection::RuntimeProjection;
use crate::state::{ConsoleState, RunState, ServerRequestState, TranscriptState};
use crate::terminal::Frame;
use crate::transport::client::create_client;
use crate::ui::chat_surface::ChatSurface;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use crate::ui::widgets::input_pane::{InputPane, InputPaneAction};
use agent_protocol::{AppClientCommand, ConversationSummary, FrontendMode};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, IsTerminal as _};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct ConsoleConfig {
    pub conversation_id: String,
    pub workspace_root: PathBuf,
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
    pub(crate) conversation_summaries: Vec<ConversationSummary>,
    pub(crate) connection_label: String,
    pub(crate) console_state: ConsoleState,
    pub(crate) server_request_state: ServerRequestState,
    pub(crate) transcript_state: TranscriptState,
    pub(crate) run_state: RunState,
    pub(crate) runtime_projection: RuntimeProjection,
    pub(crate) input_pane: InputPane,
    pub(crate) welcome_animation_frame: u64,
    welcome_animation_pause_ticks: u8,
    pending_history_cells: VecDeque<HistoryCell>,
    pending_history_rebuild: bool,
    pub(crate) session_picker_requested: bool,
    pub(crate) workspace_root: PathBuf,
}

impl TuiApp {
    fn new(conversation_id: String, connection_label: &str, workspace_root: PathBuf) -> Self {
        Self {
            conversation_id,
            conversation_summaries: Vec::new(),
            connection_label: connection_label.to_string(),
            console_state: ConsoleState::new(),
            server_request_state: ServerRequestState::default(),
            transcript_state: TranscriptState::default(),
            run_state: RunState::new(connection_label),
            runtime_projection: RuntimeProjection::new(),
            input_pane: InputPane::new(),
            welcome_animation_frame: 0,
            welcome_animation_pause_ticks: 0,
            pending_history_cells: VecDeque::new(),
            pending_history_rebuild: false,
            session_picker_requested: false,
            workspace_root,
        }
    }

    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        self.transcript_state.transcript.push(cell.clone());
        self.pending_history_cells.push_back(cell);
    }

    pub(crate) fn replace_history_cells(&mut self, cells: Vec<HistoryCell>) {
        let mut cells = cells;
        for cell in &mut cells {
            if matches!(
                cell.tone,
                HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.expanded = self.run_state.expand_tool_details;
            }
        }
        self.transcript_state
            .transcript
            .replace_cells(cells.clone());
        self.transcript_state
            .transcript
            .set_tool_cells_expanded(self.run_state.expand_tool_details);
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
        self.runtime_projection = RuntimeProjection::new();
        self.run_state.history_loaded = true;
        self.input_pane.clear_views();
        self.welcome_animation_frame = 0;
        self.welcome_animation_pause_ticks = 0;
        self.pending_history_cells.clear();
        self.pending_history_rebuild = false;
    }

    pub(crate) fn switch_conversation(&mut self, conversation_id: String) {
        self.conversation_id = conversation_id;
        self.reset_local_view();
    }

    pub(crate) fn set_conversation_summaries(
        &mut self,
        conversation_summaries: Vec<ConversationSummary>,
    ) {
        self.conversation_summaries = conversation_summaries;
    }

    pub(crate) fn set_mode(&mut self, mode: FrontendMode) {
        self.console_state.mode = mode;
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
        if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.run_state.expand_tool_details = !self.run_state.expand_tool_details;
            self.transcript_state
                .transcript
                .set_tool_cells_expanded(self.run_state.expand_tool_details);
            if let Some(cell) = self.transcript_state.active_cell.as_mut()
                && matches!(
                    cell.tone,
                    HistoryTone::Tool
                        | HistoryTone::Control
                        | HistoryTone::Warning
                        | HistoryTone::Error
                )
            {
                cell.expanded = self.run_state.expand_tool_details;
            }
            self.run_state.set_system_notice(
                if self.run_state.expand_tool_details {
                    "Tool details expanded"
                } else {
                    "Tool details collapsed"
                },
                Some(std::time::Duration::from_secs(4)),
            );
            return None;
        }
        match self.input_pane.handle_key(key)? {
            InputPaneAction::Composer(ComposerIntent::Submit(text)) => Some(parse_line(
                &text,
                &self.conversation_id,
                self.console_state.mode,
            )),
            InputPaneAction::Composer(ComposerIntent::Interrupt) => {
                if self.console_state.mode == FrontendMode::Idle {
                    self.run_state.should_exit = true;
                    Some(ParsedInput::Command(AppClientCommand::Exit))
                } else {
                    Some(ParsedInput::Command(AppClientCommand::InterruptTurn {
                        conversation_id: self.conversation_id.clone(),
                    }))
                }
            }
            InputPaneAction::Composer(ComposerIntent::Compact) => Some(ParsedInput::Command(
                AppClientCommand::CompactConversation {
                    conversation_id: self.conversation_id.clone(),
                },
            )),
            InputPaneAction::Composer(ComposerIntent::Session) => {
                self.session_picker_requested = true;
                Some(ParsedInput::Command(AppClientCommand::ListConversations))
            }
            InputPaneAction::Composer(ComposerIntent::NewConversation(conversation_id)) => {
                Some(ParsedInput::LocalConversationCreate(conversation_id))
            }
            InputPaneAction::Composer(ComposerIntent::SessionSwitch(conversation_id)) => {
                Some(ParsedInput::LocalConversationSwitch(conversation_id))
            }
            InputPaneAction::Composer(ComposerIntent::SetTitle(title)) => {
                Some(ParsedInput::LocalConversationTitle(title))
            }
            InputPaneAction::Composer(ComposerIntent::ArchiveConversation(conversation_id)) => {
                Some(ParsedInput::LocalConversationArchive(conversation_id))
            }
            InputPaneAction::Composer(ComposerIntent::Filter(args)) => {
                Some(ParsedInput::LocalFilterToggle(args))
            }
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
    let mut app = TuiApp::new(
        conversation_id.clone(),
        config.connection.label(),
        config.workspace_root.clone(),
    );
    app.run_state.pre_llm_filter_enabled = load_filter_enabled(&app.workspace_root);
    client.send_command(AppClientCommand::RequestConversationHistory {
        conversation_id: conversation_id.clone(),
    })?;
    runtime_loop::run_tui_event_loop(&mut app, &mut client).await?;
    client.shutdown().await
}

#[cfg(test)]
mod tests;
