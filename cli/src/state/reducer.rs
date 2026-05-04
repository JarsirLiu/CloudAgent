use crate::state::NoticeLevel;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, ConversationTurn,
    FrontendMode, ModelRetryStage, ModelUsage, RequestId, ServerRequest, ServerRequestDecisionKind,
    TranscriptItem, TurnItemKind,
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
    LocalConversationCreate(String),
    LocalConversationSwitch(String),
    LocalConversationTitle(String),
    LocalConversationArchive(String),
    LocalConversationDelete(String),
    LocalFilterToggle(String),
    LocalPermissionMode(String),
    LocalConfig {
        api_key: String,
        base_url: String,
        model: String,
    },
    ServerRequestAnswer {
        request_id: RequestId,
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
#[allow(clippy::large_enum_variant)]
pub(crate) enum ServerAction {
    SetMode(FrontendMode),
    SetSystemNotice {
        text: String,
        level: NoticeLevel,
    },
    ClearSystemNotice,
    SetHistoryLoaded(bool),
    SetConversationList(Vec<agent_protocol::ConversationSummary>),
    SwitchConversation(String),
    ClearCurrentTurnUsage,
    SetTokenUsage {
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
    },
    SetRetryStatus {
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    },
    ClearServerRequestView,
    DismissServerRequestView(RequestId),
    ClearServerRequestStatus,
    ClearLastToolName,
    ReplaceHistory(Vec<ConversationTurn>),
    PushErrorCell(String),
    ItemDispatch(ItemDispatch),
    TurnDispatch(TurnDispatch),
    ShowServerRequestPrompt {
        request_id: RequestId,
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
                AppServerNotification::TurnStarted { .. } => {
                    actions.push(ServerAction::ClearCurrentTurnUsage);
                }
                AppServerNotification::ConversationStatus { .. } => {
                    actions.push(ServerAction::ClearSystemNotice);
                }
                AppServerNotification::ConversationHistory { turns, .. } => {
                    actions.push(ServerAction::SetSystemNotice {
                        text: "Workspace context ready".to_string(),
                        level: NoticeLevel::Info,
                    });
                    actions.push(ServerAction::SetHistoryLoaded(true));
                    actions.push(ServerAction::ReplaceHistory(turns.clone()));
                }
                AppServerNotification::ConversationList { conversations, .. } => {
                    actions.push(ServerAction::SetConversationList(conversations.clone()));
                }
                AppServerNotification::ConversationSwitched { conversation_id } => {
                    actions.push(ServerAction::SwitchConversation(conversation_id.clone()));
                    actions.push(ServerAction::SetSystemNotice {
                        text: format!("Switched to `{conversation_id}`"),
                        level: NoticeLevel::Info,
                    });
                }
                AppServerNotification::Info { message, .. } => {
                    actions.push(ServerAction::SetSystemNotice {
                        text: message.clone(),
                        level: NoticeLevel::Info,
                    });
                }
                AppServerNotification::TokenUsageUpdated {
                    last_usage,
                    total_usage,
                    model_context_window,
                    ..
                } => {
                    actions.push(ServerAction::SetTokenUsage {
                        last_usage: last_usage.clone(),
                        total_usage: total_usage.clone(),
                        model_context_window: *model_context_window,
                    });
                }
                AppServerNotification::ModelRetrying {
                    stage,
                    attempt,
                    next_delay_ms,
                    ..
                } => {
                    actions.push(ServerAction::SetRetryStatus {
                        stage: stage.clone(),
                        attempt: *attempt,
                        next_delay_ms: *next_delay_ms,
                    });
                }
                AppServerNotification::ContextCompacted {
                    pre_context_tokens_estimate,
                    post_context_tokens_estimate,
                    ..
                } => {
                    let summary = format!(
                        "Context compacted: ~{} -> ~{} tokens",
                        pre_context_tokens_estimate, post_context_tokens_estimate
                    );
                    actions.push(ServerAction::SetSystemNotice {
                        text: summary.clone(),
                        level: NoticeLevel::Warn,
                    });
                    actions.push(ServerAction::ClearLastToolName);
                }
                AppServerNotification::ContextCompactionStarted {
                    estimated_tokens, ..
                } => {
                    let summary = format!("Compacting context... (~{} tokens)", estimated_tokens);
                    actions.push(ServerAction::SetSystemNotice {
                        text: summary.clone(),
                        level: NoticeLevel::Warn,
                    });
                }
                AppServerNotification::Error { message, .. } => {
                    actions.push(ServerAction::SetSystemNotice {
                        text: message.clone(),
                        level: NoticeLevel::Error,
                    });
                    actions.push(ServerAction::PushErrorCell(message.clone()));
                }
                AppServerNotification::ServerRequestRequested { request, .. } => {
                    let notice = match request {
                        ServerRequest::CommandApproval { request } => {
                            format!("Command approval required for {}", request.tool_name)
                        }
                        ServerRequest::FileChangeApproval { request } => {
                            format!("Action required for {}", request.tool_name)
                        }
                    };
                    actions.push(ServerAction::SetSystemNotice {
                        text: notice,
                        level: NoticeLevel::Warn,
                    });
                }
                AppServerNotification::ServerRequestResolved {
                    request_id,
                    decision,
                    ..
                } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Running));
                    actions.push(ServerAction::DismissServerRequestView(request_id.clone()));
                    actions.push(ServerAction::SetSystemNotice {
                        text: format!(
                            "Request {}{}",
                            decision.label(),
                            decision
                                .reason
                                .as_deref()
                                .map(|r| format!(": {r}"))
                                .unwrap_or_default()
                        ),
                        level: NoticeLevel::Info,
                    });
                }
                AppServerNotification::TurnCompleted { .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::ClearServerRequestStatus);
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::SetSystemNotice {
                        text: "Turn complete".to_string(),
                        level: NoticeLevel::Info,
                    });
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Completed));
                }
                AppServerNotification::TurnFailed { error, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::ClearServerRequestStatus);
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Failed {
                        error: error.clone(),
                    }));
                }
                AppServerNotification::TurnCancelled { reason, .. } => {
                    actions.push(ServerAction::SetMode(FrontendMode::Idle));
                    actions.push(ServerAction::ClearServerRequestStatus);
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
            ServerRequest::CommandApproval { request } => {
                let args_hint = summarize_args_preview(&request.command_preview);
                actions.push(ServerAction::SetMode(FrontendMode::WaitingForServerRequest));
                actions.push(ServerAction::ShowServerRequestPrompt {
                    request_id: request_id.clone(),
                    title: format!(
                        "[{}] command tool `{}` wants approval",
                        message.conversation_id().unwrap_or("conversation"),
                        request.tool_name
                    ),
                    detail: format!("reason: {}\nargs: {args_hint}", request.reason),
                    notice: format!("Command approval required for {}", request.tool_name),
                });
            }
            ServerRequest::FileChangeApproval { request } => {
                let change_hint = summarize_args_preview(&request.change_preview);
                actions.push(ServerAction::SetMode(FrontendMode::WaitingForServerRequest));
                actions.push(ServerAction::ShowServerRequestPrompt {
                    request_id: request_id.clone(),
                    title: format!(
                        "[{}] file change tool `{}` wants approval",
                        message.conversation_id().unwrap_or("conversation"),
                        request.tool_name
                    ),
                    detail: format!("reason: {}\nchange: {change_hint}", request.reason),
                    notice: format!("File change approval required for {}", request.tool_name),
                });
            }
        },
    }

    ServerMessageReduce { actions }
}

fn summarize_args_preview(arguments_preview: &str) -> String {
    let trimmed = arguments_preview.trim();
    if trimmed.is_empty() {
        return "(none)".to_string();
    }
    if trimmed.chars().count() <= 80 {
        return trimmed.to_string();
    }
    let mut out = String::new();
    for ch in trimmed.chars().take(80) {
        out.push(ch);
    }
    out.push_str("… (truncated)");
    out
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
                items: vec![
                    TranscriptItem::UserMessage {
                        id: "user:1".to_string(),
                        text: "hi".to_string(),
                    },
                    TranscriptItem::CommandExecution {
                        id: "cmd:1".to_string(),
                        tool_name: "exec_command".to_string(),
                        command: "pwd".to_string(),
                        current_directory: "D:\\work".to_string(),
                        status: agent_protocol::CommandExecutionStatus::Completed,
                        exit_code: Some(0),
                        stdout: Some("D:\\work".to_string()),
                        stderr: None,
                        aggregated_output: Some("D:\\work".to_string()),
                        duration_ms: Some(1),
                        summary: "D:\\work".to_string(),
                    },
                    TranscriptItem::AgentMessage {
                        id: "assistant:1".to_string(),
                        text: "hello".to_string(),
                    },
                ],
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

    #[test]
    fn token_usage_notification_updates_run_state() {
        let message = AppServerMessage::Notification(AppServerNotification::TokenUsageUpdated {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            last_usage: agent_protocol::ModelUsage {
                input_tokens: 10,
                cached_input_tokens: 2,
                output_tokens: 3,
                reasoning_output_tokens: 1,
                total_tokens: 13,
            },
            total_usage: agent_protocol::ModelUsage {
                input_tokens: 20,
                cached_input_tokens: 4,
                output_tokens: 6,
                reasoning_output_tokens: 2,
                total_tokens: 26,
            },
            model_context_window: Some(100),
        });

        let reduced = apply_server_message(&message);

        assert!(reduced.actions.iter().any(|action| {
            matches!(
                action,
                ServerAction::SetTokenUsage {
                    last_usage,
                    total_usage,
                    model_context_window,
                } if last_usage.total_tokens == 13
                    && total_usage.cached_input_tokens == 4
                    && *model_context_window == Some(100)
            )
        }));
    }

    #[test]
    fn model_retrying_notification_sets_retry_status() {
        let message = AppServerMessage::Notification(AppServerNotification::ModelRetrying {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            stage: agent_protocol::ModelRetryStage::Streaming,
            attempt: 2,
            next_delay_ms: 500,
        });

        let reduced = apply_server_message(&message);

        assert!(reduced.actions.iter().any(|action| {
            matches!(
                action,
                ServerAction::SetRetryStatus {
                    stage,
                    attempt,
                    next_delay_ms,
                } if *stage == agent_protocol::ModelRetryStage::Streaming
                    && *attempt == 2
                    && *next_delay_ms == 500
            )
        }));
    }
}
