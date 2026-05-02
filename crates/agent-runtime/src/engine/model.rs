use agent_core::{
    ChatModel, ModelRequest, ModelResponse, ModelUsage, ResponseItem, ToolCall, ToolSpec,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use config::LlmConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) struct OpenAiCompatibleModel {
    client: reqwest::Client,
    config: config::LlmConfig,
}

#[derive(Default)]
pub(crate) struct StreamingToolCallAcc {
    id: String,
    name: String,
    arguments: String,
}

impl OpenAiCompatibleModel {
    pub(crate) fn new(config: LlmConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent("cloudagent/0.1.0")
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { client, config })
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait]
impl ChatModel for OpenAiCompatibleModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Result<Vec<_>>>()?,
            tools: request.tools.iter().map(ChatToolSpec::from_spec).collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: request.temperature,
            stream: None,
            stream_options: None,
        };

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await
            .context("failed to send LLM request")?;

        let status = response.status();
        let body = response.text().await.context("failed to read LLM body")?;
        if !status.is_success() {
            bail!("LLM request failed with status {status}: {body}");
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body).context("failed to parse LLM response")?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("LLM response contained no choices"))?;

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| {
                let arguments = serde_json::from_str::<Value>(&call.function.arguments)
                    .unwrap_or_else(|_| Value::String(call.function.arguments.clone()));
                ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments,
                }
            })
            .collect();

        Ok(ModelResponse {
            content: choice.message.content,
            tool_calls,
            model_name: Some(parsed.model),
            usage: parsed.usage.map(ModelUsage::from),
        })
    }

    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_text_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ModelResponse> {
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Result<Vec<_>>>()?,
            tools: request.tools.iter().map(ChatToolSpec::from_spec).collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: request.temperature,
            stream: Some(true),
            stream_options: Some(ChatCompletionStreamOptions {
                include_usage: true,
            }),
        };

        let mut response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await
            .context("failed to send streaming LLM request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .context("failed to read streaming LLM error body")?;
            bail!("LLM streaming request failed with status {status}: {body}");
        }

        let mut content = String::new();
        let mut model_name: Option<String> = None;
        let mut stream_buffer = String::new();
        let mut usage: Option<ModelUsage> = None;
        let mut tool_calls_acc: std::collections::HashMap<usize, StreamingToolCallAcc> =
            std::collections::HashMap::new();

        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed reading streaming response chunk")?
        {
            stream_buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = stream_buffer.find('\n') {
                let line = stream_buffer[..pos].trim().to_string();
                stream_buffer = stream_buffer[pos + 1..].to_string();
                if line.is_empty() || !line.starts_with("data:") {
                    continue;
                }
                let data = line.trim_start_matches("data:").trim();
                if data == "[DONE]" {
                    break;
                }
                let parsed: ChatCompletionStreamChunk = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if model_name.is_none() {
                    model_name = Some(parsed.model.clone());
                }
                if let Some(chunk_usage) = parsed.usage {
                    usage = Some(ModelUsage::from(chunk_usage));
                }
                for choice in parsed.choices {
                    if let Some(delta) = choice.delta.content
                        && !delta.is_empty()
                    {
                        on_text_delta(delta.clone());
                        content.push_str(&delta);
                    }
                    if let Some(delta_tool_calls) = choice.delta.tool_calls {
                        for delta_call in delta_tool_calls {
                            let index = delta_call.index;
                            let acc = tool_calls_acc.entry(index).or_default();
                            if let Some(id) = delta_call.id {
                                acc.id = id;
                            }
                            if let Some(function) = delta_call.function {
                                if let Some(name) = function.name {
                                    acc.name = name;
                                }
                                if let Some(arguments) = function.arguments {
                                    acc.arguments.push_str(&arguments);
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut tool_calls = Vec::new();
        let mut ordered: Vec<(usize, StreamingToolCallAcc)> = tool_calls_acc.into_iter().collect();
        ordered.sort_by_key(|(idx, _)| *idx);
        for (_, acc) in ordered {
            if acc.id.is_empty() || acc.name.is_empty() {
                continue;
            }
            let arguments = serde_json::from_str::<Value>(&acc.arguments)
                .unwrap_or_else(|_| Value::String(acc.arguments.clone()));
            tool_calls.push(ToolCall {
                id: acc.id,
                name: acc.name,
                arguments,
            });
        }

        Ok(ModelResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            tool_calls,
            model_name,
            usage,
        })
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatApiMessage>,
    tools: Vec<ChatToolSpec>,
    tool_choice: String,
    parallel_tool_calls: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<ChatCompletionStreamOptions>,
}
#[derive(Serialize)]
struct ChatCompletionStreamOptions {
    include_usage: bool,
}
#[derive(Serialize)]
struct ChatApiMessage {
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
    fn from_message(message: &ResponseItem) -> Result<Self> {
        match message {
            ResponseItem::System { content } => Ok(Self {
                role: "system".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }),
            ResponseItem::User { content } => Ok(Self {
                role: "user".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }),
            ResponseItem::Assistant {
                content,
                tool_calls,
            } => Ok(Self {
                role: "assistant".to_string(),
                content: content.clone(),
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls
                            .iter()
                            .map(ChatToolCall::from_internal)
                            .collect::<Result<Vec<_>>>()?,
                    )
                },
                tool_call_id: None,
                name: None,
            }),
            ResponseItem::Tool {
                tool_call_id,
                name,
                content,
                ..
            } => Ok(Self {
                role: "tool".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
                name: Some(name.clone()),
            }),
        }
    }
}
#[derive(Serialize)]
struct ChatToolSpec {
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolFunctionSpec,
}
impl ChatToolSpec {
    fn from_spec(spec: &ToolSpec) -> Self {
        Self {
            kind: "function".to_string(),
            function: ChatToolFunctionSpec {
                name: spec.name.clone(),
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
#[derive(Serialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolFunctionCall,
}
impl ChatToolCall {
    fn from_internal(call: &ToolCall) -> Result<Self> {
        Ok(Self {
            id: call.id.clone(),
            kind: "function".to_string(),
            function: ChatToolFunctionCall {
                name: call.name.clone(),
                arguments: serde_json::to_string(&call.arguments)?,
            },
        })
    }
}
#[derive(Serialize, Deserialize)]
struct ChatToolFunctionCall {
    name: String,
    arguments: String,
}
#[derive(Deserialize)]
struct ChatCompletionResponse {
    model: String,
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}
#[derive(Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatResponseToolCall>>,
}
#[derive(Deserialize)]
struct ChatResponseToolCall {
    id: String,
    function: ChatToolFunctionCall,
}
#[derive(Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<ChatPromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<ChatCompletionTokensDetails>,
}
#[derive(Deserialize)]
struct ChatPromptTokensDetails {
    #[serde(default)]
    cached_tokens: u64,
}
#[derive(Deserialize)]
struct ChatCompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}
impl From<ChatUsage> for ModelUsage {
    fn from(value: ChatUsage) -> Self {
        Self {
            input_tokens: value.prompt_tokens,
            cached_input_tokens: value
                .prompt_tokens_details
                .map(|details| details.cached_tokens)
                .unwrap_or_default(),
            output_tokens: value.completion_tokens,
            reasoning_output_tokens: value
                .completion_tokens_details
                .map(|details| details.reasoning_tokens)
                .unwrap_or_default(),
            total_tokens: value.total_tokens,
        }
    }
}
#[derive(Deserialize)]
struct ChatCompletionStreamChunk {
    model: String,
    choices: Vec<ChatCompletionStreamChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}
#[derive(Deserialize)]
struct ChatCompletionStreamChoice {
    delta: ChatCompletionStreamDelta,
}
#[derive(Deserialize)]
struct ChatCompletionStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatCompletionStreamToolCallDelta>>,
}
#[derive(Deserialize)]
struct ChatCompletionStreamToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChatCompletionStreamFunctionDelta>,
}
#[derive(Deserialize)]
struct ChatCompletionStreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
