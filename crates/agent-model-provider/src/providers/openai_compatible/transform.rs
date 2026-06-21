use super::wire::{
    ChatApiMessage, ChatCompletionRequest, ChatCompletionResponse, ChatCompletionStreamOptions,
    ChatToolSpec, ResponsesRequest, ResponsesResponse, ResponsesToolSpec,
};
use crate::request::{ProviderMessage, ProviderRequest};
use agent_core::model::{ModelResponse, ModelUsage};
use agent_core::tool::{ToolCall, ToolIdentity, ToolSpec};
use agent_core::{WebSearchAction, WebSearchRecord};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct StreamingToolCallAcc {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: String,
}

pub(super) fn tool_spec_index(tools: &[ToolSpec]) -> HashMap<String, ToolSpec> {
    tools
        .iter()
        .cloned()
        .map(|spec| (spec.identity.wire_name.clone(), spec))
        .collect()
}

pub(super) fn build_chat_request(
    provider_request: &ProviderRequest,
    model: &str,
    stream: bool,
) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: model.to_string(),
        messages: provider_request
            .messages
            .iter()
            .map(ChatApiMessage::from_message)
            .collect::<Vec<_>>(),
        tools: provider_request
            .tools
            .iter()
            .filter(|spec| spec.identity.wire_name != "web_search")
            .map(ChatToolSpec::from_spec)
            .collect(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: true,
        temperature: provider_request.temperature,
        reasoning_effort: provider_request.reasoning_effort.clone(),
        stream: stream.then_some(true),
        stream_options: stream.then_some(ChatCompletionStreamOptions {
            include_usage: true,
        }),
    }
}

pub(super) fn build_responses_request(
    provider_request: &ProviderRequest,
    model: &str,
    stream: bool,
) -> ResponsesRequest {
    let mut instructions = Vec::new();
    let mut input = Vec::new();

    for message in &provider_request.messages {
        match message {
            ProviderMessage::System { content } => {
                if !content.trim().is_empty() {
                    instructions.push(content.trim().to_string());
                }
            }
            ProviderMessage::User { content } => input.push(json!({
                "type": "message",
                "role": "user",
                "content": responses_user_content(content),
            })),
            ProviderMessage::Assistant {
                content,
                reasoning,
                tool_calls,
            } => {
                if let Some(reasoning) = reasoning
                    && !reasoning.trim().is_empty()
                {
                    input.push(json!({
                        "type": "reasoning",
                        "summary": [{
                            "type": "summary_text",
                            "text": reasoning,
                        }],
                    }));
                }

                if let Some(content) = content
                    && !content.trim().is_empty()
                {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": content,
                        }],
                    }));
                }

                for call in tool_calls {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.identity.wire_name,
                        "arguments": call.arguments.to_string(),
                    }));
                }
            }
            ProviderMessage::Tool {
                tool_call_id,
                content,
                ..
            } => input.push(json!({
                "type": "function_call_output",
                "call_id": tool_call_id,
                "output": content,
            })),
        }
    }

    ResponsesRequest {
        model: model.to_string(),
        instructions: (!instructions.is_empty()).then_some(instructions.join("\n\n")),
        input,
        tools: provider_request
            .tools
            .iter()
            .map(ResponsesToolSpec::from_spec)
            .collect(),
        temperature: provider_request.temperature,
        reasoning: provider_request
            .reasoning_effort
            .as_ref()
            .map(|effort| json!({ "effort": effort })),
        stream: stream.then_some(true),
    }
}

pub(super) fn parse_chat_response(
    parsed: ChatCompletionResponse,
    tool_spec_index: &HashMap<String, ToolSpec>,
) -> Result<ModelResponse> {
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("LLM response contained no choices"))?;

    Ok(ModelResponse {
        content: choice.message.content,
        reasoning: choice.message.reasoning_content,
        tool_calls: map_tool_calls(
            choice.message.tool_calls.unwrap_or_default(),
            tool_spec_index,
        ),
        web_searches: Vec::new(),
        finish_reason: choice.finish_reason,
        model_name: Some(parsed.model),
        usage: parsed.usage.map(ModelUsage::from),
    })
}

pub(super) fn parse_responses_response(
    parsed: ResponsesResponse,
    tool_spec_index: &HashMap<String, ToolSpec>,
) -> ModelResponse {
    let mut content_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut web_searches = Vec::new();

    for item in parsed.output {
        match item.get("type").and_then(|value| value.as_str()) {
            Some("message") => {
                if let Some(content) = item.get("content").and_then(|value| value.as_array()) {
                    for part in content {
                        match part.get("type").and_then(|value| value.as_str()) {
                            Some("output_text") | Some("text") => {
                                if let Some(text) =
                                    part.get("text").and_then(|value| value.as_str())
                                {
                                    content_parts.push(text.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some("reasoning") => {
                reasoning_parts.extend(extract_reasoning_texts(&item));
            }
            Some("function_call") => {
                if let Some(call) = map_responses_tool_call_item(&item, tool_spec_index) {
                    tool_calls.push(call);
                }
            }
            Some("custom_tool_call") => {
                if let Some(call) = map_responses_tool_call_item(&item, tool_spec_index) {
                    tool_calls.push(call);
                }
            }
            Some("tool_search_call") => {
                if let Some(call) = map_responses_tool_call_item(&item, tool_spec_index) {
                    tool_calls.push(call);
                }
            }
            Some("web_search_call") => {
                if let Some(record) = map_responses_web_search_item(&item) {
                    web_searches.push(record);
                }
            }
            _ => {}
        }
    }

    ModelResponse {
        content: (!content_parts.is_empty()).then_some(content_parts.join("")),
        reasoning: (!reasoning_parts.is_empty()).then_some(reasoning_parts.join("")),
        tool_calls,
        web_searches,
        finish_reason: responses_finish_reason(&parsed.status, parsed.incomplete_details.as_ref()),
        model_name: Some(parsed.model),
        usage: parsed.usage.map(ModelUsage::from),
    }
}

pub(super) fn finalize_stream_tool_calls(
    tool_calls_acc: HashMap<usize, StreamingToolCallAcc>,
    tool_spec_index: &HashMap<String, ToolSpec>,
) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();
    let mut ordered: Vec<(usize, StreamingToolCallAcc)> = tool_calls_acc.into_iter().collect();
    ordered.sort_by_key(|(index, _)| *index);
    for (_, acc) in ordered {
        if acc.id.is_empty() || acc.name.is_empty() {
            continue;
        }
        let arguments = serde_json::from_str::<Value>(&acc.arguments)
            .unwrap_or_else(|_| Value::String(acc.arguments.clone()));
        tool_calls.push(map_tool_call(acc.id, acc.name, arguments, tool_spec_index));
    }
    tool_calls
}

fn map_tool_calls(
    calls: Vec<super::wire::ChatToolCall>,
    tool_spec_index: &HashMap<String, ToolSpec>,
) -> Vec<ToolCall> {
    calls
        .into_iter()
        .map(|call| {
            let arguments = serde_json::from_str::<Value>(&call.function.arguments)
                .unwrap_or_else(|_| Value::String(call.function.arguments.clone()));
            map_tool_call(call.id, call.function.name, arguments, tool_spec_index)
        })
        .collect()
}

fn map_tool_call(
    id: String,
    wire_name: String,
    arguments: Value,
    tool_spec_index: &HashMap<String, ToolSpec>,
) -> ToolCall {
    let fallback_identity = infer_tool_identity_from_wire_name(&wire_name);
    let fallback_name = infer_tool_name_from_wire_name(&wire_name);
    ToolCall {
        id,
        name: tool_spec_index
            .get(&wire_name)
            .map(|spec| spec.name.clone())
            .unwrap_or(fallback_name),
        identity: tool_spec_index
            .get(&wire_name)
            .map(|spec| spec.identity.clone())
            .unwrap_or(fallback_identity),
        arguments,
    }
}

fn map_responses_tool_call_item(
    item: &Value,
    tool_spec_index: &HashMap<String, ToolSpec>,
) -> Option<ToolCall> {
    let id = item.get("call_id").and_then(|value| value.as_str())?;
    let wire_name = item.get("name").and_then(|value| value.as_str())?;
    let arguments = item
        .get("arguments")
        .cloned()
        .or_else(|| item.get("input").cloned())
        .unwrap_or_else(|| Value::Object(Default::default()));
    let arguments = match arguments {
        Value::String(raw) => serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw)),
        other => other,
    };
    Some(map_tool_call(
        id.to_string(),
        wire_name.to_string(),
        arguments,
        tool_spec_index,
    ))
}

fn responses_user_content(content: &[agent_core::conversation::InputItem]) -> Vec<Value> {
    content
        .iter()
        .map(|item| match item {
            agent_core::conversation::InputItem::Text { text } => json!({
                "type": "input_text",
                "text": text,
            }),
            agent_core::conversation::InputItem::Image { source, detail, alt } => json!({
                "type": "input_image",
                "image_url": match source {
                    agent_core::conversation::AttachmentRef::InlineDataUrl { data_url } => data_url,
                    agent_core::conversation::AttachmentRef::RemoteUrl { url } => url,
                    agent_core::conversation::AttachmentRef::HubAsset { download_url, .. } => download_url.as_deref().unwrap_or(""),
                    agent_core::conversation::AttachmentRef::LocalPath { .. } => "",
                },
                "detail": detail.as_ref().map(|value| match value {
                    agent_core::conversation::ImageDetail::Auto => "auto",
                    agent_core::conversation::ImageDetail::Low => "low",
                    agent_core::conversation::ImageDetail::High => "high",
                    agent_core::conversation::ImageDetail::Original => "high",
                }),
                "alt": alt,
            }),
            agent_core::conversation::InputItem::File { name, mime_type, .. } => json!({
                "type": "input_text",
                "text": format!(
                    "[file: {}{}]",
                    name.clone().unwrap_or_else(|| "attachment".to_string()),
                    mime_type
                        .as_ref()
                        .map(|mime| format!(" ({mime})"))
                        .unwrap_or_default()
                ),
            }),
            agent_core::conversation::InputItem::Mention { name, path } => json!({
                "type": "input_text",
                "text": format!("@{name} ({path})"),
            }),
            agent_core::conversation::InputItem::Skill { name, path } => json!({
                "type": "input_text",
                "text": format!("${name} ({path})"),
            }),
        })
        .collect()
}

fn extract_reasoning_texts(item: &Value) -> Vec<String> {
    let mut texts = Vec::new();

    if let Some(summary) = item.get("summary").and_then(|value| value.as_array()) {
        for part in summary {
            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                texts.push(text.to_string());
            }
        }
    }

    if let Some(content) = item.get("content").and_then(|value| value.as_array()) {
        for part in content {
            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                texts.push(text.to_string());
            }
        }
    }

    texts
}

fn map_responses_web_search_item(item: &Value) -> Option<WebSearchRecord> {
    let id = item
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| item.get("call_id").and_then(|value| value.as_str()))?;
    let action = item.get("action").and_then(map_responses_web_search_action);
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

fn map_responses_web_search_action(value: &Value) -> Option<WebSearchAction> {
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

fn responses_finish_reason(status: &str, incomplete_details: Option<&Value>) -> Option<String> {
    match status {
        "completed" => Some("stop".to_string()),
        "incomplete" => Some(
            incomplete_details
                .and_then(|value| value.get("reason"))
                .and_then(|value| value.as_str())
                .unwrap_or("length")
                .to_string(),
        ),
        other if !other.is_empty() => Some(other.to_string()),
        _ => None,
    }
}

fn infer_tool_identity_from_wire_name(wire_name: &str) -> ToolIdentity {
    if let Some((server, tool)) = parse_mcp_wire_name(wire_name) {
        ToolIdentity::mcp(server, tool, wire_name.to_string())
    } else {
        ToolIdentity::built_in(wire_name.to_string())
    }
}

fn parse_mcp_wire_name(wire_name: &str) -> Option<(String, String)> {
    let rest = wire_name.strip_prefix("mcp__")?;
    if let Some((namespace, tool)) = rest.rsplit_once("___")
        && !namespace.is_empty()
        && !tool.is_empty()
    {
        return Some((namespace.to_string(), tool.to_string()));
    }
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}

fn infer_tool_name_from_wire_name(wire_name: &str) -> String {
    if let Some((_, tool)) = parse_mcp_wire_name(wire_name) {
        tool
    } else {
        wire_name.to_string()
    }
}

#[cfg(test)]
#[path = "transform_tests.rs"]
mod tests;
