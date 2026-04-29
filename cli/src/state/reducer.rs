use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode,
    HistoryEntry, RequestId, ServerRequest, ThreadItem, TurnItemKind,
    UserTurnInput,
};

#[derive(Debug, Clone)]
pub(crate) enum ItemDispatch {
    AssistantStarted { turn_id: String, item_id: String },
    ToolLikeStarted { item_id: String, kind: TurnItemKind, title: String },
    AssistantDelta { item_id: String, delta: String },
    ToolLikeDelta { item_id: String, delta: String },
    AssistantCompleted { item: ThreadItem },
    ToolLikeCompleted { item: ThreadItem },
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
    ServerRequestAnswer { approved: bool, reason: String },
    LocalCopy,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ServerMessageReduce {
    pub(crate) actions: Vec<ServerAction>,
}

#[derive(Debug, Clone)]
pub(crate) enum ServerAction {
    SetMode(FrontendMode),
    SetPendingServerRequest(Option<RequestId>),
    SetStatusNotice(Option<String>),
    SetLastMessageCount(usize),
    SetHistoryLoaded(bool),
    ClearServerRequestView,
    ClearLastToolName,
    ReplaceHistory(Vec<HistoryEntry>),
    PushErrorCell(String),
    ItemDispatch(ItemDispatch),
    TurnDispatch(TurnDispatch),
    ShowServerRequestPrompt { title: String, detail: String, notice: String },
}

pub(crate) fn apply_server_message(message: &AppServerMessage) -> ServerMessageReduce {
    let mut actions = Vec::new();
    match message {
        AppServerMessage::Notification(notification) => {
            match notification {
                AppServerNotification::FrontendStateChanged { mode, .. } => {
                    actions.push(ServerAction::SetMode(*mode));
                }
                AppServerNotification::ConversationStatus { snapshot, .. } => {
                    actions.push(ServerAction::SetLastMessageCount(snapshot.message_count));
                    actions.push(ServerAction::SetStatusNotice(None));
                }
                AppServerNotification::ConversationHistory { messages, .. } => {
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
                AppServerNotification::ServerRequestRequested { request, .. } => {
                    let notice = match request {
                        ServerRequest::ToolApproval { request } => {
                            format!("Action required for {}", request.tool_name)
                        }
                    };
                    actions.push(ServerAction::SetStatusNotice(Some(notice)));
                }
                AppServerNotification::ServerRequestResolved {
                    decision,
                    ..
                } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Running));
                    actions.push(ServerAction::SetPendingServerRequest(None));
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::SetStatusNotice(Some(if decision.approved {
                        format!("Request approved{}", decision.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default())
                    } else {
                        format!("Request denied{}", decision.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default())
                    })));
                }
                AppServerNotification::TurnCompleted { .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::SetPendingServerRequest(None));
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::SetStatusNotice(None));
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Completed));
                }
                AppServerNotification::TurnFailed { error, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::SetPendingServerRequest(None));
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Failed {
                        error: error.clone(),
                    }));
                }
                AppServerNotification::TurnCancelled { reason, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::SetPendingServerRequest(None));
                    actions.push(ServerAction::ClearServerRequestView);
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
        AppServerMessage::Request(AppServerRequest::ServerRequest {
            request_id,
            request,
            ..
        }) => match request {
            ServerRequest::ToolApproval { request } => {
                actions.push(ServerAction::SetMode(FrontendMode::WaitingForServerRequest));
                actions.push(ServerAction::SetPendingServerRequest(Some(request_id.clone())));
                actions.push(ServerAction::ShowServerRequestPrompt {
                    title: format!("tool `{}` wants to run", request.tool_name),
                    detail: format!(
                        "reason: {}  args: {}",
                        request.reason, request.arguments_preview
                    ),
                    notice: format!("Action required for {}", request.tool_name),
                });
            }
        }
    }

    ServerMessageReduce {
        actions,
    }
}

pub(crate) fn apply_ui_event(
    line: &str,
    conversation_id: &str,
    mode: FrontendMode,
) -> UiInputEvent {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return UiInputEvent::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            conversation_id: conversation_id.to_string(),
            content: String::new(),
        }));
    }

    match trimmed {
        "/copy" => UiInputEvent::LocalCopy,
        "/exit" | "/quit" => UiInputEvent::Command(AppClientCommand::Exit),
        "/clear" => UiInputEvent::Command(AppClientCommand::ResetConversation {
            conversation_id: conversation_id.to_string(),
        }),
        "/interrupt" => UiInputEvent::Command(AppClientCommand::InterruptTurn {
            conversation_id: conversation_id.to_string(),
        }),
        _ if mode == FrontendMode::WaitingForServerRequest => {
            let approved = matches!(trimmed, "1" | "y" | "Y" | "yes" | "YES");
            UiInputEvent::ServerRequestAnswer {
                approved,
                reason: if approved {
                    "approved by console operator".to_string()
                } else {
                    "denied by console operator".to_string()
                },
            }
        }
        _ => UiInputEvent::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            conversation_id: conversation_id.to_string(),
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
        } if (*kind == TurnItemKind::Reasoning
            || *kind == TurnItemKind::ToolCall
            || *kind == TurnItemKind::CommandExecution)
            && title.is_some() =>
        {
            Some(ItemDispatch::ToolLikeStarted {
                item_id: item_id.clone(),
                kind: kind.clone(),
                title: title.clone().unwrap_or_default(),
            })
        }
        AppServerNotification::AgentMessageDelta {
            item_id,
            delta,
            ..
        } => Some(ItemDispatch::AssistantDelta {
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ReasoningSummaryTextDelta {
            item_id,
            delta,
            ..
        }
        | AppServerNotification::ReasoningTextDelta {
            item_id,
            delta,
            ..
        }
        | AppServerNotification::CommandExecutionOutputDelta {
            item_id,
            delta,
            ..
        } => Some(ItemDispatch::ToolLikeDelta {
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ItemCompleted { item, .. } => match item {
            ThreadItem::AgentMessage { .. } => Some(ItemDispatch::AssistantCompleted {
                item: item.clone(),
            }),
            ThreadItem::CommandExecution { .. }
            | ThreadItem::ToolResult { .. }
            | ThreadItem::Reasoning { .. } => {
                Some(ItemDispatch::ToolLikeCompleted { item: item.clone() })
            }
            ThreadItem::UserMessage { .. } => None,
        },
        _ => None,
    }
}
