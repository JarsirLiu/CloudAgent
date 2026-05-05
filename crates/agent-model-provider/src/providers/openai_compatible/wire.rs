use crate::request::ProviderMessage;
use agent_core::{ModelUsage, ToolCall, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub(super) struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatApiMessage>,
    pub tools: Vec<ChatToolSpec>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<ChatCompletionStreamOptions>,
}

#[derive(Serialize)]
pub(super) struct ChatCompletionStreamOptions {
    pub include_usage: bool,
}

#[derive(Serialize)]
pub(super) struct ChatApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl ChatApiMessage {
    pub(super) fn from_message(message: &ProviderMessage) -> Self {
        match message {
            ProviderMessage::System { content } => Self {
                role: "system".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ProviderMessage::User { content } => Self {
                role: "user".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ProviderMessage::Assistant {
                content,
                tool_calls,
            } => Self {
                role: "assistant".to_string(),
                content: content.clone(),
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls
                            .iter()
                            .map(ChatToolCall::from_tool_call)
                            .collect(),
                    )
                },
                tool_call_id: None,
                name: None,
            },
            ProviderMessage::Tool {
                tool_call_id,
                name,
                content,
            } => Self {
                role: "tool".to_string(),
                content: Some(tool_message_content(name, content)),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
                name: Some(name.clone()),
            },
        }
    }
}

fn tool_message_content(_name: &str, content: &str) -> String {
    content.to_string()
}

#[derive(Serialize)]
pub(super) struct ChatToolSpec {
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolFunctionSpec,
}

impl ChatToolSpec {
    pub(super) fn from_spec(spec: &ToolSpec) -> Self {
        Self {
            kind: "function".to_string(),
            function: ChatToolFunctionSpec {
                name: spec.identity.wire_name.clone(),
                description: spec.description.clone(),
                parameters: spec.parameters.clone(),
            },
        }
    }
}

#[derive(Serialize)]
struct ChatToolFunctionSpec {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionResponse {
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: Option<ChatCompletionUsage>,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionChoice {
    pub message: ChatCompletionMessage,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ChatToolCall {
    pub id: String,
    #[serde(rename = "type")]
    kind: String,
    pub function: ChatToolFunctionCall,
}

impl ChatToolCall {
    fn from_tool_call(call: &ToolCall) -> Self {
        Self {
            id: call.id.clone(),
            kind: "function".to_string(),
            function: ChatToolFunctionCall {
                name: call.identity.wire_name.clone(),
                arguments: call.arguments.to_string(),
            },
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ChatToolFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionStreamChunk {
    pub model: String,
    pub choices: Vec<ChatCompletionStreamChoice>,
    pub usage: Option<ChatCompletionUsage>,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionStreamChoice {
    pub delta: ChatCompletionStreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionStreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ChatCompletionStreamToolCallDelta>>,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionStreamToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub function: Option<ChatCompletionStreamFunctionDelta>,
}

#[derive(Deserialize)]
pub(super) struct ChatCompletionStreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct ChatCompletionUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Clone, Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u64,
}

#[derive(Clone, Deserialize, Default)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}

impl From<ChatCompletionUsage> for ModelUsage {
    fn from(value: ChatCompletionUsage) -> Self {
        Self {
            input_tokens: value.prompt_tokens,
            cached_input_tokens: value
                .prompt_tokens_details
                .map(|details| details.cached_tokens)
                .unwrap_or(0),
            output_tokens: value.completion_tokens,
            reasoning_output_tokens: value
                .completion_tokens_details
                .map(|details| details.reasoning_tokens)
                .unwrap_or(0),
            total_tokens: value.total_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tool_message_content;

    #[test]
    fn tool_messages_forward_content_verbatim() {
        let filtered = "[rtk:generic]\nCommand summary\n- listed workspace files";
        let rendered = tool_message_content("exec_command", filtered);

        assert_eq!(rendered, filtered);
    }
}
