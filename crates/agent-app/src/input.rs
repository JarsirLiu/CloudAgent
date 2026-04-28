use agent_protocol::{AppClientCommand, FrontendMode, UserTurnInput};

#[derive(Debug)]
pub(crate) enum ParsedInput {
    Command(AppClientCommand),
    Empty,
}

pub(crate) fn parse_line(
    line: &str,
    session_id: &str,
    mode: FrontendMode,
) -> ParsedInput {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedInput::Empty;
    }

    if mode == FrontendMode::WaitingForApproval && !trimmed.starts_with('/') {
        let approved = matches!(trimmed, "y" | "Y" | "yes" | "YES");
        let reason = if approved {
            Some("approved by console operator".to_string())
        } else {
            Some("denied by console operator".to_string())
        };
        return ParsedInput::Command(AppClientCommand::ApprovalResponse {
            session_id: session_id.to_string(),
            approved,
            reason,
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
        _ => AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: trimmed.to_string(),
        }),
    };

    ParsedInput::Command(command)
}
