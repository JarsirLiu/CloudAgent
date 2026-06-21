    use super::*;
    use agent_core::tool::ToolIdentity;
    use serde_json::json;

    fn tool_call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            identity: ToolIdentity::built_in("read_file"),
            arguments: json!({ "path": "src/main.rs" }),
        }
    }

    #[test]
    fn enrich_restores_missing_assistant_before_tool_result() {
        let history = RequestHistory::default();
        history.record_from_messages(&[
            ResponseItem::Assistant {
                content: None,
                reasoning: Some("Need file contents".to_string()),
                tool_calls: vec![tool_call("call_1")],
            },
            ResponseItem::Tool {
                tool_call_id: "call_1".to_string(),
                name: "read_file".to_string(),
                content: "fn main() {}".to_string(),
                structured: None,
            },
        ]);

        let request = ModelRequest {
            messages: vec![ResponseItem::Tool {
                tool_call_id: "call_1".to_string(),
                name: "read_file".to_string(),
                content: "fn main() {}".to_string(),
                structured: None,
            }],
            tools: Vec::new(),
            temperature: 0.0,
            reasoning_effort: None,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let enriched = history.enrich_chat_request(request);
        assert!(matches!(
            &enriched.messages[..],
            [
                ResponseItem::Assistant { tool_calls, .. },
                ResponseItem::Tool { tool_call_id, .. }
            ] if tool_calls.iter().any(|call| call.id == "call_1") && tool_call_id == "call_1"
        ));
    }

    #[test]
    fn enrich_inserts_aborted_output_for_missing_tool_result() {
        let history = RequestHistory::default();
        let enriched = history.enrich_chat_request(ModelRequest {
            messages: vec![ResponseItem::Assistant {
                content: None,
                reasoning: None,
                tool_calls: vec![tool_call("call_1")],
            }],
            tools: Vec::new(),
            temperature: 0.0,
            reasoning_effort: None,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        });

        assert!(matches!(
            &enriched.messages[..],
            [
                ResponseItem::Assistant { .. },
                ResponseItem::Tool { tool_call_id, content, .. }
            ] if tool_call_id == "call_1" && content == "aborted"
        ));
    }
