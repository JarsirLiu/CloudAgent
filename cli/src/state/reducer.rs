use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, ConversationTurn,
    FrontendMode, RequestId, ServerRequest, ServerRequestDecisionKind, TranscriptItem,
    TurnItemKind, UserTurnInput,
};

#[derive(Debug, Clone)]
pub(crate) enum ItemDispatch {
    AssistantStarted {
        turn_id: String,
        item_id: String,
    },
    ReasoningStarted {
        item_id: String,
        title: String,
    },
    ControlStarted {
        item_id: String,
        kind: TurnItemKind,
        title: String,
    },
    AssistantDelta {
        item_id: String,
        delta: String,
    },
    ReasoningDelta {
        item_id: String,
        delta: String,
    },
    ControlDelta {
        item_id: String,
        delta: String,
    },
    AssistantCompleted {
        item: TranscriptItem,
    },
    ReasoningCompleted {
        item: TranscriptItem,
    },
    ControlCompleted {
        item: TranscriptItem,
    },
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
    ServerRequestAnswer {
        decision: ServerRequestDecisionKind,
        reason: String,
    },
    LocalCopy,
    LocalHelp,
    LocalInputError(String),
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
    ReplaceHistory(Vec<ConversationTurn>),
    PushErrorCell(String),
    ItemDispatch(ItemDispatch),
    TurnDispatch(TurnDispatch),
    ShowServerRequestPrompt {
        title: String,
        detail: String,
        notice: String,
    },
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
                AppServerNotification::ConversationHistory { turns, .. } => {
                    let message_count = turns.iter().map(|turn| turn.items.len()).sum();
                    actions.push(ServerAction::SetLastMessageCount(message_count));
                    actions.push(ServerAction::SetStatusNotice(Some(
                        "Workspace context ready".to_string(),
                    )));
                    actions.push(ServerAction::SetHistoryLoaded(true));
                    actions.push(ServerAction::ReplaceHistory(turns.clone()));
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
                AppServerNotification::ServerRequestResolved { decision, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Running));
                    actions.push(ServerAction::SetPendingServerRequest(None));
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::SetStatusNotice(Some(format!(
                        "Request {}{}",
                        decision.label(),
                        decision
                            .reason
                            .as_deref()
                            .map(|r| format!(": {r}"))
                            .unwrap_or_default()
                    ))));
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
                actions.push(ServerAction::SetPendingServerRequest(Some(
                    request_id.clone(),
                )));
                actions.push(ServerAction::ShowServerRequestPrompt {
                    title: format!("tool `{}` wants to run", request.tool_name),
                    detail: format!(
                        "reason: {}  args: {}",
                        request.reason, request.arguments_preview
                    ),
                    notice: format!("Action required for {}", request.tool_name),
                });
            }
        },
    }

    ServerMessageReduce { actions }
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
        _ if mode == FrontendMode::WaitingForServerRequest => {
            let decision = match trimmed {
                "2" | "a" | "A" | "all" | "ALL" | "session" | "SESSION" => {
                    ServerRequestDecisionKind::AcceptForSession
                }
                "3" | "n" | "N" | "no" | "NO" => ServerRequestDecisionKind::Decline,
                _ => ServerRequestDecisionKind::Accept,
            };
            UiInputEvent::ServerRequestAnswer {
                reason: format!("{} by console operator", decision_label(&decision)),
                decision,
            }
        }
        _ => UiInputEvent::Command(AppClientCommand::SubmitTurn(UserTurnInput {
            conversation_id: conversation_id.to_string(),
            content: trimmed.to_string(),
        })),
    }
}

fn decision_label(decision: &ServerRequestDecisionKind) -> &'static str {
    match decision {
        ServerRequestDecisionKind::Accept => "approved",
        ServerRequestDecisionKind::AcceptForSession => "approved for session",
        ServerRequestDecisionKind::Decline => "denied",
        ServerRequestDecisionKind::Cancel => "cancelled",
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
        } if *kind == TurnItemKind::Reasoning && title.is_some() => {
            Some(ItemDispatch::ReasoningStarted {
                item_id: item_id.clone(),
                title: title.clone().unwrap_or_default(),
            })
        }
        AppServerNotification::ItemStarted {
            item_id,
            kind,
            title,
            ..
        } if (*kind == TurnItemKind::ToolCall || *kind == TurnItemKind::CommandExecution)
            && title.is_some() =>
        {
            Some(ItemDispatch::ControlStarted {
                item_id: item_id.clone(),
                kind: kind.clone(),
                title: title.clone().unwrap_or_default(),
            })
        }
        AppServerNotification::AgentMessageDelta { item_id, delta, .. } => {
            Some(ItemDispatch::AssistantDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            })
        }
        AppServerNotification::ReasoningSummaryTextDelta { item_id, delta, .. } => {
            Some(ItemDispatch::ReasoningDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            })
        }
        AppServerNotification::ReasoningTextDelta { item_id, delta, .. } => {
            Some(ItemDispatch::ReasoningDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            })
        }
        AppServerNotification::CommandExecutionOutputDelta { item_id, delta, .. } => {
            Some(ItemDispatch::ControlDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            })
        }
        AppServerNotification::ToolOutputDelta { item_id, delta, .. } => {
            Some(ItemDispatch::ControlDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            })
        }
        AppServerNotification::FileChangeOutputDelta { item_id, delta, .. } => {
            Some(ItemDispatch::ControlDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            })
        }
        AppServerNotification::ItemCompleted { item, .. } => match item {
            TranscriptItem::AgentMessage { .. } => {
                Some(ItemDispatch::AssistantCompleted { item: item.clone() })
            }
            TranscriptItem::Reasoning { .. } => {
                Some(ItemDispatch::ReasoningCompleted { item: item.clone() })
            }
            TranscriptItem::CommandExecution { .. }
            | TranscriptItem::FileChange { .. }
            | TranscriptItem::ToolResult { .. } => {
                Some(ItemDispatch::ControlCompleted { item: item.clone() })
            }
            TranscriptItem::UserMessage { .. } | TranscriptItem::SystemMessage { .. } => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::TurnState;

    #[test]
    fn conversation_history_action_preserves_turns() {
        let message = AppServerMessage::Notification(AppServerNotification::ConversationHistory {
            conversation_id: "default".to_string(),
            turns: vec![ConversationTurn {
                id: "turn-1".to_string(),
                state: TurnState::Completed,
                items: vec![TranscriptItem::AgentMessage {
                    id: "assistant:1".to_string(),
                    text: "hello".to_string(),
                }],
                rollout_start_index: 0,
                rollout_end_index: 1,
            }],
        });

        let reduced = apply_server_message(&message);

        assert!(reduced.actions.iter().any(|action| {
            matches!(
                action,
                ServerAction::ReplaceHistory(turns)
                    if turns.len() == 1 && turns[0].id == "turn-1"
            )
        }));
    }
}
