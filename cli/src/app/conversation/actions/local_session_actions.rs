use crate::app::conversation::actions::show_local_notice;
use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use agent_app_server_client::AppServerClient;
use anyhow::Result;

pub(crate) async fn handle_session_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalConversationCreate(new_conversation_id) => {
            app.bottom_pane.clear_session_picker();
            let trimmed = new_conversation_id.trim();
            let conversation_id = if trimmed.is_empty() {
                agent_core::host::timestamp_conversation_id()
            } else {
                trimmed.to_string()
            };
            client.send_command(agent_protocol::AppClientCommand::SwitchConversation {
                conversation_id: conversation_id.clone(),
            })?;
            Ok(false)
        }
        ParsedInput::LocalSessionListNextPage { cursor } => {
            client.send_command(agent_protocol::AppClientCommand::ListConversationsPage {
                cursor: Some(cursor),
                limit: 25,
            })?;
            Ok(false)
        }
        ParsedInput::LocalConversationSwitch(target_conversation_id) => {
            app.bottom_pane.clear_session_picker();
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                show_local_notice(app, crate::state::NoticeLevel::Warn, "Usage: /session <session-id>");
                return Ok(false);
            }
            client.send_command(agent_protocol::AppClientCommand::SwitchConversation {
                conversation_id: trimmed.to_string(),
            })?;
            Ok(false)
        }
        ParsedInput::LocalConversationTitle(title) => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                show_local_notice(app, crate::state::NoticeLevel::Warn, "Usage: /title <text>");
                return Ok(false);
            }
            client.send_command(agent_protocol::AppClientCommand::SetConversationTitle {
                conversation_id: app.conversation_id.clone(),
                title: trimmed.to_string(),
            })?;
            Ok(false)
        }
        ParsedInput::LocalConversationArchive(target_conversation_id) => {
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                show_local_notice(app, crate::state::NoticeLevel::Warn, "Usage: /archive <session-id>");
                return Ok(false);
            }
            client.send_command(agent_protocol::AppClientCommand::ArchiveConversation {
                conversation_id: trimmed.to_string(),
            })?;
            Ok(false)
        }
        ParsedInput::LocalConversationDelete(target_conversation_id) => {
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                app.bottom_pane.request_session_picker(
                    crate::ui::widgets::session_picker::SessionPickerMode::Delete,
                );
                client.send_command(agent_protocol::AppClientCommand::ListConversationsPage {
                    cursor: None,
                    limit: 25,
                })?;
                return Ok(false);
            }
            client.send_command(agent_protocol::AppClientCommand::DeleteConversation {
                conversation_id: trimmed.to_string(),
            })?;
            Ok(false)
        }
        _ => unreachable!("session input dispatcher received non-session input"),
    }
}
