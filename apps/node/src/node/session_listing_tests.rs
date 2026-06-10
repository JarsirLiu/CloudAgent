use super::{
    notification_for_command, notification_from_page_response, read_conversation_list_page,
};
use crate::node::platform::PlatformManager;
use crate::node::runtime::NodeRuntime;
use crate::node::test_support::{test_worker_program, unique_temp_path};
use crate::node::worker_manager::WorkerManager;
use agent_core::{EventMsg, SkillRuntime, rollout::RolloutItem};
use agent_protocol::{AppClientCommand, AppServerMessage, AppServerNotification, JsonRpcMessage};

async fn test_runtime() -> NodeRuntime {
    let root = unique_temp_path("cloudagent-node-session-listing");
    NodeRuntime::new(
        WorkerManager::new(test_worker_program(), None),
        infra_store::JsonConversationStore::new(root.join("conversations")),
        PlatformManager::load(Some(root.as_os_str()))
            .await
            .expect("platform manager"),
        "127.0.0.1:47070",
        root.clone(),
        SkillRuntime::new(true, Vec::new()),
        root,
    )
}

fn turn_started(conversation_id: &str, turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg {
        event: EventMsg::TurnStarted {
            turn_id: turn_id.to_string(),
            conversation_id: conversation_id.to_string(),
            user_input: Vec::new(),
        },
    }
}

#[test]
fn page_reads_merge_into_registry() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let runtime = test_runtime().await;
        for id in ["session-a", "session-b", "session-c"] {
            runtime
                .conversation_store()
                .append_rollout_items(id, &[turn_started(id, &format!("turn-{id}"))])
                .await
                .expect("append rollout");
        }

        let first = read_conversation_list_page(&runtime, None, 2)
            .await
            .expect("first page");
        let cursor = first.next_cursor.clone().expect("cursor");
        let second = read_conversation_list_page(&runtime, Some(cursor), 2)
            .await
            .expect("second page");

        assert_eq!(first.conversations.len(), 2);
        assert_eq!(second.conversations.len(), 1);
        assert_eq!(runtime.conversations().lock().await.summaries().len(), 3);
    });
}

#[test]
fn notification_path_emits_page_notification() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let runtime = test_runtime().await;
        runtime
            .conversation_store()
            .append_rollout_items("session-a", &[turn_started("session-a", "turn-a")])
            .await
            .expect("append rollout");

        let message = notification_for_command(
            &JsonRpcMessage::Notification(agent_protocol::JsonRpcNotification {
                method: "conversation/listPage".to_string(),
                params: Some(serde_json::json!({})),
            }),
            &AppClientCommand::ListConversationsPage {
                cursor: None,
                limit: 25,
            },
            "active",
            &runtime,
        )
        .await
        .expect("page notification");

        match message {
            AppServerMessage::Notification(AppServerNotification::ConversationListPage {
                conversations,
                ..
            }) => assert_eq!(conversations.len(), 1),
            other => panic!("unexpected message: {other:?}"),
        }
    });
}

#[test]
fn notification_helpers_preserve_page_cursor() {
    let message = notification_from_page_response(
        "active",
        agent_protocol::ConversationListPageResponse {
            conversations: Vec::new(),
            has_more: true,
            next_cursor: Some("cursor-1".to_string()),
        },
    );

    match message {
        AppServerMessage::Notification(AppServerNotification::ConversationListPage {
            next_cursor,
            has_more,
            ..
        }) => {
            assert!(has_more);
            assert_eq!(next_cursor.as_deref(), Some("cursor-1"));
        }
        other => panic!("unexpected message: {other:?}"),
    }
}
