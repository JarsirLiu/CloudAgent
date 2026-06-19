use crate::app::conversation::actions::decision_label;
use crate::app::conversation::actions::show_local_notice;
use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use agent_app_server_client::AppServerClient;
use agent_core::ServerRequestDecision;
use agent_protocol::{AppClientCommand, UserTurnInput};
use anyhow::Result;

pub(crate) async fn handle_command_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::Command(command) => {
            if let AppClientCommand::Exit = command {
                if app.current_mode() != agent_protocol::FrontendMode::Idle {
                    client.send_command(AppClientCommand::InterruptTurn {
                        conversation_id: app.conversation_id.clone(),
                    })?;
                }
                app.run_state.should_exit = true;
                return Ok(true);
            }

            if matches!(command, AppClientCommand::SubmitTurn(_)) && !app.can_submit_turn() {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Warn,
                    "turn already running; wait, answer the pending request, or interrupt first",
                );
                return Ok(false);
            }

            if let AppClientCommand::ResolveServerRequest { .. } = &command {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Error,
                    "server requests must be answered through the active approval view",
                );
                return Ok(false);
            }
            if let AppClientCommand::ResetConversation { .. } = &command {
                app.reset_local_view();
                client.send_command(command)?;
                app.arm_reset_notice_suppression();
                return Ok(false);
            }
            if let AppClientCommand::SubmitTurn(UserTurnInput { content, .. }) = &command {
                app.prepare_submitted_turn(content);
            }
            match command {
                AppClientCommand::SubmitTurn(input) => client.submit_turn(input)?,
                AppClientCommand::InterruptTurn { conversation_id } => {
                    client.interrupt_turn(conversation_id)?
                }
                other => client.send_command(other)?,
            }
            Ok(false)
        }
        ParsedInput::ServerRequestAnswer {
            request_id,
            decision,
            reason,
        } => {
            show_local_notice(
                app,
                crate::state::NoticeLevel::Info,
                format!("Request {}", decision_label(&decision)),
            );
            client.resolve_server_request(
                app.conversation_id.clone(),
                request_id,
                ServerRequestDecision {
                    decision,
                    reason: Some(reason),
                },
            )?;
            Ok(false)
        }
        _ => unreachable!("command input dispatcher received non-command input"),
    }
}
