use agent_protocol::{AppClientCommand, FrontendMode, UserTurnInput};

pub(crate) enum ParsedInput {
    Command(AppClientCommand),
    ApprovalAnswer { approved: bool, reason: String },
    LocalCopy,
}

pub(crate) fn parse_line(line: &str, session_id: &str, mode: FrontendMode) -> ParsedInput {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedInput::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: String::new(),
        }));
    }

    let command = match trimmed {
        "/copy" => return ParsedInput::LocalCopy,
        "/exit" | "/quit" => AppClientCommand::Exit,
        "/clear" => AppClientCommand::ResetSession {
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

