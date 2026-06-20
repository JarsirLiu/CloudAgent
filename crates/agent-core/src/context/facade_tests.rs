use super::*;
use crate::tool::{CommandExecutionStatus, StructuredToolResult};
use crate::{ToolExecutionPolicy, ToolIdentity, TurnItemDeltaKind, TurnItemKind};
use serde_json::json;

#[test]
fn apply_pre_llm_filter_respects_policy_flag() {
    let facade = ContextFacade::new();
    let messages = vec![ResponseItem::Tool {
        tool_call_id: "call-1".to_string(),
        name: "exec_command".to_string(),
        content: "raw".to_string(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: "git status".to_string(),
            current_directory: "D:\\repo".to_string(),
            session_id: None,
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            success: Some(true),
            output: Some("modified: a.rs\nnew file: b.rs".to_string()),
            duration_ms: Some(1),
            original_token_count: Some(12),
            max_output_tokens: Some(10_000),
        }),
    }];

    let filtered = facade.apply_pre_llm_filter(
        messages.clone(),
        FilterPolicy { enabled: true },
        Path::new("D:\\repo"),
    );
    let unfiltered = facade.apply_pre_llm_filter(
        messages,
        FilterPolicy { enabled: false },
        Path::new("D:\\repo"),
    );

    match &filtered[0] {
        ResponseItem::Tool { content, .. } => assert!(content.starts_with("[rtk:git]")),
        _ => panic!("expected tool message"),
    }
    match &unfiltered[0] {
        ResponseItem::Tool { content, .. } => assert_eq!(content, "raw"),
        _ => panic!("expected tool message"),
    }
}

#[test]
fn estimate_history_tokens_for_compaction_respects_policy_flag() {
    let facade = ContextFacade::new();
    let messages = vec![ResponseItem::Tool {
        tool_call_id: "call-1".to_string(),
        name: "exec_command".to_string(),
        content: "raw".to_string(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: "git status".to_string(),
            current_directory: "D:\\repo".to_string(),
            session_id: None,
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            success: Some(true),
            output: Some("modified: a.rs\nnew file: b.rs".to_string()),
            duration_ms: Some(1),
            original_token_count: Some(12),
            max_output_tokens: Some(10_000),
        }),
    }];

    let filtered = facade.estimate_history_tokens_for_compaction(
        &messages,
        FilterPolicy { enabled: true },
        Path::new("D:\\repo"),
    );
    let unfiltered = facade.estimate_history_tokens_for_compaction(
        &messages,
        FilterPolicy { enabled: false },
        Path::new("D:\\repo"),
    );

    assert!(filtered > 0);
    assert!(unfiltered > 0);
    assert_ne!(filtered, unfiltered);
}

#[test]
fn estimate_model_request_tokens_counts_rendered_messages_and_tools() {
    let facade = ContextFacade::new();
    let request = ModelRequest {
        messages: vec![
            ResponseItem::System {
                content: "system".to_string(),
            },
            ResponseItem::User {
                content: crate::text_input_items("user"),
            },
        ],
        tools: vec![ToolSpec {
            name: "search_workspace".to_string(),
            identity: ToolIdentity::built_in("search_workspace"),
            description: "search repo".to_string(),
            parameters: json!({"type":"object","properties":{"query":{"type":"string"}}}),
            mutating: false,
            execution_policy: ToolExecutionPolicy::ParallelSafe,
            requires_approval: false,
            item_kind: TurnItemKind::ToolCall,
            delta_kind: TurnItemDeltaKind::ToolOutput,
            approval_reason: None,
        }],
        temperature: 0.0,
        reasoning_effort: None,
        tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
    };

    let estimated = facade.estimate_model_request_tokens(&request);
    assert!(estimated > facade.estimate_history_tokens(&request.messages));
}

#[test]
fn final_model_request_budget_flags_oversized_request() {
    let facade = ContextFacade::new();
    let request = ModelRequest {
        messages: vec![ResponseItem::User {
            content: crate::text_input_items("x".repeat(2_400)),
        }],
        tools: Vec::new(),
        temperature: 0.0,
        reasoning_effort: None,
        tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
    };

    let budget = facade.check_final_model_request_budget(&request, 512, 64);
    assert!(budget.exceeded);
    assert!(budget.estimated_tokens > budget.limit_tokens);
}
