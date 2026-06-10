use crate::node::runtime::NodeRuntime;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, ConversationListPageResponse,
    JsonRpcMessage,
};
use anyhow::Result;

pub(super) async fn notification_for_command(
    rpc: &JsonRpcMessage,
    command: &AppClientCommand,
    active_conversation_id: &str,
    runtime: &NodeRuntime,
) -> Option<AppServerMessage> {
    if !matches!(rpc, JsonRpcMessage::Notification(_)) {
        return None;
    }

    match command {
        AppClientCommand::ListConversationsPage { cursor, limit } => {
            Some(notification_from_page_response(
                active_conversation_id,
                read_conversation_list_page(runtime, cursor.clone(), *limit)
                    .await
                    .ok()?,
            ))
        }
        _ => None,
    }
}

pub(super) async fn read_conversation_list_page(
    runtime: &NodeRuntime,
    cursor: Option<String>,
    limit: usize,
) -> Result<ConversationListPageResponse> {
    let _ = runtime
        .conversation_store()
        .reconcile_missing_conversations(100)
        .await;
    match runtime
        .conversation_store()
        .list_conversations_page(cursor, limit)
        .await
    {
        Ok(page) => {
            runtime
                .conversations()
                .lock()
                .await
                .merge_from_summaries(&page.conversations);
            Ok(ConversationListPageResponse {
                conversations: page.conversations,
                has_more: page.has_more,
                next_cursor: page.next_cursor,
            })
        }
        Err(_) => {
            let conversations = runtime.conversations().lock().await.summaries();
            Ok(ConversationListPageResponse {
                conversations,
                has_more: false,
                next_cursor: None,
            })
        }
    }
}

pub(super) fn notification_from_page_response(
    conversation_id: &str,
    response: ConversationListPageResponse,
) -> AppServerMessage {
    AppServerMessage::Notification(AppServerNotification::ConversationListPage {
        conversation_id: conversation_id.to_string(),
        conversations: response.conversations,
        has_more: response.has_more,
        next_cursor: response.next_cursor,
    })
}

#[cfg(test)]
#[path = "session_listing_tests.rs"]
mod tests;
