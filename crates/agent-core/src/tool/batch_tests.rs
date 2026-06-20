use super::collect_exposed_tools;
use crate::{ConversationHistory, StructuredToolResult, ToolResult, ToolSearchHit, ToolSource};

#[test]
fn collect_exposed_tools_reads_tool_search_hits_from_history() {
    let mut history = ConversationHistory::new("test", "system");
    history.push_tool_result(ToolResult {
        tool_call_id: "call-1".to_string(),
        name: "tool_search".to_string(),
        content: "found".to_string(),
        is_error: false,
        structured: Some(StructuredToolResult::ToolSearch {
            query: "watch".to_string(),
            max_results: 8,
            match_count: 2,
            hits: vec![
                ToolSearchHit {
                    tool_name: "watch".to_string(),
                    source: ToolSource::BuiltIn,
                    description: "Watch workspace changes".to_string(),
                    mutating: false,
                    rank: 1,
                    match_reason: "tool name match".to_string(),
                },
                ToolSearchHit {
                    tool_name: "unwatch".to_string(),
                    source: ToolSource::BuiltIn,
                    description: "Stop watching workspace changes".to_string(),
                    mutating: false,
                    rank: 2,
                    match_reason: "tool description match".to_string(),
                },
            ],
        }),
    });

    let exposed = collect_exposed_tools(&history);

    assert_eq!(exposed, vec!["watch".to_string(), "unwatch".to_string()]);
}
