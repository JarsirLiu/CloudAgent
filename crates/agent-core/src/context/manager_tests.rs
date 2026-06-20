use super::*;
use crate::context::EnvironmentContext;
use crate::{input_items_to_plain_text, text_input_items};

#[test]
fn contextual_fragments_are_model_request_only_and_before_latest_user() {
    let mut manager = ContextManager::new("default", "system");
    manager.record_user_message(crate::text_input_items("hello"));

    let environment = EnvironmentContext::new(
        r"D:\learn\gifti\cloudagent",
        "powershell",
        "2026-04-30",
        "19:16:01",
        "2026-04-30T19:16:01+08:00",
        "+08:00",
    );
    let request =
        manager.build_current_model_request_with_fragments(&[environment], Vec::new(), 0.0);

    assert_eq!(manager.history().messages.len(), 2);
    assert_eq!(request.messages.len(), 3);
    assert!(matches!(request.messages[0], ResponseItem::System { .. }));
    assert!(
        matches!(request.messages[1], ResponseItem::User { ref content } if input_items_to_plain_text(content).starts_with("<environment_context>"))
    );
    assert!(
        matches!(request.messages[2], ResponseItem::User { ref content } if content == &text_input_items("hello"))
    );
}

#[test]
fn contextual_fragments_insert_before_latest_real_user_after_compaction_summary() {
    let history = ConversationHistory {
        id: "default".to_string(),
        turn_count: 1,
        messages: vec![
            ResponseItem::System {
                content: "system".to_string(),
            },
            ResponseItem::System {
                content: "[Context Summary]\nprevious work".to_string(),
            },
            ResponseItem::User {
                content: text_input_items("latest user"),
            },
            ResponseItem::Assistant {
                content: Some("latest assistant".to_string()),
                reasoning: None,
                tool_calls: Vec::new(),
            },
        ],
    };
    let manager = ContextManager::from_history(history);
    let environment = EnvironmentContext::new(
        r"D:\learn\gifti\cloudagent",
        "powershell",
        "2026-04-30",
        "19:16:01",
        "2026-04-30T19:16:01+08:00",
        "+08:00",
    );

    let request =
        manager.build_current_model_request_with_fragments(&[environment], Vec::new(), 0.0);

    assert!(matches!(
        &request.messages[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::System { content: summary },
            ResponseItem::User { content: env },
            ResponseItem::User { content: latest_user },
            ResponseItem::Assistant { content: Some(latest_assistant), .. },
        ] if system == "system"
            && summary == "[Context Summary]\nprevious work"
            && input_items_to_plain_text(env).starts_with("<environment_context>")
            && latest_user == &text_input_items("latest user")
            && latest_assistant == "latest assistant"
    ));
}
