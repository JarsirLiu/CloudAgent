use crate::input::intent::GatewayConfigUpdate;
use crate::state::NoticeLevel;
use crate::ui::bottom_pane::dialogs::server_request::server_request_model::ServerRequestPresentation;
use agent_core::conversation::{ConversationSummary, ConversationTurn, TranscriptItem};
use agent_core::turn::{TurnId, TurnItemKind};
use agent_core::{ModelRetryStage, ModelUsage, ServerRequest, ServerRequestDecisionKind};
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest,
    ConversationViewSnapshot, InterruptDisposition, RequestId,
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
    LocalSessionListNextPage {
        cursor: String,
    },
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
    LocalReasoning(String),
    LocalModel(String),
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
    LocalImagePaste,
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
    SetConversationListPage {
        conversations: Vec<ConversationSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    },
    InvalidateSkillsCatalog,
    SetConversationView(ConversationViewSnapshot),
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
    ClearActiveTool {
        item_id: Option<String>,
    },
    ReplaceHistory(Vec<ConversationTurn>),
    ReplaceHistoryPage {
        turns: Vec<ConversationTurn>,
        has_more: bool,
        next_before_turn_id: Option<String>,
    },
    PrependHistoryPage {
        turns: Vec<ConversationTurn>,
        has_more: bool,
        next_before_turn_id: Option<String>,
    },
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
    AppendActiveToolDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendCommandOutputDelta {
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
    InterruptResult(InterruptDisposition),
    PushErrorCell(String),
    TurnDispatch(TurnDispatch),
    ShowServerRequestPrompt {
        request_id: RequestId,
        request: ServerRequestPresentation,
    },
}

pub(crate) fn apply_server_message(message: &AppServerMessage) -> ServerMessageReduce {
    let mut actions = Vec::new();
    match message {
        AppServerMessage::Notification(notification) => match notification {
            AppServerNotification::ConversationViewChanged { snapshot, .. } => {
                actions.push(ServerAction::SetConversationView(snapshot.clone()));
            }
            AppServerNotification::TurnStarted { turn_id, .. } => {
                actions.push(ServerAction::ClearCurrentTurnUsage);
                actions.push(ServerAction::BindActiveTurn(turn_id.clone()));
            }
            AppServerNotification::ConversationHistory { turns, .. } => {
                actions.push(ServerAction::ReplaceHistory(turns.clone()));
            }
            AppServerNotification::ConversationHistoryPage {
                turns,
                has_more,
                next_before_turn_id,
                ..
            } => {
                actions.push(ServerAction::ReplaceHistoryPage {
                    turns: turns.clone(),
                    has_more: *has_more,
                    next_before_turn_id: next_before_turn_id.clone(),
                });
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
            AppServerNotification::CommandExecutionOutputDelta { item_id, delta, .. } => {
                actions.push(ServerAction::AppendCommandOutputDelta {
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                });
            }
            AppServerNotification::ToolOutputDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                actions.push(ServerAction::AppendActiveToolDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                });
            }
            AppServerNotification::FileChangeOutputDelta { .. } => {}
            AppServerNotification::ItemCompleted { turn_id, item, .. } => {
                if matches!(item, TranscriptItem::ToolResult { .. }) {
                    actions.push(ServerAction::ClearActiveTool {
                        item_id: Some(item.id().to_string()),
                    });
                }
                actions.push(ServerAction::CompleteActiveTurnItem {
                    turn_id: turn_id.clone(),
                    item_id: item.id().to_string(),
                    item: item.clone(),
                });
            }
            AppServerNotification::ConversationListPage {
                conversations,
                has_more,
                next_cursor,
                ..
            } => {
                actions.push(ServerAction::SetConversationListPage {
                    conversations: conversations.clone(),
                    has_more: *has_more,
                    next_cursor: next_cursor.clone(),
                });
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
            AppServerNotification::InterruptResult { disposition, .. } => {
                actions.push(ServerAction::InterruptResult(disposition.clone()));
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
                actions.push(ServerAction::ClearActiveTool { item_id: None });
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
                    actions.push(ServerAction::ClearActiveTool { item_id: None });
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
                actions.push(ServerAction::TurnDispatch(TurnDispatch::Completed));
            }
            AppServerNotification::TurnFailed { error, .. } => {
                actions.push(ServerAction::TurnDispatch(TurnDispatch::Failed {
                    error: error.clone(),
                }));
            }
            AppServerNotification::TurnCancelled { reason, .. } => {
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
                actions.push(ServerAction::ShowServerRequestPrompt {
                    request_id: request_id.clone(),
                    request: ServerRequestPresentation::command(
                        request.tool_name.clone(),
                        request.reason.clone(),
                        summarize_args_preview(&request.command_preview),
                    ),
                });
            }
            ServerRequest::FileChangeApproval { request } => {
                actions.push(ServerAction::ShowServerRequestPrompt {
                    request_id: request_id.clone(),
                    request: ServerRequestPresentation::file_change(
                        request.tool_name.clone(),
                        request.reason.clone(),
                        summarize_args_preview(&request.change_preview),
                    ),
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
#[path = "reducer_tests.rs"]
mod tests;
