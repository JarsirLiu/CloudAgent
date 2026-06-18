use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permission_profile::turn_policy_for_mode;
use crate::input::intent::ComposerIntent;
use crate::input::keymap::matches_ctrl_char;
use crate::input::keymap::matches_image_paste_shortcut;
use crate::state::NoticeLevel;
use crate::ui::widgets::input_pane::InputPaneAction;
use agent_protocol::AppClientCommand;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl TuiApp {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<ParsedInput> {
        if self.should_route_key_to_transcript_scroll(key) && self.transcript_scroll.handle_key(key)
        {
            return None;
        }

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
                && self.bottom_pane.no_modal_or_popup_active()
                && self.bottom_pane.composer_is_empty()
            {
                self.run_state.should_exit = true;
                return Some(ParsedInput::Command(AppClientCommand::Exit));
            }
            return None;
        }

        if matches_ctrl_char(key, 't') && self.bottom_pane.no_modal_or_popup_active() {
            self.run_state.expand_tool_details = !self.run_state.expand_tool_details;
            self.transcript_owner
                .set_expand_details(self.run_state.expand_tool_details);
            self.bottom_pane.show_transient_notice(
                NoticeLevel::Info,
                if self.run_state.expand_tool_details {
                    "Tool details expanded".to_string()
                } else {
                    "Tool details collapsed".to_string()
                },
            );
            return None;
        }
        if matches_image_paste_shortcut(key)
            && self.bottom_pane.no_modal_or_popup_active()
            && self.bottom_pane.should_capture_global_paste_shortcut()
        {
            return Some(ParsedInput::LocalImagePaste);
        }
        match self.bottom_pane.handle_key(key)? {
            InputPaneAction::LoadMoreSessions { cursor } => {
                Some(ParsedInput::LocalSessionListNextPage { cursor })
            }
            InputPaneAction::Composer(ComposerIntent::Submit(content)) => {
                Some(ParsedInput::Command(AppClientCommand::SubmitTurn(
                    agent_protocol::UserTurnInput {
                        conversation_id: self.conversation_id.clone(),
                        content,
                        turn_policy: turn_policy_for_mode(&self.run_state.permission_mode),
                    },
                )))
            }
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
                self.bottom_pane.request_session_picker(
                    crate::ui::widgets::session_picker::SessionPickerMode::Switch,
                );
                Some(ParsedInput::Command(
                    AppClientCommand::ListConversationsPage {
                        cursor: None,
                        limit: 25,
                    },
                ))
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
            InputPaneAction::Composer(ComposerIntent::Skill(name)) => {
                Some(ParsedInput::LocalSkillInsert(name))
            }
            InputPaneAction::Composer(ComposerIntent::Skills) => Some(ParsedInput::LocalSkillsOpen),
            InputPaneAction::Composer(ComposerIntent::Gateway) => {
                Some(ParsedInput::LocalGatewayOpen)
            }
            InputPaneAction::Composer(ComposerIntent::GatewaySelect(platform)) => {
                Some(ParsedInput::LocalGatewaySelect(platform))
            }
            InputPaneAction::Composer(ComposerIntent::GatewayWeixinLoginStart { platform }) => {
                Some(ParsedInput::LocalGatewayWeixinLoginStart(platform))
            }
            InputPaneAction::Composer(ComposerIntent::GatewayWeixinLoginCheck {
                platform,
                session_id,
                qr_url,
            }) => Some(ParsedInput::LocalGatewayWeixinLoginCheck {
                platform,
                session_id,
                qr_url,
            }),
            InputPaneAction::Composer(ComposerIntent::GatewaySave {
                platform,
                enabled,
                updates,
            }) => Some(ParsedInput::LocalGatewaySave {
                platform,
                enabled,
                updates,
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
            InputPaneAction::Composer(ComposerIntent::Reasoning(effort)) => {
                Some(ParsedInput::LocalReasoning(effort))
            }
            InputPaneAction::Composer(ComposerIntent::Model(model)) => {
                Some(ParsedInput::LocalModel(model))
            }
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

    fn should_route_key_to_transcript_scroll(&self, key: KeyEvent) -> bool {
        if !self.transcript_owner.has_transcript_content() {
            return false;
        }
        if self.current_mode() != agent_protocol::FrontendMode::Idle {
            return self.bottom_pane.no_modal_or_popup_active();
        }
        matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
            || (self.bottom_pane.composer_is_empty()
                && matches!(key.code, KeyCode::Up | KeyCode::Down))
    }
}

#[cfg(test)]
mod tests {
    use crate::app::TuiApp;
    use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn test_app() -> TuiApp {
        TuiApp::new(
            "default".to_string(),
            "test",
            PathBuf::from("D:\\learn\\gifti\\cloudagent"),
            PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
            false,
            "WorkspaceWrite".to_string(),
        )
    }

    #[test]
    fn idle_transcript_scroll_captures_arrow_keys_only_when_composer_empty() {
        let mut app = test_app();
        app.push_live_cell(HistoryCell::agent(
            "assistant",
            "visible transcript",
            HistoryFormat::Markdown,
        ));

        assert!(
            app.should_route_key_to_transcript_scroll(KeyEvent::new(
                KeyCode::Up,
                KeyModifiers::NONE,
            ))
        );
        app.bottom_pane
            .restore_submission(&agent_core::text_input_items("x"));
        assert!(
            !app.should_route_key_to_transcript_scroll(KeyEvent::new(
                KeyCode::Up,
                KeyModifiers::NONE,
            ))
        );
        assert!(app.should_route_key_to_transcript_scroll(KeyEvent::new(
            KeyCode::PageUp,
            KeyModifiers::NONE,
        )));
        assert!(!app.should_route_key_to_transcript_scroll(KeyEvent::new(
            KeyCode::Home,
            KeyModifiers::NONE,
        )));
        assert!(!app.should_route_key_to_transcript_scroll(KeyEvent::new(
            KeyCode::End,
            KeyModifiers::NONE,
        )));
    }

    #[test]
    fn running_transcript_scroll_can_capture_arrow_keys() {
        let mut app = test_app();
        app.push_live_cell(HistoryCell::agent(
            "assistant",
            "visible transcript",
            HistoryFormat::Markdown,
        ));
        app.apply_conversation_view_snapshot(
            crate::app::core::conversation_state::conversation_view_snapshot_for_test(
                &app.conversation_id,
                agent_protocol::FrontendMode::Running,
            ),
        );

        assert!(
            app.should_route_key_to_transcript_scroll(KeyEvent::new(
                KeyCode::Up,
                KeyModifiers::NONE,
            ))
        );
    }

    #[test]
    fn ctrl_d_does_not_exit_while_turn_is_running() {
        let mut app = test_app();
        app.apply_conversation_view_snapshot(
            crate::app::core::conversation_state::conversation_view_snapshot_for_test(
                &app.conversation_id,
                agent_protocol::FrontendMode::Running,
            ),
        );

        let parsed = app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));

        assert!(parsed.is_none());
        assert!(!app.run_state.should_exit);
    }
}
