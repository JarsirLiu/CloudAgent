use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permission_profile::turn_policy_for_mode;
use crate::input::intent::ComposerIntent;
use crate::ui::widgets::input_pane::InputPaneAction;
use agent_protocol::AppClientCommand;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl TuiApp {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<ParsedInput> {
        if matches_ctrl_char(key, 'c') {
            if self.current_mode() == agent_protocol::FrontendMode::Idle
                && self.bottom_pane.composer_has_selection()
            {
                return Some(ParsedInput::LocalCopyText(
                    self.bottom_pane
                        .handle_key(KeyEvent::new(
                            KeyCode::Char('C'),
                            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                        ))
                        .and_then(|action| match action {
                            InputPaneAction::Composer(ComposerIntent::CopyText(text)) => Some(text),
                            _ => None,
                        })?,
                ));
            }
            return None;
        }

        if matches_ctrl_char(key, 'd') {
            if self.current_mode() == agent_protocol::FrontendMode::Idle
                && self.bottom_pane.composer_is_empty()
            {
                self.run_state.should_exit = true;
                return Some(ParsedInput::Command(AppClientCommand::Exit));
            }
            return None;
        }

        if matches_ctrl_char(key, 't') {
            self.run_state.expand_tool_details = !self.run_state.expand_tool_details;
            self.transcript_owner
                .set_expand_details(self.run_state.expand_tool_details);
            self.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::info(
                "conversation",
                if self.run_state.expand_tool_details {
                    "Tool details expanded"
                } else {
                    "Tool details collapsed"
                },
                crate::ui::widgets::history_cell::HistoryTone::Control,
            ));
            return None;
        }
        match self.bottom_pane.handle_key(key)? {
            InputPaneAction::Composer(ComposerIntent::Submit(text)) => Some(ParsedInput::Command(
                AppClientCommand::SubmitTurn(agent_protocol::UserTurnInput {
                    conversation_id: self.conversation_id.clone(),
                    content: text,
                    turn_policy: turn_policy_for_mode(&self.run_state.permission_mode),
                }),
            )),
            InputPaneAction::Composer(ComposerIntent::Interrupt) => {
                if self.current_mode() == agent_protocol::FrontendMode::Idle {
                    None
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
                self.bottom_pane
                    .request_session_picker(crate::ui::widgets::session_picker::SessionPickerMode::Switch);
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
            InputPaneAction::Composer(ComposerIntent::DeleteConversation(conversation_id)) => {
                Some(ParsedInput::LocalConversationDelete(conversation_id))
            }
            InputPaneAction::Composer(ComposerIntent::Filter(args)) => {
                Some(ParsedInput::LocalFilterToggle(args))
            }
            InputPaneAction::Composer(ComposerIntent::Permissions(mode)) => {
                Some(ParsedInput::LocalPermissionMode(mode))
            }
            InputPaneAction::Composer(ComposerIntent::Config) => Some(ParsedInput::LocalConfig {
                api_key: String::new(),
                base_url: String::new(),
                model: String::new(),
            }),
            InputPaneAction::Composer(ComposerIntent::ConfigSave {
                api_key,
                base_url,
                model,
            }) => Some(ParsedInput::LocalConfig {
                api_key,
                base_url,
                model,
            }),
            InputPaneAction::Composer(ComposerIntent::Copy) => Some(ParsedInput::LocalCopy),
            InputPaneAction::Composer(ComposerIntent::CopyText(text)) => {
                Some(ParsedInput::LocalCopyText(text))
            }
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

fn matches_ctrl_char(key: KeyEvent, ch: char) -> bool {
    matches!(key.code, KeyCode::Char(code) if code.eq_ignore_ascii_case(&ch))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}
