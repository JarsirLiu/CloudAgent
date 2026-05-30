use crate::input::intent::GatewayConfigUpdate;
use crate::state::NoticeLevel;
use agent_core::conversation::{ConversationSummary, ConversationTurn, TranscriptItem};
use agent_core::turn::{TurnId, TurnItemKind};
use agent_core::{ModelRetryStage, ModelUsage, ServerRequest, ServerRequestDecisionKind};
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode,
    RequestId,
};

const ERR_TRANSPORT_CLOSED_PREFIX: &str = "ERR_TRANSPORT_CLOSED:";

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
    LocalSkillInsert(String),
    LocalSkillsOpen,
    LocalGatewayOpen,
    LocalGatewaySelect(String),
    LocalGatewayWeixinLoginStart(String),
    LocalGatewayWeixinLoginCheck {
        platform: String,
        session_id: String,
        qr_url: String,
    },
    LocalGatewaySave {
        platform: String,
        enabled: bool,
        updates: Vec<GatewayConfigUpdate>,
    },
    ServerRequestAnswer {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
    LocalCopy,
    LocalCopyText(String),
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
    SetConversationList(Vec<ConversationSummary>),
    InvalidateSkillsCatalog,
    SetFrontendMode(FrontendMode),
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
    SetContextCompactionStatus {
        estimated_tokens: u64,
    },
    ClearContextCompactionStatus,
    ClearServerRequestView,
    DismissServerRequestView(RequestId),
    ClearServerRequestStatus,
    ClearLastToolName,
    ReplaceHistory(Vec<ConversationTurn>),
    UpsertTurnSnapshot(ConversationTurn),
    BindActiveTurn(TurnId),
    StartActiveTurnItem {
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    AppendActiveAgentDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendActiveReasoningDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendActiveOutputDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    CompleteActiveTurnItem {
        turn_id: TurnId,
        item_id: String,
        item: TranscriptItem,
    },
    PushNoticeCell {
        label: String,
        message: String,
        level: NoticeLevel,
    },
    PushErrorCell(String),
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
        AppServerMessage::Notification(notification) => match notification {
            AppServerNotification::FrontendStateChanged { mode, .. } => {
                actions.push(ServerAction::SetFrontendMode(*mode));
            }
            AppServerNotification::TurnStarted { turn_id, .. } => {
                actions.push(ServerAction::ClearCurrentTurnUsage);
                actions.push(ServerAction::BindActiveTurn(turn_id.clone()));
            }
            AppServerNotification::ConversationStatus { snapshot, .. } => {
                let mode = match snapshot.conversation_status {
                    agent_core::ConversationStatus::Busy => FrontendMode::Running,
                    agent_core::ConversationStatus::Idle => FrontendMode::Idle,
                };
                actions.push(ServerAction::SetFrontendMode(mode));
            }
            AppServerNotification::ConversationHistory { turns, .. } => {
                actions.push(ServerAction::ReplaceHistory(turns.clone()));
            }
            AppServerNotification::TurnSnapshot { turn, .. } => {
                actions.push(ServerAction::UpsertTurnSnapshot(turn.clone()));
            }
            AppServerNotification::ItemStarted { turn_id, item, .. } => {
                let item_id = item.id().to_string();
                actions.push(ServerAction::StartActiveTurnItem {
                    turn_id: turn_id.clone(),
                    item_id,
                    kind: turn_item_kind(item),
                    title: turn_item_title(item),
                });
            }
            AppServerNotification::AgentMessageDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                actions.push(ServerAction::AppendActiveAgentDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                });
            }
            AppServerNotification::ReasoningSummaryTextDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                actions.push(ServerAction::AppendActiveReasoningDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                });
            }
            AppServerNotification::ReasoningTextDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                actions.push(ServerAction::AppendActiveReasoningDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                });
            }
            AppServerNotification::ReasoningSummaryPartAdded { .. } => {}
            AppServerNotification::CommandExecutionOutputDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                actions.push(ServerAction::AppendActiveOutputDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                });
            }
            AppServerNotification::ToolOutputDelta { .. }
            | AppServerNotification::FileChangeOutputDelta { .. } => {}
            AppServerNotification::ItemCompleted { turn_id, item, .. } => {
                actions.push(ServerAction::CompleteActiveTurnItem {
                    turn_id: turn_id.clone(),
                    item_id: item.id().to_string(),
                    item: item.clone(),
                });
            }
            AppServerNotification::ConversationList { conversations, .. } => {
                actions.push(ServerAction::SetConversationList(conversations.clone()));
            }
            AppServerNotification::SkillsChanged { .. } => {
                actions.push(ServerAction::InvalidateSkillsCatalog);
            }
            AppServerNotification::ConversationSwitched { conversation_id } => {
                actions.push(ServerAction::SwitchConversation(conversation_id.clone()));
            }
            AppServerNotification::Info { message, .. } => {
                actions.push(ServerAction::PushNoticeCell {
                    label: "conversation".to_string(),
                    message: message.clone(),
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
                actions.push(ServerAction::PushNoticeCell {
                    label: "context".to_string(),
                    message: summary.clone(),
                    level: NoticeLevel::Warn,
                });
                actions.push(ServerAction::ClearContextCompactionStatus);
                actions.push(ServerAction::ClearLastToolName);
            }
            AppServerNotification::ContextCompactionStarted {
                estimated_tokens, ..
            } => {
                actions.push(ServerAction::SetContextCompactionStatus {
                    estimated_tokens: *estimated_tokens,
                });
            }
            AppServerNotification::Error { message, .. } => {
                if let Some(message) = transport_closed_message(message) {
                    actions.push(ServerAction::ClearContextCompactionStatus);
                    actions.push(ServerAction::ClearServerRequestStatus);
                    actions.push(ServerAction::ClearServerRequestView);
                    actions.push(ServerAction::ClearLastToolName);
                    actions.push(ServerAction::TurnDispatch(TurnDispatch::Failed {
                        error: message,
                    }));
                } else {
                    actions.push(ServerAction::PushErrorCell(message.clone()));
                }
            }
            AppServerNotification::ServerRequestRequested { request, .. } => {
                let _ = request;
            }
            AppServerNotification::ServerRequestResolved {
                request_id,
                decision,
                ..
            } => {
                actions.push(ServerAction::DismissServerRequestView(request_id.clone()));
                actions.push(ServerAction::PushNoticeCell {
                    label: "request".to_string(),
                    message: format!(
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
                actions.push(ServerAction::ClearContextCompactionStatus);
                actions.push(ServerAction::ClearServerRequestStatus);
                actions.push(ServerAction::ClearServerRequestView);
                actions.push(ServerAction::ClearLastToolName);
                actions.push(ServerAction::TurnDispatch(TurnDispatch::Completed));
            }
            AppServerNotification::TurnFailed { error, .. } => {
                actions.push(ServerAction::ClearContextCompactionStatus);
                actions.push(ServerAction::ClearServerRequestStatus);
                actions.push(ServerAction::ClearServerRequestView);
                actions.push(ServerAction::ClearLastToolName);
                actions.push(ServerAction::TurnDispatch(TurnDispatch::Failed {
                    error: error.clone(),
                }));
            }
            AppServerNotification::TurnCancelled { reason, .. } => {
                actions.push(ServerAction::ClearContextCompactionStatus);
                actions.push(ServerAction::ClearServerRequestStatus);
                actions.push(ServerAction::ClearServerRequestView);
                actions.push(ServerAction::ClearLastToolName);
                actions.push(ServerAction::TurnDispatch(TurnDispatch::Cancelled {
                    reason: reason.clone(),
                }));
            }
            _ => {}
        },
        AppServerMessage::Request(AppServerRequest::ServerRequest {
            request_id,
            request,
            ..
        }) => match request {
            ServerRequest::CommandApproval { request } => {
                let args_hint = summarize_args_preview(&request.command_preview);
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

fn turn_item_kind(item: &TranscriptItem) -> TurnItemKind {
    match item {
        TranscriptItem::SystemMessage { .. } => TurnItemKind::SystemNote,
        TranscriptItem::UserMessage { .. } => TurnItemKind::UserMessage,
        TranscriptItem::AgentMessage { .. } => TurnItemKind::AssistantMessage,
        TranscriptItem::CommandExecution { .. } => TurnItemKind::CommandExecution,
        TranscriptItem::FileChange { .. } => TurnItemKind::FileChange,
        TranscriptItem::ToolResult { .. } => TurnItemKind::ToolResult,
        TranscriptItem::Reasoning { .. } => TurnItemKind::Reasoning,
    }
}

fn turn_item_title(item: &TranscriptItem) -> Option<String> {
    match item {
        TranscriptItem::SystemMessage { .. } | TranscriptItem::UserMessage { .. } => None,
        TranscriptItem::AgentMessage { .. } => Some("assistant_message".to_string()),
        TranscriptItem::CommandExecution { command, .. } => Some(command.clone()),
        TranscriptItem::FileChange { path, .. } => Some(path.clone()),
        TranscriptItem::ToolResult { tool_name, .. } => Some(tool_name.clone()),
        TranscriptItem::Reasoning { title, .. } => Some(title.clone()),
    }
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

fn transport_closed_message(message: &str) -> Option<String> {
    message
        .strip_prefix(ERR_TRANSPORT_CLOSED_PREFIX)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(ToOwned::to_owned)
}
#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        CommandExecutionStatus, CompactionContinuation, InputItem, TranscriptItem, TurnState,
    };

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
                        content: vec![InputItem::Text {
                            text: "hi".to_string(),
                        }],
                    },
                    TranscriptItem::CommandExecution {
                        id: "cmd:1".to_string(),
                        tool_name: "exec_command".to_string(),
                        command: "pwd".to_string(),
                        current_directory: "D:\\work".to_string(),
                        status: CommandExecutionStatus::Completed,
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
            last_usage: ModelUsage {
                input_tokens: 10,
                cached_input_tokens: 2,
                output_tokens: 3,
                reasoning_output_tokens: 1,
                total_tokens: 13,
            },
            total_usage: ModelUsage {
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
                } if last_usage.total_output_tokens() == 4
                    && last_usage.total_tokens == 13
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
            stage: ModelRetryStage::Streaming,
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
                } if *stage == ModelRetryStage::Streaming
                    && *attempt == 2
                    && *next_delay_ms == 500
            )
        }));
    }

    #[test]
    fn command_output_delta_updates_active_output() {
        let message =
            AppServerMessage::Notification(AppServerNotification::CommandExecutionOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "cmd-1".to_string(),
                call_id: Some("call-1".to_string()),
                delta: "stdout".to_string(),
            });

        let reduced = apply_server_message(&message);

        assert!(reduced.actions.iter().any(|action| {
            matches!(
                action,
                ServerAction::AppendActiveOutputDelta {
                    turn_id,
                    item_id,
                    delta,
                } if turn_id == "turn-1" && item_id == "cmd-1" && delta == "stdout"
            )
        }));
    }

    #[test]
    fn generic_tool_output_deltas_wait_for_completed_item() {
        let tool_message = AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            delta: "large streaming tool output".to_string(),
        });
        let file_message =
            AppServerMessage::Notification(AppServerNotification::FileChangeOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "edit-1".to_string(),
                call_id: Some("call-2".to_string()),
                delta: "patch output".to_string(),
            });

        let tool_reduced = apply_server_message(&tool_message);
        let file_reduced = apply_server_message(&file_message);

        assert!(
            tool_reduced
                .actions
                .iter()
                .all(|action| !matches!(action, ServerAction::AppendActiveOutputDelta { .. }))
        );
        assert!(
            file_reduced
                .actions
                .iter()
                .all(|action| !matches!(action, ServerAction::AppendActiveOutputDelta { .. }))
        );
    }

    #[test]
    fn context_compaction_started_sets_runtime_status_without_notice_cell() {
        let message =
            AppServerMessage::Notification(AppServerNotification::ContextCompactionStarted {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                continuation: CompactionContinuation::MidTurn,
                estimated_tokens: 12_345,
            });

        let reduced = apply_server_message(&message);

        assert!(reduced.actions.iter().any(|action| {
            matches!(
                action,
                ServerAction::SetContextCompactionStatus { estimated_tokens }
                    if *estimated_tokens == 12_345
            )
        }));
        assert!(
            !reduced
                .actions
                .iter()
                .any(|action| matches!(action, ServerAction::PushNoticeCell { .. }))
        );
    }

    #[test]
    fn conversation_switched_only_updates_active_conversation() {
        let message = AppServerMessage::Notification(AppServerNotification::ConversationSwitched {
            conversation_id: "draft-1".to_string(),
        });

        let reduced = apply_server_message(&message);

        assert_eq!(reduced.actions.len(), 1);
        assert!(matches!(
            reduced.actions.first(),
            Some(ServerAction::SwitchConversation(conversation_id))
                if conversation_id == "draft-1"
        ));
    }

    #[test]
    fn skills_changed_invalidates_local_skill_catalog() {
        let message = AppServerMessage::Notification(AppServerNotification::SkillsChanged {
            conversation_id: "default".to_string(),
        });

        let reduced = apply_server_message(&message);

        assert_eq!(reduced.actions.len(), 1);
        assert!(matches!(
            reduced.actions.first(),
            Some(ServerAction::InvalidateSkillsCatalog)
        ));
    }

    #[test]
    fn transport_closed_error_finishes_active_turn() {
        let message = AppServerMessage::Notification(AppServerNotification::Error {
            conversation_id: "default".to_string(),
            message: "ERR_TRANSPORT_CLOSED: worker app server closed unexpectedly".to_string(),
        });

        let reduced = apply_server_message(&message);

        assert!(reduced.actions.iter().any(|action| matches!(
            action,
            ServerAction::TurnDispatch(TurnDispatch::Failed { error })
                if error == "worker app server closed unexpectedly"
        )));
        assert!(
            !reduced
                .actions
                .iter()
                .any(|action| matches!(action, ServerAction::PushErrorCell(_)))
        );
    }
}
