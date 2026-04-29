use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode,
    HistoryEntry, RequestId, TurnItemDeltaKind, TurnItemKind, UserTurnInput,
};

#[derive(Debug, Clone)]
pub(crate) enum ItemDispatch {
    AssistantStarted { turn_id: String, item_id: String },
    ToolLikeStarted { item_id: String, title: String },
    AssistantDelta { item_id: String, delta: String },
    AssistantCompleted { item_id: String },
    ToolLikeCompleted { item_id: String },
}

#[derive(Debug, Clone)]
pub(crate) enum TurnDispatch {
    Completed,
    Failed { error: String },
    Cancelled { reason: String },
}

#[derive(Debug, Clone)]
pub(crate) enum UiInputEvent {
    Command(AppClientCommand),
    ApprovalAnswer { approved: bool, reason: String },
    LocalCopy,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ServerMessageReduce {
    pub(crate) actions: Vec<ServerAction>,
}

#[derive(Debug, Clone)]
pub(crate) enum ServerAction {
    SetMode(FrontendMode),
    SetPendingApproval(Option<RequestId>),
    SetStatusNotice(Option<String>),
    SetLastMessageCount(usize),
    SetHistoryLoaded(bool),
    ClearApprovalView,
    ClearLastToolName,
    ReplaceHistory(Vec<HistoryEntry>),
    PushErrorCell(String),
    ItemDispatch(ItemDispatch),
    TurnDispatch(TurnDispatch),
    ShowApprovalPrompt { title: String, detail: String, notice: String },
}

pub(crate) fn apply_server_message(message: &AppServerMessage) -> ServerMessageReduce {
    let mut actions = Vec::new();
    match message {
        AppServerMessage::Notification(notification) => {
            match notification {
                AppServerNotification::FrontendStateChanged { mode, .. } => {
                    actions.push(ServerAction::SetMode(*mode));
                }
                AppServerNotification::SessionStatus { snapshot, .. } => {
                    actions.push(ServerAction::SetLastMessageCount(snapshot.message_count));
                    actions.push(ServerAction::SetStatusNotice(None));
                }
                AppServerNotification::SessionHistory { messages, .. } => {
                    actions.push(ServerAction::SetLastMessageCount(messages.len()));
                    actions.push(ServerAction::SetStatusNotice(Some(
                        "Workspace context ready".to_string(),
                    )));
                    actions.push(ServerAction::SetHistoryLoaded(true));
                    actions.push(ServerAction::ReplaceHistory(messages.clone()));
                }
                AppServerNotification::Info { message, .. } => {
                    actions.push(ServerAction::SetStatusNotice(Some(message.clone())));
                }
                AppServerNotification::Error { message, .. } => {
                    actions.push(ServerAction::SetStatusNotice(Some(message.clone())));
                    actions.push(ServerAction::PushErrorCell(message.clone()));
                }
                AppServerNotification::TurnCompleted { .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::SetPendingApproval(None));
                    actions.push(ServerAction::ClearApprovalView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Completed));
                }
                AppServerNotification::TurnFailed { error, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::SetPendingApproval(None));
                    actions.push(ServerAction::ClearApprovalView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Failed {
                        error: error.clone(),
                    }));
                }
                AppServerNotification::TurnCancelled { reason, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::SetPendingApproval(None));
                    actions.push(ServerAction::ClearApprovalView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Cancelled {
                        reason: reason.clone(),
                    }));
                }
                _ => {}
            }
            if let Some(dispatch) = derive_item_dispatch(notification) {
                actions.push(ServerAction::ItemDispatch(dispatch));
            }
        }
        AppServerMessage::Request(AppServerRequest::Approval {
            request_id,
            request,
            ..
        }) => {
            actions.push(ServerAction::SetMode(FrontendMode::WaitingForApproval));
            actions.push(ServerAction::SetPendingApproval(Some(request_id.clone())));
            actions.push(ServerAction::ShowApprovalPrompt {
                title: format!("tool `{}` wants to run", request.tool_name),
                detail: format!(
                    "reason: {}  args: {}",
                    request.reason, request.arguments_preview
                ),
                notice: format!("Approval for {}", request.tool_name),
            });
        }
    }

    ServerMessageReduce {
        actions,
    }
}

pub(crate) fn apply_ui_event(
    line: &str,
    session_id: &str,
    mode: FrontendMode,
) -> UiInputEvent {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return UiInputEvent::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: String::new(),
        }));
    }

    match trimmed {
        "/copy" => UiInputEvent::LocalCopy,
        "/exit" | "/quit" => UiInputEvent::Command(AppClientCommand::Exit),
        "/clear" => UiInputEvent::Command(AppClientCommand::ResetSession {
            session_id: session_id.to_string(),
        }),
        "/interrupt" => UiInputEvent::Command(AppClientCommand::InterruptTurn {
            session_id: session_id.to_string(),
        }),
        _ if mode == FrontendMode::WaitingForApproval => {
            let approved = matches!(trimmed, "1" | "y" | "Y" | "yes" | "YES");
            UiInputEvent::ApprovalAnswer {
                approved,
                reason: if approved {
                    "approved by console operator".to_string()
                } else {
                    "denied by console operator".to_string()
                },
            }
        }
        _ => UiInputEvent::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: trimmed.to_string(),
        })),
    }
}

pub(crate) fn derive_item_dispatch(notification: &AppServerNotification) -> Option<ItemDispatch> {
    match notification {
        AppServerNotification::ItemStarted {
            turn_id,
            item_id,
            kind,
            title,
            ..
        } if *kind == TurnItemKind::AssistantMessage => Some(ItemDispatch::AssistantStarted {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
        }),
        AppServerNotification::ItemStarted {
            item_id,
            kind,
            title,
            ..
        } if (*kind == TurnItemKind::Reasoning || *kind == TurnItemKind::ToolCall)
            && title.is_some() =>
        {
            Some(ItemDispatch::ToolLikeStarted {
                item_id: item_id.clone(),
                title: title.clone().unwrap_or_default(),
            })
        }
        AppServerNotification::ItemDelta {
            item_id,
            kind,
            delta,
            ..
        } if *kind == TurnItemDeltaKind::Text => Some(ItemDispatch::AssistantDelta {
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ItemCompleted { item_id, kind, .. }
            if *kind == TurnItemKind::AssistantMessage =>
        {
            Some(ItemDispatch::AssistantCompleted {
                item_id: item_id.clone(),
            })
        }
        AppServerNotification::ItemCompleted { item_id, kind, .. }
            if *kind == TurnItemKind::Reasoning || *kind == TurnItemKind::ToolCall =>
        {
            Some(ItemDispatch::ToolLikeCompleted {
                item_id: item_id.clone(),
            })
        }
        _ => None,
    }
}
