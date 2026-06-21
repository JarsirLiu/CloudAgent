use super::{ToolOperation, classify_tool_operation};
use agent_core::{StructuredToolResult, ToolIdentity};

#[test]
fn structured_result_takes_priority_over_tool_name() {
    let operation = classify_tool_operation(
        Some("tool"),
        Some(&StructuredToolResult::SearchWorkspace {
            session_id: "search-1".to_string(),
            operation: agent_core::SearchWorkspaceOperation::Search,
            mode: agent_core::SearchWorkspaceMode::Text,
            status: agent_core::SearchWorkspaceStatus::Active,
            query: "ToolRegistry".to_string(),
            path_scope: None,
            case_sensitive: false,
            context_lines: 0,
            max_results: 20,
            offset: 0,
            file_count: 1,
            match_count: 1,
            truncated: false,
            next_offset: None,
            hits: Vec::new(),
        }),
        None,
    );

    assert_eq!(operation, ToolOperation::Search);
}

#[test]
fn hosted_identity_classifies_as_external_without_name_heuristic() {
    let operation = classify_tool_operation(None, None, Some(&ToolIdentity::hosted("web_search")));

    assert_eq!(operation, ToolOperation::External);
}

#[test]
fn built_in_identity_uses_wire_name_mapping() {
    let operation = classify_tool_operation(None, None, Some(&ToolIdentity::built_in("edit_file")));

    assert_eq!(operation, ToolOperation::Edit);
}
