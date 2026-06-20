use super::*;
use crate::conversation::ConversationHistory;
use crate::tool::{StructuredToolResult, ToolExecutionPolicy, ToolIdentity};
use crate::turn::{TurnItemDeltaKind, TurnItemKind};
use serde_json::json;

fn read_file_spec() -> ToolSpec {
    ToolSpec {
        name: "read_file".to_string(),
        identity: ToolIdentity::built_in("read_file"),
        description: String::new(),
        parameters: json!({}),
        mutating: false,
        execution_policy: ToolExecutionPolicy::Sequential,
        requires_approval: false,
        item_kind: TurnItemKind::ToolCall,
        delta_kind: TurnItemDeltaKind::ToolOutput,
        approval_reason: None,
    }
}

#[test]
fn trips_after_three_identical_read_only_roundtrips() {
    let mut history = ConversationHistory::new("conv".to_string(), "system".to_string());
    let tool_specs = vec![read_file_spec()];
    let tool_call = ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        identity: ToolIdentity::built_in("read_file"),
        arguments: json!({"path":"cli/src/ui/chat_surface.rs"}),
    };
    let tool_result = crate::tool::ToolResult {
        tool_call_id: "call-1".to_string(),
        name: "read_file".to_string(),
        content: "same output".to_string(),
        is_error: false,
        structured: Some(StructuredToolResult::ReadFile {
            path: "cli/src/ui/chat_surface.rs".to_string(),
            start_line: None,
            max_lines: None,
            total_chars: 8115,
            read: crate::tool::ReadFileEntry {
                path: "cli/src/ui/chat_surface.rs".to_string(),
                start_line: None,
                end_line: None,
                next_start_line: None,
                returned_line_count: 0,
                total_line_count: None,
                returned_char_count: 0,
                truncated: false,
                char_count: 8115,
                status: crate::tool::ReadFileStatus::Ok,
                version_token: None,
            },
        }),
    };
    let mut guard = LoopGuard::new();

    for index in 0..2 {
        let mut current_call = tool_call.clone();
        current_call.id = format!("call-{index}");
        let mut current_result = tool_result.clone();
        current_result.tool_call_id = current_call.id.clone();
        history.push_assistant_message(None, None, vec![current_call.clone()]);
        history.push_tool_result(current_result);
        assert!(
            guard
                .record_roundtrip(&[current_call], &tool_specs, &history.messages)
                .is_none()
        );
    }

    let mut final_call = tool_call.clone();
    final_call.id = "call-3".to_string();
    let mut final_result = tool_result;
    final_result.tool_call_id = final_call.id.clone();
    history.push_assistant_message(None, None, vec![final_call.clone()]);
    history.push_tool_result(final_result);
    assert_eq!(
        guard.record_roundtrip(&[final_call], &tool_specs, &history.messages),
        Some(LoopAbort { repeated_count: 3 })
    );
}
