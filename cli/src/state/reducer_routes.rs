use crate::state::NoticeLevel;
use crate::state::reducer::{ServerAction, TurnDispatch};
use crate::ui::bottom_pane::dialogs::server_request::server_request_model::ServerRequestPresentation;
use agent_core::{ServerRequest, ServerRequestDecision};
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest};

pub(crate) fn route_server_message(message: &AppServerMessage) -> Vec<ServerAction> {
    let mut actions = Vec::new();
    match message {
        AppServerMessage::Notification(notification) => {
            route_notification(notification, &mut actions)
        }
        AppServerMessage::Request(request) => route_request(request, &mut actions),
    }
    actions
}

fn route_request(request: &AppServerRequest, actions: &mut Vec<ServerAction>) {
    let AppServerRequest::ServerRequest {
        request_id,
        request,
        ..
    } = request;

    let request = match request {
        ServerRequest::CommandApproval { request } => ServerRequestPresentation::command(
            request.tool_name.clone(),
            request.reason.clone(),
            preview_excerpt(&request.command_preview),
        ),
        ServerRequest::FileChangeApproval { request } => ServerRequestPresentation::file_change(
            request.tool_name.clone(),
            request.reason.clone(),
            preview_excerpt(&request.change_preview),
        ),
    };

    actions.push(ServerAction::ShowServerRequestPrompt {
        request_id: request_id.clone(),
        request,
    });
}

fn route_notification(notification: &AppServerNotification, actions: &mut Vec<ServerAction>) {
    match notification {
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
            actions.push(ServerAction::StartActiveTurnItem {
                turn_id: turn_id.clone(),
                item: item.clone(),
            });
        }
        AppServerNotification::AgentMessageDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => actions.push(ServerAction::AppendActiveAgentDelta {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ReasoningSummaryTextDelta {
            turn_id,
            item_id,
            delta,
            ..
        }
        | AppServerNotification::ReasoningTextDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => actions.push(ServerAction::AppendActiveReasoningDelta {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ReasoningSummaryPartAdded { .. } => {}
        AppServerNotification::CommandExecutionOutputDelta { item_id, delta, .. } => {
            actions.push(ServerAction::AppendActiveRuntimeOutputDelta {
                item_id: item_id.clone(),
                delta: delta.clone(),
            });
        }
        AppServerNotification::ToolOutputDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => actions.push(ServerAction::AppendActiveRuntimeDelta {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::JsonPatchDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => actions.push(ServerAction::AppendActivePatchDelta {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ItemProgress {
            turn_id,
            item_id,
            progress,
            ..
        } => actions.push(ServerAction::UpdateActiveItemProgress {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            progress: progress.clone(),
        }),
        AppServerNotification::ItemMetricsUpdated {
            turn_id,
            item_id,
            metrics,
            ..
        } => actions.push(ServerAction::UpdateActiveItemMetrics {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            metrics: metrics.clone(),
        }),
        AppServerNotification::ItemCompleted {
            turn_id,
            item,
            transcript_item,
            ..
        } => {
            actions.push(ServerAction::CompleteActiveTurnItem {
                turn_id: turn_id.clone(),
                item: item.clone(),
                transcript_item: transcript_item.clone(),
            });
        }
        AppServerNotification::ConversationListPage {
            conversations,
            has_more,
            next_cursor,
            ..
        } => actions.push(ServerAction::SetConversationListPage {
            conversations: conversations.clone(),
            has_more: *has_more,
            next_cursor: next_cursor.clone(),
        }),
        AppServerNotification::SkillsChanged { .. } => {
            actions.push(ServerAction::InvalidateSkillsCatalog);
        }
        AppServerNotification::ConversationSwitched { conversation_id } => {
            actions.push(ServerAction::SwitchConversation(conversation_id.clone()));
        }
        AppServerNotification::Info { message, .. } => {
            push_notice(actions, "conversation", message, NoticeLevel::Info);
        }
        AppServerNotification::InterruptResult { disposition, .. } => {
            actions.push(ServerAction::InterruptResult(disposition.clone()));
        }
        AppServerNotification::TokenUsageUpdated {
            last_usage,
            total_usage,
            model_context_window,
            ..
        } => actions.push(ServerAction::SetTokenUsage {
            last_usage: last_usage.clone(),
            total_usage: total_usage.clone(),
            model_context_window: *model_context_window,
        }),
        AppServerNotification::ModelRetrying {
            stage,
            attempt,
            next_delay_ms,
            ..
        } => actions.push(ServerAction::SetRetryStatus {
            stage: stage.clone(),
            attempt: *attempt,
            next_delay_ms: *next_delay_ms,
        }),
        AppServerNotification::ContextCompacted {
            pre_context_tokens_estimate,
            post_context_tokens_estimate,
            ..
        } => {
            push_notice(
                actions,
                "context",
                &context_compacted_message(
                    *pre_context_tokens_estimate,
                    *post_context_tokens_estimate,
                ),
                NoticeLevel::Warn,
            );
            actions.push(ServerAction::ClearContextCompactionStatus);
        }
        AppServerNotification::ContextCompactionStarted {
            estimated_tokens, ..
        } => {
            actions.push(ServerAction::SetContextCompactionStatus {
                estimated_tokens: *estimated_tokens,
            });
        }
        AppServerNotification::Error { message, .. } => {
            actions.push(ServerAction::PushErrorCell(message.clone()));
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
            push_notice(
                actions,
                "request",
                &server_request_resolved_message(decision),
                NoticeLevel::Info,
            );
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
    }
}

fn push_notice(actions: &mut Vec<ServerAction>, label: &str, message: &str, level: NoticeLevel) {
    actions.push(ServerAction::PushNoticeCell {
        label: label.to_string(),
        message: message.to_string(),
        level,
    });
}

fn server_request_resolved_message(decision: &ServerRequestDecision) -> String {
    format!(
        "Request {}{}",
        decision.label(),
        decision
            .reason
            .as_deref()
            .map(|r| format!(": {r}"))
            .unwrap_or_default()
    )
}

fn context_compacted_message(
    pre_context_tokens_estimate: u64,
    post_context_tokens_estimate: u64,
) -> String {
    format!(
        "Context compacted: ~{} -> ~{} tokens",
        pre_context_tokens_estimate, post_context_tokens_estimate
    )
}

fn preview_excerpt(arguments_preview: &str) -> String {
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
