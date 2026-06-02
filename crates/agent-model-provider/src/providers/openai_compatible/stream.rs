use super::wire::ChatCompletionStreamChunk;
use crate::error::ProviderStreamError;
use crate::event::{
    ProviderCompletion, ProviderMetadata, ProviderReasoningDelta, ProviderStreamEvent,
    ProviderToolCallDelta,
};
use agent_core::ModelUsage;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Clone, Copy)]
pub(super) enum WireApi {
    Chat,
    Responses,
}

pub(super) struct ParsedStreamFrame {
    pub events: Vec<ProviderStreamEvent>,
    pub completion: Option<ProviderCompletion>,
    pub done: bool,
}

pub(super) struct ProviderEventStream {
    rx: mpsc::Receiver<Result<ProviderStreamEvent, ProviderStreamError>>,
}

impl ProviderEventStream {
    pub(super) fn new(
        rx: mpsc::Receiver<Result<ProviderStreamEvent, ProviderStreamError>>,
    ) -> Self {
        Self { rx }
    }

    pub(super) async fn recv(
        &mut self,
    ) -> Option<Result<ProviderStreamEvent, ProviderStreamError>> {
        self.rx.recv().await
    }
}

pub(super) fn parse_stream_frame(
    wire_api: WireApi,
    block: &str,
) -> Result<ParsedStreamFrame, ProviderStreamError> {
    match wire_api {
        WireApi::Chat => parse_chat_stream_frame(block),
        WireApi::Responses => parse_responses_stream_frame(block),
    }
}

fn parse_chat_stream_frame(block: &str) -> Result<ParsedStreamFrame, ProviderStreamError> {
    let (_, data) = parse_sse_block(block);
    let data = data.trim();
    if data.is_empty() {
        return Ok(ParsedStreamFrame {
            events: Vec::new(),
            completion: None,
            done: false,
        });
    }
    if data == "[DONE]" {
        return Ok(ParsedStreamFrame {
            events: Vec::new(),
            completion: None,
            done: true,
        });
    }

    let parsed: ChatCompletionStreamChunk =
        serde_json::from_str(data).map_err(|err| ProviderStreamError::Protocol {
            message: format!("failed to parse streaming chunk: {err}"),
        })?;

    let mut events = Vec::new();
    if !parsed.model.is_empty() {
        events.push(ProviderStreamEvent::Metadata(ProviderMetadata {
            model_name: Some(parsed.model.clone()),
        }));
    }
    if let Some(chunk_usage) = parsed.usage {
        events.push(ProviderStreamEvent::Usage(ModelUsage::from(chunk_usage)));
    }

    let mut completion = ProviderCompletion::default();
    let mut saw_completion = false;
    for choice in parsed.choices {
        if let Some(delta) = choice.delta.reasoning_content
            && !delta.is_empty()
        {
            events.push(ProviderStreamEvent::ReasoningDelta(
                ProviderReasoningDelta::Text {
                    content_index: 0,
                    delta,
                },
            ));
        }
        if let Some(delta) = choice.delta.content
            && !delta.is_empty()
        {
            events.push(ProviderStreamEvent::TextDelta(delta));
        }
        if let Some(delta_tool_calls) = choice.delta.tool_calls {
            for delta_call in delta_tool_calls {
                events.push(ProviderStreamEvent::ToolCallDelta(ProviderToolCallDelta {
                    index: delta_call.index,
                    id: delta_call.id,
                    name: delta_call
                        .function
                        .as_ref()
                        .and_then(|function| function.name.clone()),
                    arguments_delta: delta_call.function.and_then(|function| function.arguments),
                    arguments_replace: false,
                }));
            }
        }
        if let Some(finish_reason) = choice.finish_reason {
            completion.finish_reason = Some(finish_reason);
            saw_completion = true;
        }
    }

    Ok(ParsedStreamFrame {
        events,
        completion: saw_completion.then_some(completion),
        done: false,
    })
}

fn parse_responses_stream_frame(block: &str) -> Result<ParsedStreamFrame, ProviderStreamError> {
    let (event_name, data) = parse_sse_block(block);
    let data = data.trim();
    if data.is_empty() {
        return Ok(ParsedStreamFrame {
            events: Vec::new(),
            completion: None,
            done: false,
        });
    }
    let parsed: Value =
        serde_json::from_str(data).map_err(|err| ProviderStreamError::Protocol {
            message: format!("failed to parse responses event: {err}"),
        })?;

    let event_type = event_name.unwrap_or_else(|| {
        parsed
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
    });
    let mut events = Vec::new();
    let mut completion = None;
    let mut done = false;

    match event_type {
        "response.created" => {
            let response = parsed.get("response").unwrap_or(&parsed);
            if let Some(model_name) = response.get("model").and_then(|value| value.as_str()) {
                events.push(ProviderStreamEvent::Metadata(ProviderMetadata {
                    model_name: Some(model_name.to_string()),
                }));
            }
            if let Some(usage) = response.get("usage").cloned()
                && let Ok(usage) = serde_json::from_value::<super::wire::ResponsesUsage>(usage)
            {
                events.push(ProviderStreamEvent::Usage(ModelUsage::from(usage)));
            }
        }
        "response.output_text.delta" => {
            if let Some(delta) = parsed.get("delta").and_then(|value| value.as_str())
                && !delta.is_empty()
            {
                events.push(ProviderStreamEvent::TextDelta(delta.to_string()));
            }
        }
        "response.reasoning_summary_text.delta" => {
            if let Some(delta) = parsed.get("delta").and_then(|value| value.as_str())
                && !delta.is_empty()
            {
                let summary_index = parsed
                    .get("summary_index")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0) as usize;
                events.push(ProviderStreamEvent::ReasoningDelta(
                    ProviderReasoningDelta::SummaryText {
                        summary_index,
                        delta: delta.to_string(),
                    },
                ));
            }
        }
        "response.reasoning_text.delta" => {
            if let Some(delta) = parsed.get("delta").and_then(|value| value.as_str())
                && !delta.is_empty()
            {
                let content_index = parsed
                    .get("content_index")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0) as usize;
                events.push(ProviderStreamEvent::ReasoningDelta(
                    ProviderReasoningDelta::Text {
                        content_index,
                        delta: delta.to_string(),
                    },
                ));
            }
        }
        "response.function_call_arguments.delta" => {
            if let Some(delta) = parse_responses_tool_call_delta(&parsed) {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.custom_tool_call_input.delta" => {
            if let Some(delta) = parse_responses_tool_call_delta(&parsed) {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.function_call_arguments.done" => {
            if let Some(delta) = parse_responses_tool_call_done_delta(&parsed, "arguments") {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.custom_tool_call_input.done" => {
            if let Some(delta) = parse_responses_tool_call_done_delta(&parsed, "input") {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.output_item.added" => {
            if let Some(delta) = parse_responses_output_item_tool_call_delta(&parsed) {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.output_item.done" => {
            if let Some(delta) = parse_responses_output_item_done_tool_call_delta(&parsed) {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.completed" => {
            let response = parsed.get("response").unwrap_or(&parsed);
            completion = Some(ProviderCompletion {
                finish_reason: map_responses_finish_reason(response),
                end_turn: None,
            });
            if let Some(usage) = response.get("usage").cloned()
                && let Ok(usage) = serde_json::from_value::<super::wire::ResponsesUsage>(usage)
            {
                events.push(ProviderStreamEvent::Usage(ModelUsage::from(usage)));
            }
            done = true;
        }
        _ => {}
    }

    Ok(ParsedStreamFrame {
        events,
        completion,
        done,
    })
}

fn map_responses_finish_reason(response: &Value) -> Option<String> {
    let status = response
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    match status {
        "completed" => Some("stop".to_string()),
        "incomplete" => Some(
            response
                .get("incomplete_details")
                .and_then(|value| value.get("reason"))
                .and_then(|value| value.as_str())
                .unwrap_or("length")
                .to_string(),
        ),
        other if !other.is_empty() => Some(other.to_string()),
        _ => None,
    }
}

fn parse_responses_tool_call_delta(parsed: &Value) -> Option<ProviderToolCallDelta> {
    let index = parsed
        .get("output_index")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let id = parsed
        .get("item_id")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            parsed
                .get("call_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    let name = parsed
        .get("name")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            parsed
                .get("item")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    let arguments_delta = parsed
        .get("delta")
        .and_then(|value| value.as_str())
        .map(ToString::to_string);

    Some(ProviderToolCallDelta {
        index,
        id,
        name,
        arguments_delta,
        arguments_replace: false,
    })
}

fn parse_responses_output_item_tool_call_delta(parsed: &Value) -> Option<ProviderToolCallDelta> {
    let item = parsed.get("item")?;
    if !is_responses_tool_call_item(item) {
        return None;
    }

    let index = parsed
        .get("output_index")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let id = item
        .get("call_id")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            item.get("id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    let name = item
        .get("name")
        .and_then(|value| value.as_str())
        .map(ToString::to_string);

    Some(ProviderToolCallDelta {
        index,
        id,
        name,
        arguments_delta: None,
        arguments_replace: false,
    })
}

fn parse_responses_output_item_done_tool_call_delta(
    parsed: &Value,
) -> Option<ProviderToolCallDelta> {
    let item = parsed.get("item")?;
    if !is_responses_tool_call_item(item) {
        return None;
    }

    let index = parsed
        .get("output_index")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let id = item
        .get("call_id")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            item.get("id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    let name = item
        .get("name")
        .and_then(|value| value.as_str())
        .map(ToString::to_string);
    let arguments_delta = item
        .get("arguments")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            item.get("input").and_then(|value| match value {
                Value::String(text) => Some(text.clone()),
                other => serde_json::to_string(other).ok(),
            })
        });

    Some(ProviderToolCallDelta {
        index,
        id,
        name,
        arguments_delta,
        arguments_replace: true,
    })
}

fn is_responses_tool_call_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(|value| value.as_str()),
        Some("function_call" | "custom_tool_call" | "tool_search_call")
    )
}

fn parse_responses_tool_call_done_delta(
    parsed: &Value,
    field: &str,
) -> Option<ProviderToolCallDelta> {
    let index = parsed
        .get("output_index")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let id = parsed
        .get("item_id")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            parsed
                .get("call_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    let name = parsed
        .get("name")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            parsed
                .get("item")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        });
    let arguments_delta = parsed.get(field).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        other => serde_json::to_string(other).ok(),
    });

    Some(ProviderToolCallDelta {
        index,
        id,
        name,
        arguments_delta,
        arguments_replace: true,
    })
}

fn parse_sse_block(block: &str) -> (Option<&str>, String) {
    let mut event_name = None;
    let mut data_lines = Vec::new();

    for line in block.lines() {
        if let Some(rest) = strip_sse_field(line, "event") {
            event_name = Some(rest.trim());
        } else if let Some(rest) = strip_sse_field(line, "data") {
            data_lines.push(rest);
        }
    }

    (event_name, data_lines.join("\n"))
}

fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{field}: "))
        .or_else(|| line.strip_prefix(&format!("{field}:")))
}

#[cfg(test)]
mod tests {
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
    fn chat_event_only_frame_is_ignored() {
        let frame =
            parse_stream_frame(WireApi::Chat, "event: ping").expect("ignore event-only frame");

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
}
