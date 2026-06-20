use super::*;
use serde_json::json;

#[test]
fn rollback_last_user_message_removes_matching_user_and_decrements_turn_count() {
    let mut history = ConversationHistory::new("conv".to_string(), "system".to_string());
    let user = history.push_user_message(vec![InputItem::Text {
        text: "hello".to_string(),
    }]);

    assert!(history.rollback_last_user_message(&user));
    assert_eq!(history.turn_count, 0);
    assert!(matches!(
        history.messages.as_slice(),
        [ResponseItem::System { .. }]
    ));
}

#[test]
fn rollback_last_user_message_ignores_non_matching_tail() {
    let mut history = ConversationHistory::new("conv".to_string(), "system".to_string());
    let user = history.push_user_message(vec![InputItem::Text {
        text: "hello".to_string(),
    }]);
    history.push_assistant_message(Some("ok".to_string()), None, Vec::new());

    assert!(!history.rollback_last_user_message(&user));
    assert_eq!(history.turn_count, 1);
}

#[test]
fn ensure_tool_outputs_present_inserts_aborted_output_for_missing_call() {
    let mut items = vec![ResponseItem::Assistant {
        content: None,
        reasoning: None,
        tool_calls: vec![ToolCall {
            id: "call_1".to_string(),
            name: "exec_command".to_string(),
            identity: crate::tool::ToolIdentity::built_in("exec_command"),
            arguments: json!({"command": "pwd"}),
        }],
    }];

    ensure_tool_outputs_present(&mut items);

    assert!(matches!(
        &items[..],
        [
            ResponseItem::Assistant { .. },
            ResponseItem::Tool {
                tool_call_id,
                name,
                content,
                structured,
            }
        ] if tool_call_id == "call_1"
            && name == "exec_command"
            && content == "aborted"
            && matches!(
                structured,
                Some(StructuredToolResult::ToolError { tool_name, message })
                    if tool_name == "exec_command" && message == "aborted"
            )
    ));
}

#[test]
fn ensure_tool_outputs_present_does_not_duplicate_existing_output() {
    let mut items = vec![
        ResponseItem::Assistant {
            content: None,
            reasoning: None,
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "exec_command".to_string(),
                identity: crate::tool::ToolIdentity::built_in("exec_command"),
                arguments: json!({"command": "pwd"}),
            }],
        },
        ResponseItem::Tool {
            tool_call_id: "call_1".to_string(),
            name: "exec_command".to_string(),
            content: "ok".to_string(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "pwd".to_string(),
                current_directory: "D:\\work".to_string(),
                session_id: None,
                status: crate::tool::CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                output: Some("D:\\work".to_string()),
                duration_ms: Some(1),
                original_token_count: Some(3),
                max_output_tokens: Some(10_000),
            }),
        },
    ];

    ensure_tool_outputs_present(&mut items);

    assert_eq!(items.len(), 2);
}
