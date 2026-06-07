use super::sync_registry_from_message;
use crate::node::platform::PlatformManager;
use crate::node::runtime::NodeRuntime;
use crate::node::test_support::{test_worker_program, unique_temp_path};
use crate::node::worker_manager::WorkerManager;
use agent_core::{SkillRuntime, conversation::ConversationSummary};
use agent_protocol::{
    AppServerMessage, AppServerNotification, ConversationViewSnapshot, ConversationViewStatus,
};

async fn test_runtime() -> NodeRuntime {
    let root = unique_temp_path("cloudagent-node-platform-tests");
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

#[test]
fn worker_conversation_list_replaces_shared_registry_state() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let runtime = test_runtime().await;
        runtime.conversations().lock().await.touch("stale");

        sync_registry_from_message(
            &runtime,
            &AppServerMessage::Notification(AppServerNotification::ConversationList {
                conversation_id: "conversation-1".to_string(),
                conversations: vec![ConversationSummary {
                    conversation_id: "conversation-1".to_string(),
                    title: Some("Alpha".to_string()),
                    message_count: 4,
                    updated_at_ms: 12,
                }],
            }),
        )
        .await;

        let summaries = runtime.conversations().lock().await.summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].conversation_id, "conversation-1");
        assert_eq!(summaries[0].title.as_deref(), Some("Alpha"));
        assert_eq!(summaries[0].message_count, 4);
    });
}

#[test]
fn conversation_view_notifications_update_busy_state() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let runtime = test_runtime().await;

        sync_registry_from_message(
            &runtime,
            &AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
                conversation_id: "conversation-1".to_string(),
                snapshot: ConversationViewSnapshot {
                    conversation_id: "conversation-1".to_string(),
                    status: ConversationViewStatus::Active {
                        active_turn_id: Some("turn-1".to_string()),
                        flags: Vec::new(),
                    },
                    active_turn: None,
                    pending_requests: Vec::new(),
                    message_count: 1,
                    updated_at_ms: 0,
                },
            }),
        )
        .await;

        assert!(runtime.is_conversation_busy("conversation-1").await);

        sync_registry_from_message(
            &runtime,
            &AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
                conversation_id: "conversation-1".to_string(),
                snapshot: ConversationViewSnapshot {
                    conversation_id: "conversation-1".to_string(),
                    status: ConversationViewStatus::Idle,
                    active_turn: None,
                    pending_requests: Vec::new(),
                    message_count: 1,
                    updated_at_ms: 0,
                },
            }),
        )
        .await;

        assert!(!runtime.is_conversation_busy("conversation-1").await);
    });
}

#[test]
fn terminal_turn_notifications_do_not_update_busy_state() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let runtime = test_runtime().await;

        sync_registry_from_message(
            &runtime,
            &AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
                conversation_id: "conversation-1".to_string(),
                snapshot: ConversationViewSnapshot {
                    conversation_id: "conversation-1".to_string(),
                    status: ConversationViewStatus::Active {
                        active_turn_id: Some("turn-1".to_string()),
                        flags: Vec::new(),
                    },
                    active_turn: None,
                    pending_requests: Vec::new(),
                    message_count: 1,
                    updated_at_ms: 0,
                },
            }),
        )
        .await;

        sync_registry_from_message(
            &runtime,
            &AppServerMessage::Notification(AppServerNotification::TurnCompleted {
                conversation_id: "conversation-1".to_string(),
                turn_id: "turn-1".to_string(),
            }),
        )
        .await;

        assert!(runtime.is_conversation_busy("conversation-1").await);
    });
}
