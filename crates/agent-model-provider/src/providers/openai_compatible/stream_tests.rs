use super::*;

#[test]
fn done_frame_maps_to_completed_event() {
    let frame = parse_stream_frame(WireApi::Chat, "data: [DONE]").expect("parse done frame");
    assert!(frame.events.is_empty());
    assert!(frame.completion.is_none());
    assert!(frame.done);
}

#[test]
fn finish_reason_is_deferred_until_done() {
    let frame = parse_stream_frame(
        WireApi::Chat,
        r#"data: {"id":"resp_1","object":"chat.completion.chunk","created":0,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    )
    .expect("parse finish_reason frame");
    assert!(
        frame
            .events
            .iter()
            .all(|event| !matches!(event, ProviderStreamEvent::Completed(_)))
    );
    assert_eq!(
        frame.completion,
        Some(ProviderCompletion {
            finish_reason: Some("stop".to_string()),
            end_turn: None,
        })
    );
    assert!(!frame.done);
}

#[test]
fn reasoning_content_maps_to_reasoning_delta_event() {
    let frame = parse_stream_frame(
        WireApi::Chat,
        r#"data: {"id":"resp_1","object":"chat.completion.chunk","created":0,"model":"test-model","choices":[{"index":0,"delta":{"reasoning_content":"让我分析一下"}}]}"#,
    )
    .expect("parse reasoning frame");

    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ReasoningDelta(ProviderReasoningDelta::Text {
            content_index: 0,
            delta
        }) if delta == "让我分析一下"
    )));
}

#[test]
fn responses_text_delta_maps_to_text_event() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}",
    )
    .expect("parse responses delta");

    assert!(frame.events.iter().any(
        |event| matches!(event, ProviderStreamEvent::TextDelta(delta) if delta == "hello")
    ));
    assert!(!frame.done);
}

#[test]
fn responses_completed_finishes_stream() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}",
    )
    .expect("parse responses completed");

    assert!(frame.done);
    assert_eq!(
        frame.completion,
        Some(ProviderCompletion {
            finish_reason: Some("stop".to_string()),
            end_turn: None,
        })
    );
}

#[test]
fn responses_function_call_delta_maps_to_tool_call_event() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"item_id\":\"fc_1\",\"name\":\"read_file\",\"delta\":\"{\\\"path\\\":\\\"src/main.rs\\\"}\"}",
    )
    .expect("parse responses function call delta");

    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallDelta(ProviderToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
            arguments_replace
        }) if *index == 1
            && id.as_deref() == Some("fc_1")
            && name.as_deref() == Some("read_file")
            && arguments_delta.as_deref() == Some("{\"path\":\"src/main.rs\"}")
            && !arguments_replace
    )));
}

#[test]
fn responses_custom_tool_input_delta_maps_to_tool_call_event() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.custom_tool_call_input.delta\ndata: {\"type\":\"response.custom_tool_call_input.delta\",\"output_index\":2,\"item_id\":\"ct_1\",\"name\":\"open_panel\",\"delta\":\"{\\\"tab\\\":\\\"settings\\\"}\"}",
    )
    .expect("parse responses custom tool input delta");

    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallDelta(ProviderToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
            arguments_replace
        }) if *index == 2
            && id.as_deref() == Some("ct_1")
            && name.as_deref() == Some("open_panel")
            && arguments_delta.as_deref() == Some("{\"tab\":\"settings\"}")
            && !arguments_replace
    )));
}

#[test]
fn responses_output_item_added_maps_custom_tool_to_tool_call_event() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":3,\"item\":{\"type\":\"custom_tool_call\",\"call_id\":\"ct_2\",\"name\":\"do_thing\"}}",
    )
    .expect("parse responses output item added");

    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallDelta(ProviderToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
            arguments_replace
        }) if *index == 3
            && id.as_deref() == Some("ct_2")
            && name.as_deref() == Some("do_thing")
            && arguments_delta.is_none()
            && !arguments_replace
    )));
}

#[test]
fn responses_output_item_done_maps_tool_search_to_tool_call_event() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":4,\"item\":{\"type\":\"tool_search_call\",\"call_id\":\"search_1\",\"name\":\"tool_search\",\"arguments\":\"{\\\"query\\\":\\\"gmail\\\"}\"}}",
    )
    .expect("parse responses output item done");

    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallDelta(ProviderToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
            arguments_replace
        }) if *index == 4
            && id.as_deref() == Some("search_1")
            && name.as_deref() == Some("tool_search")
            && arguments_delta.as_deref() == Some("{\"query\":\"gmail\"}")
            && *arguments_replace
    )));
}

#[test]
fn responses_function_call_arguments_done_replaces_arguments() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.function_call_arguments.done\ndata: {\"type\":\"response.function_call_arguments.done\",\"output_index\":5,\"item_id\":\"fc_done\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"Cargo.toml\\\"}\"}",
    )
    .expect("parse responses function call arguments done");

    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::ToolCallDelta(ProviderToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
            arguments_replace
        }) if *index == 5
            && id.as_deref() == Some("fc_done")
            && name.as_deref() == Some("read_file")
            && arguments_delta.as_deref() == Some("{\"path\":\"Cargo.toml\"}")
            && *arguments_replace
    )));
}

#[test]
fn hosted_web_search_output_items_do_not_become_tool_calls() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"id\":\"ws_1\",\"status\":\"completed\"}}\n\n",
    )
    .expect("parse hosted web search frame");

    assert!(frame.events.iter().all(
        |event| !matches!(event, ProviderStreamEvent::ToolCallDelta(_))
    ));
    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::WebSearchCompleted(web_search)
            if web_search.id == "ws_1"
    )));
    assert!(frame.completion.is_none());
    assert!(!frame.done);
}

#[test]
fn hosted_web_search_added_maps_to_started_event() {
    let frame = parse_stream_frame(
        WireApi::Responses,
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"id\":\"ws_1\",\"status\":\"in_progress\"}}\n\n",
    )
    .expect("parse hosted web search added frame");

    assert!(frame.events.iter().all(
        |event| !matches!(event, ProviderStreamEvent::ToolCallDelta(_))
    ));
    assert!(frame.events.iter().any(|event| matches!(
        event,
        ProviderStreamEvent::WebSearchStarted(web_search)
            if web_search.id == "ws_1"
    )));
    assert!(frame.completion.is_none());
    assert!(!frame.done);
}

#[test]
fn chat_event_only_frame_is_ignored() {
    let frame = parse_stream_frame(WireApi::Chat, "event: ping").expect("ignore event-only frame");

    assert!(frame.events.is_empty());
    assert!(frame.completion.is_none());
    assert!(!frame.done);
}

#[test]
fn chat_empty_data_frame_is_ignored() {
    let frame = parse_stream_frame(WireApi::Chat, "data: ").expect("ignore empty data frame");

    assert!(frame.events.is_empty());
    assert!(frame.completion.is_none());
    assert!(!frame.done);
}

#[test]
fn responses_event_only_frame_is_ignored() {
    let frame = parse_stream_frame(WireApi::Responses, "event: response.ping")
        .expect("ignore event-only responses frame");

    assert!(frame.events.is_empty());
    assert!(frame.completion.is_none());
    assert!(!frame.done);
}

#[test]
fn parse_sse_block_joins_multiline_data_fields() {
    let (event_name, data) = parse_sse_block(
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\ndata: \"delta\":\"hello\"}",
    );

    assert_eq!(event_name, Some("response.output_text.delta"));
    assert_eq!(
        data,
        "{\"type\":\"response.output_text.delta\",\n\"delta\":\"hello\"}"
    );
}

#[test]
fn parse_sse_block_accepts_optional_space_after_field_name() {
    let (event_name, data) = parse_sse_block(
        "event:response.output_text.delta\ndata:{\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}",
    );

    assert_eq!(event_name, Some("response.output_text.delta"));
    assert_eq!(
        data,
        "{\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}"
    );
}
