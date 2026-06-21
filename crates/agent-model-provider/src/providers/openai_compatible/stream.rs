use super::wire::ChatCompletionStreamChunk;
use crate::error::ProviderStreamError;
use crate::event::{
    ProviderCompletion, ProviderMetadata, ProviderReasoningDelta, ProviderStreamEvent,
    ProviderToolCallDelta, ProviderWebSearch,
};
use agent_core::{WebSearchAction, WebSearchRecord};
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
            if let Some(web_search) = parse_responses_output_item_added_web_search(&parsed) {
                events.push(ProviderStreamEvent::WebSearchStarted(web_search));
            }
            if let Some(delta) = parse_responses_output_item_tool_call_delta(&parsed) {
                events.push(ProviderStreamEvent::ToolCallDelta(delta));
            }
        }
        "response.output_item.done" => {
            if let Some(web_search) = parse_responses_output_item_done_web_search(&parsed) {
                events.push(ProviderStreamEvent::WebSearchCompleted(web_search));
            }
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

fn parse_responses_output_item_added_web_search(parsed: &Value) -> Option<ProviderWebSearch> {
    let item = parsed.get("item")?;
    if item.get("type").and_then(|value| value.as_str()) != Some("web_search_call") {
        return None;
    }

    let id = item
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| item.get("call_id").and_then(|value| value.as_str()))?;
    let record = map_stream_web_search_record(item).unwrap_or(WebSearchRecord {
        id: id.to_string(),
        query: String::new(),
        action: None,
    });
    Some(ProviderWebSearch {
        id: record.id,
        query: record.query,
        action: record.action,
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

fn parse_responses_output_item_done_web_search(parsed: &Value) -> Option<ProviderWebSearch> {
    let item = parsed.get("item")?;
    if item.get("type").and_then(|value| value.as_str()) != Some("web_search_call") {
        return None;
    }

    let record = map_stream_web_search_record(item)?;
    Some(ProviderWebSearch {
        id: record.id,
        query: record.query,
        action: record.action,
    })
}

fn map_stream_web_search_record(item: &Value) -> Option<WebSearchRecord> {
    let id = item
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| item.get("call_id").and_then(|value| value.as_str()))?;
    let action = item.get("action").and_then(map_stream_web_search_action);
    let query = action
        .as_ref()
        .map(web_search_action_detail)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            item.get("query")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_default();
    Some(WebSearchRecord {
        id: id.to_string(),
        query,
        action,
    })
}

fn map_stream_web_search_action(value: &Value) -> Option<WebSearchAction> {
    let kind = value.get("type").and_then(|entry| entry.as_str())?;
    match kind {
        "search" => Some(WebSearchAction::Search {
            query: value
                .get("query")
                .and_then(|entry| entry.as_str())
                .map(ToString::to_string),
            queries: value.get("queries").and_then(|entry| {
                entry.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
            }),
        }),
        "open_page" => Some(WebSearchAction::OpenPage {
            url: value
                .get("url")
                .and_then(|entry| entry.as_str())
                .map(ToString::to_string),
        }),
        "find_in_page" => Some(WebSearchAction::FindInPage {
            url: value
                .get("url")
                .and_then(|entry| entry.as_str())
                .map(ToString::to_string),
            pattern: value
                .get("pattern")
                .and_then(|entry| entry.as_str())
                .map(ToString::to_string),
        }),
        "other" => Some(WebSearchAction::Other),
        _ => None,
    }
}

fn web_search_action_detail(action: &WebSearchAction) -> String {
    match action {
        WebSearchAction::Search { query, queries } => query
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                let first = queries
                    .as_ref()
                    .and_then(|values| values.first())
                    .cloned()
                    .unwrap_or_default();
                if queries.as_ref().is_some_and(|values| values.len() > 1) && !first.is_empty() {
                    format!("{first} ...")
                } else {
                    first
                }
            }),
        WebSearchAction::OpenPage { url } => url.clone().unwrap_or_default(),
        WebSearchAction::FindInPage { url, pattern } => match (pattern, url) {
            (Some(pattern), Some(url)) => format!("'{pattern}' in {url}"),
            (Some(pattern), None) => format!("'{pattern}'"),
            (None, Some(url)) => url.clone(),
            (None, None) => String::new(),
        },
        WebSearchAction::Other => String::new(),
    }
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
#[path = "stream_tests.rs"]
mod tests;
