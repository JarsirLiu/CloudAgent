    use super::*;
    use agent_core::tool::ToolSource;
    use serde_json::json;

    #[test]
    fn infers_mcp_identity_from_wire_name_without_registered_spec() {
        let index = HashMap::new();
        let call = map_tool_call(
            "call_1".to_string(),
            "mcp__demo__lookup".to_string(),
            json!({"query":"abc"}),
            &index,
        );

        assert_eq!(call.identity.source, ToolSource::Mcp);
        assert_eq!(call.identity.namespace.as_deref(), Some("demo"));
        assert_eq!(call.identity.wire_name, "mcp__demo__lookup");
        assert_eq!(call.name, "lookup");
    }

    #[test]
    fn keeps_builtin_identity_for_non_mcp_wire_name() {
        let index = HashMap::new();
        let call = map_tool_call(
            "call_1".to_string(),
            "exec_command".to_string(),
            json!({"command":"pwd"}),
            &index,
        );

        assert_eq!(call.identity.source, ToolSource::BuiltIn);
        assert_eq!(call.identity.namespace, None);
        assert_eq!(call.identity.wire_name, "exec_command");
    }

    #[test]
    fn infers_flattened_namespace_mcp_identity_from_wire_name() {
        let index = HashMap::new();
        let call = map_tool_call(
            "call_1".to_string(),
            "mcp__codex_apps__gmail___search_emails".to_string(),
            json!({"query":"inbox"}),
            &index,
        );

        assert_eq!(call.identity.source, ToolSource::Mcp);
        assert_eq!(
            call.identity.namespace.as_deref(),
            Some("codex_apps__gmail")
        );
        assert_eq!(
            call.identity.wire_name,
            "mcp__codex_apps__gmail___search_emails"
        );
        assert_eq!(call.name, "search_emails");
    }

    #[test]
    fn parses_custom_tool_call_items_from_responses_output() {
        let index = HashMap::new();
        let response = ResponsesResponse {
            model: "test-model".to_string(),
            output: vec![json!({
                "type": "custom_tool_call",
                "call_id": "call_1",
                "name": "open_panel",
                "input": "settings"
            })],
            status: "completed".to_string(),
            incomplete_details: None,
            usage: None,
        };

        let parsed = parse_responses_response(response, &index);

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].name, "open_panel");
        assert_eq!(parsed.tool_calls[0].arguments, json!("settings"));
    }

    #[test]
    fn parses_tool_search_call_items_from_responses_output() {
        let index = HashMap::new();
        let response = ResponsesResponse {
            model: "test-model".to_string(),
            output: vec![json!({
                "type": "tool_search_call",
                "call_id": "call_1",
                "name": "tool_search",
                "arguments": {
                    "query": "gmail",
                    "limit": 5
                }
            })],
            status: "completed".to_string(),
            incomplete_details: None,
            usage: None,
        };

        let parsed = parse_responses_response(response, &index);

        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].name, "tool_search");
        assert_eq!(
            parsed.tool_calls[0].arguments,
            json!({"query":"gmail","limit":5})
        );
    }

    #[test]
    fn ignores_hosted_web_search_call_items_from_responses_output() {
        let index = HashMap::new();
        let response = ResponsesResponse {
            model: "test-model".to_string(),
            output: vec![
                json!({
                    "type": "web_search_call",
                    "id": "ws_1",
                    "status": "completed"
                }),
                json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "Latest result."
                    }]
                }),
            ],
            status: "completed".to_string(),
            incomplete_details: None,
            usage: None,
        };

        let parsed = parse_responses_response(response, &index);

        assert!(parsed.tool_calls.is_empty());
        assert_eq!(parsed.content.as_deref(), Some("Latest result."));
    }
