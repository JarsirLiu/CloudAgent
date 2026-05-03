use crate::app::commands::parse::{ParsedInput, parse_line};
use crate::app::TuiApp;
use crate::input::intent::ComposerIntent;
use crate::ui::widgets::history_cell::HistoryTone;
use crate::ui::widgets::input_pane::InputPaneAction;
use agent_protocol::{AppClientCommand, FrontendMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl TuiApp {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<ParsedInput> {
        if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.run_state.expand_tool_details = !self.run_state.expand_tool_details;
            self.transcript_state
                .transcript
                .set_tool_cells_expanded(self.run_state.expand_tool_details);
            if let Some(cell) = self.transcript_state.active_cell.as_mut()
                && matches!(
                    cell.tone,
                    HistoryTone::Tool | HistoryTone::Control | HistoryTone::Warning | HistoryTone::Error
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
}
