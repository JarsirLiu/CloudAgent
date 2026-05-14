use crate::request::ProviderMessage;
use agent_core::conversation::{AttachmentRef, ImageDetail, InputItem};
use agent_core::model::ModelUsage;
use agent_core::tool::{ToolCall, ToolSpec};
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
    content: Option<ChatApiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
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
                content: Some(ChatApiContent::Text(content.clone())),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ProviderMessage::User { content } => Self {
                role: "user".to_string(),
                content: Some(ChatApiContent::Parts(user_content_parts(content))),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ProviderMessage::Assistant {
                content,
                reasoning,
                tool_calls,
            } => Self {
                role: "assistant".to_string(),
                content: content.clone().map(ChatApiContent::Text),
                reasoning_content: reasoning.clone(),
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
                content: Some(ChatApiContent::Text(tool_message_content(name, content))),
                reasoning_content: None,
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
#[serde(untagged)]
enum ChatApiContent {
    Text(String),
    Parts(Vec<ChatApiContentPart>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatApiContentPart {
    Text { text: String },
    ImageUrl { image_url: ChatApiImageUrl },
}

#[derive(Serialize)]
struct ChatApiImageUrl {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'static str>,
}

fn user_content_parts(items: &[InputItem]) -> Vec<ChatApiContentPart> {
    let mut parts = Vec::new();
    for item in items {
        match item {
            InputItem::Text { text } => parts.push(ChatApiContentPart::Text { text: text.clone() }),
            InputItem::Image {
                source,
                detail,
                alt,
            } => {
                if let Some(url) = attachment_url(source) {
                    parts.push(ChatApiContentPart::ImageUrl {
                        image_url: ChatApiImageUrl {
                            url,
                            detail: detail.as_ref().map(image_detail_label),
                        },
                    });
                } else if let Some(alt) = alt {
                    parts.push(ChatApiContentPart::Text {
                        text: format!("[image unavailable: {alt}]"),
                    });
                }
            }
            InputItem::File {
                name, mime_type, ..
            } => {
                let label = name.clone().unwrap_or_else(|| "attachment".to_string());
                let suffix = mime_type
                    .as_ref()
                    .map(|mime| format!(" ({mime})"))
                    .unwrap_or_default();
                parts.push(ChatApiContentPart::Text {
                    text: format!("[file: {label}{suffix}]"),
                });
            }
            InputItem::Mention { name, path } => parts.push(ChatApiContentPart::Text {
                text: format!("@{name} ({path})"),
            }),
            InputItem::Skill { name, path } => parts.push(ChatApiContentPart::Text {
                text: format!("${name} ({path})"),
            }),
        }
    }
    if parts.is_empty() {
        parts.push(ChatApiContentPart::Text {
            text: String::new(),
        });
    }
    parts
}

fn attachment_url(source: &AttachmentRef) -> Option<String> {
    match source {
        AttachmentRef::InlineDataUrl { data_url } => Some(data_url.clone()),
        AttachmentRef::RemoteUrl { url } => Some(url.clone()),
        AttachmentRef::HubAsset { download_url, .. } => download_url.clone(),
        AttachmentRef::LocalPath { .. } => None,
    }
}

fn image_detail_label(detail: &ImageDetail) -> &'static str {
    match detail {
        ImageDetail::Auto => "auto",
        ImageDetail::Low => "low",
        ImageDetail::High => "high",
        ImageDetail::Original => "high",
    }
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
    #[serde(default)]
    pub reasoning_content: Option<String>,
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
    #[serde(default)]
    pub reasoning_content: Option<String>,
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
    use super::{ChatApiMessage, ProviderMessage, tool_message_content};
    use agent_core::{AttachmentRef, ImageDetail, InputItem};
    use serde_json::json;

    #[test]
    fn tool_messages_forward_content_verbatim() {
        let filtered = "[rtk:generic]\nCommand summary\n- listed workspace files";
        let rendered = tool_message_content("exec_command", filtered);

        assert_eq!(rendered, filtered);
    }

    #[test]
    fn user_messages_encode_text_and_image_parts_for_openai_wire() {
        let message = ChatApiMessage::from_message(&ProviderMessage::User {
            content: vec![
                InputItem::Text {
                    text: "describe this".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::RemoteUrl {
                        url: "https://example.com/diagram.png".to_string(),
                    },
                    detail: Some(ImageDetail::High),
                    alt: Some("diagram".to_string()),
                },
            ],
        });

        let value = serde_json::to_value(message).expect("serialize user message");
        assert_eq!(
            value,
            json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": "describe this" },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/diagram.png",
                            "detail": "high"
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn local_path_images_fall_back_to_alt_text_until_materialized() {
        let message = ChatApiMessage::from_message(&ProviderMessage::User {
            content: vec![InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: "C:\\tmp\\plan.png".to_string(),
                },
                detail: Some(ImageDetail::Low),
                alt: Some("plan".to_string()),
            }],
        });

        let value = serde_json::to_value(message).expect("serialize user message");
        assert_eq!(
            value,
            json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": "[image unavailable: plan]" }
                ]
            })
        );
    }

    #[test]
    fn assistant_messages_include_reasoning_content_when_present() {
        let message = ChatApiMessage::from_message(&ProviderMessage::Assistant {
            content: Some("answer".to_string()),
            reasoning: Some("hidden chain".to_string()),
            tool_calls: Vec::new(),
        });

        let value = serde_json::to_value(message).expect("serialize assistant message");
        assert_eq!(
            value,
            json!({
                "role": "assistant",
                "content": "answer",
                "reasoning_content": "hidden chain"
            })
        );
    }
}
