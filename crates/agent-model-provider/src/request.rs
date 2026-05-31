use agent_core::conversation::{AttachmentRef, InputItem, ResponseItem};
use agent_core::model::ModelRequest;
use agent_core::output_truncation::truncate_text_to_token_budget;
use agent_core::tool::{ToolCall, ToolSpec};
use anyhow::{Result, anyhow};
use base64::Engine;
use std::path::Path;

const IMAGE_INPUT_UNSUPPORTED_PLACEHOLDER: &str =
    "image content omitted because the configured model does not support image input";

#[derive(Clone, Debug)]
pub(crate) struct ProviderRequest {
    pub messages: Vec<ProviderMessage>,
    pub tools: Vec<ToolSpec>,
    pub temperature: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum ProviderMessage {
    System {
        content: String,
    },
    User {
        content: Vec<InputItem>,
    },
    Assistant {
        content: Option<String>,
        reasoning: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        name: String,
        content: String,
    },
}

impl ProviderRequest {
    pub(crate) async fn from_model_request(
        request: &ModelRequest,
        supports_image_input: bool,
    ) -> Result<Self> {
        let mut messages = Vec::with_capacity(request.messages.len());
        for item in &request.messages {
            messages.push(
                ProviderMessage::from_response_item(
                    item,
                    supports_image_input,
                    request.tool_output_token_limit,
                )
                .await?,
            );
        }
        downgrade_incomplete_reasoning_chain(&mut messages);
        Ok(Self {
            messages,
            tools: request.tools.clone(),
            temperature: request.temperature,
        })
    }
}

impl ProviderMessage {
    async fn normalize_user_content(
        content: &[InputItem],
        supports_image_input: bool,
    ) -> Result<Vec<InputItem>> {
        let mut normalized = Vec::with_capacity(content.len());
        for item in content {
            normalized.push(match item {
                InputItem::Image { .. } if !supports_image_input => InputItem::Text {
                    text: IMAGE_INPUT_UNSUPPORTED_PLACEHOLDER.to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::LocalPath { path },
                    detail,
                    alt,
                } => InputItem::Image {
                    source: AttachmentRef::InlineDataUrl {
                        data_url: local_image_path_to_data_url(path).await?,
                    },
                    detail: detail.clone(),
                    alt: alt.clone(),
                },
                other => other.clone(),
            });
        }
        Ok(normalized)
    }

    async fn from_response_item(
        item: &ResponseItem,
        supports_image_input: bool,
        tool_output_token_limit: usize,
    ) -> Result<Self> {
        Ok(match item {
            ResponseItem::System { content } => Self::System {
                content: content.clone(),
            },
            ResponseItem::User { content } => Self::User {
                content: Self::normalize_user_content(content, supports_image_input).await?,
            },
            ResponseItem::Assistant {
                content,
                reasoning,
                tool_calls,
            } => Self::Assistant {
                content: content.clone(),
                reasoning: reasoning.clone(),
                tool_calls: tool_calls.clone(),
            },
            ResponseItem::Tool {
                tool_call_id,
                name,
                content,
                ..
            } => Self::Tool {
                tool_call_id: tool_call_id.clone(),
                name: name.clone(),
                content: truncate_text_to_token_budget(
                    content,
                    tool_output_token_limit,
                    Some("\n[tool output truncated before model request]\n"),
                )
                .text,
            },
        })
    }
}

async fn local_image_path_to_data_url(path: &str) -> Result<String> {
    let mime = infer_image_mime_type(path)?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|err| anyhow!("failed to read image `{path}`: {err}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn infer_image_mime_type(path: &str) -> Result<&'static str> {
    let ext = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or_else(|| anyhow!("image path `{path}` is missing a file extension"))?;

    match ext.as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "webp" => Ok("image/webp"),
        "gif" => Ok("image/gif"),
        "bmp" => Ok("image/bmp"),
        _ => Err(anyhow!("unsupported image extension `.{ext}` for `{path}`")),
    }
}

fn downgrade_incomplete_reasoning_chain(messages: &mut [ProviderMessage]) {
    let assistant_reasoning_states = messages.iter().filter_map(|message| match message {
        ProviderMessage::Assistant { reasoning, .. } => Some(reasoning.as_deref()),
        _ => None,
    });

    let mut saw_reasoning = false;
    let mut saw_missing_reasoning = false;
    for reasoning in assistant_reasoning_states {
        if reasoning.is_some_and(|value| !value.trim().is_empty()) {
            saw_reasoning = true;
        } else {
            saw_missing_reasoning = true;
        }
        if saw_reasoning && saw_missing_reasoning {
            break;
        }
    }

    if !(saw_reasoning && saw_missing_reasoning) {
        return;
    }

    for message in messages {
        if let ProviderMessage::Assistant { reasoning, .. } = message {
            *reasoning = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{CommandExecutionStatus, StructuredToolResult, ToolIdentity};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn provider_request_drops_structured_tool_output_details() {
        let request = ModelRequest {
            messages: vec![ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: "[rtk:generic]\nsummary".to_string(),
                structured: Some(StructuredToolResult::CommandExecution {
                    command: "Get-ChildItem -Recurse".to_string(),
                    current_directory: "D:\\repo".to_string(),
                    session_id: None,
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    success: Some(true),
                    output: Some("very large raw output".to_string()),
                    duration_ms: Some(1),
                    original_token_count: Some(1024),
                    max_output_tokens: Some(10_000),
                }),
            }],
            tools: vec![ToolSpec {
                name: "exec_command".to_string(),
                identity: ToolIdentity::built_in("exec_command"),
                description: "run a command".to_string(),
                parameters: json!({ "type": "object" }),
                mutating: false,
                execution_policy: agent_core::ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: agent_core::turn::TurnItemKind::ToolCall,
                delta_kind: agent_core::turn::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            }],
            temperature: 0.0,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let provider_request = ProviderRequest::from_model_request(&request, true)
            .await
            .expect("provider request");
        match &provider_request.messages[0] {
            ProviderMessage::Tool { content, .. } => {
                assert_eq!(content, "[rtk:generic]\nsummary");
            }
            _ => panic!("expected tool message"),
        }
    }

    #[tokio::test]
    async fn provider_request_truncates_large_tool_content_before_model_send() {
        let large = "tool-output ".repeat(20_000);
        let request = ModelRequest {
            messages: vec![ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: large.clone(),
                structured: None,
            }],
            tools: Vec::new(),
            temperature: 0.0,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let provider_request = ProviderRequest::from_model_request(&request, true)
            .await
            .expect("provider request");

        match &provider_request.messages[0] {
            ProviderMessage::Tool { content, .. } => {
                assert!(content.len() < large.len());
                assert!(content.contains("tool output truncated before model request"));
            }
            _ => panic!("expected tool message"),
        }
    }

    #[tokio::test]
    async fn provider_request_uses_request_tool_output_limit() {
        let large = "tool-output ".repeat(20_000);
        let request = ModelRequest {
            messages: vec![ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: large.clone(),
                structured: None,
            }],
            tools: Vec::new(),
            temperature: 0.0,
            tool_output_token_limit: 100,
        };

        let provider_request = ProviderRequest::from_model_request(&request, true)
            .await
            .expect("provider request");

        match &provider_request.messages[0] {
            ProviderMessage::Tool { content, .. } => {
                assert!(content.len() < large.len());
                assert!(content.contains("tool output truncated before model request"));
            }
            _ => panic!("expected tool message"),
        }
    }

    #[tokio::test]
    async fn provider_request_materializes_local_image_paths_only_for_model_send() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("provider-image-{nonce}.png"));
        tokio::fs::write(&path, [1u8, 2, 3, 4])
            .await
            .expect("write temp image");

        let request = ModelRequest {
            messages: vec![ResponseItem::User {
                content: vec![InputItem::Image {
                    source: AttachmentRef::LocalPath {
                        path: path.display().to_string(),
                    },
                    detail: None,
                    alt: Some("diagram".to_string()),
                }],
            }],
            tools: Vec::new(),
            temperature: 0.0,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let provider_request = ProviderRequest::from_model_request(&request, true)
            .await
            .expect("provider request");

        match &provider_request.messages[0] {
            ProviderMessage::User { content } => match &content[0] {
                InputItem::Image {
                    source: AttachmentRef::InlineDataUrl { data_url },
                    alt,
                    ..
                } => {
                    assert!(data_url.starts_with("data:image/png;base64,"));
                    assert_eq!(alt.as_deref(), Some("diagram"));
                }
                other => panic!("expected inline image payload, got {other:?}"),
            },
            other => panic!("expected user message, got {other:?}"),
        }

        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn provider_request_replaces_images_with_placeholder_when_model_lacks_image_input() {
        let request = ModelRequest {
            messages: vec![ResponseItem::User {
                content: vec![
                    InputItem::Text {
                        text: "please inspect".to_string(),
                    },
                    InputItem::Image {
                        source: AttachmentRef::LocalPath {
                            path: "D:\\missing.png".to_string(),
                        },
                        detail: None,
                        alt: Some("diagram".to_string()),
                    },
                ],
            }],
            tools: Vec::new(),
            temperature: 0.0,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let provider_request = ProviderRequest::from_model_request(&request, false)
            .await
            .expect("provider request");

        match &provider_request.messages[0] {
            ProviderMessage::User { content } => {
                assert_eq!(content.len(), 2);
                assert_eq!(
                    content[1],
                    InputItem::Text {
                        text: IMAGE_INPUT_UNSUPPORTED_PLACEHOLDER.to_string(),
                    }
                );
            }
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn provider_request_downgrades_to_non_thinking_when_reasoning_chain_is_incomplete() {
        let request = ModelRequest {
            messages: vec![
                ResponseItem::Assistant {
                    content: Some("first".to_string()),
                    reasoning: Some("first reasoning".to_string()),
                    tool_calls: Vec::new(),
                },
                ResponseItem::Assistant {
                    content: Some("second".to_string()),
                    reasoning: None,
                    tool_calls: Vec::new(),
                },
            ],
            tools: Vec::new(),
            temperature: 0.0,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let provider_request = ProviderRequest::from_model_request(&request, true)
            .await
            .expect("provider request");

        assert!(matches!(
            &provider_request.messages[..],
            [
                ProviderMessage::Assistant {
                    reasoning: None,
                    ..
                },
                ProviderMessage::Assistant {
                    reasoning: None,
                    ..
                }
            ]
        ));
    }

    #[tokio::test]
    async fn provider_request_keeps_reasoning_when_chain_is_complete() {
        let request = ModelRequest {
            messages: vec![ResponseItem::Assistant {
                content: Some("first".to_string()),
                reasoning: Some("first reasoning".to_string()),
                tool_calls: Vec::new(),
            }],
            tools: Vec::new(),
            temperature: 0.0,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let provider_request = ProviderRequest::from_model_request(&request, true)
            .await
            .expect("provider request");

        assert!(matches!(
            &provider_request.messages[..],
            [ProviderMessage::Assistant {
                reasoning: Some(reasoning),
                ..
            }] if reasoning == "first reasoning"
        ));
    }
}
