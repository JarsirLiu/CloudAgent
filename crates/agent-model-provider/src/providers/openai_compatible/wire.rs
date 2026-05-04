use agent_core::{ModelUsage, ResponseItem, StructuredToolResult, ToolCall, ToolSpec};
use anyhow::Result;
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
    pub(super) fn from_message(message: &ResponseItem) -> Result<Self> {
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
                            .map(ChatToolCall::from_tool_call)
                            .collect(),
                    )
                },
                tool_call_id: None,
                name: None,
            }),
            ResponseItem::Tool {
                tool_call_id,
                name,
                content,
                structured,
            } => Ok(Self {
                role: "tool".to_string(),
                content: Some(tool_message_content(name, content, structured.as_ref())),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
                name: Some(name.clone()),
            }),
        }
    }
}

fn tool_message_content(
    name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
) -> String {
    if let Some(StructuredToolResult::CommandExecution {
        command,
        current_directory,
        success,
        exit_code,
        stdout,
        stderr,
        aggregated_output,
        ..
    }) = structured
    {
        let mut rendered = String::new();
        rendered.push_str(&format!("tool `{name}` executed `{command}`"));
        rendered.push_str(&format!(" in `{current_directory}`"));
        if let Some(ok) = success {
            rendered.push_str(if *ok {
                " successfully."
            } else {
                " with failure."
            });
        } else {
            rendered.push('.');
        }
        if let Some(code) = exit_code {
            rendered.push_str(&format!(" exit_code={code}."));
        }
        if let Some(output) = aggregated_output
            && !output.trim().is_empty()
        {
            rendered.push_str("\noutput:\n");
            rendered.push_str(output);
        } else {
            if let Some(out) = stdout
                && !out.trim().is_empty()
            {
                rendered.push_str("\nstdout:\n");
                rendered.push_str(out);
            }
            if let Some(err) = stderr
                && !err.trim().is_empty()
            {
                rendered.push_str("\nstderr:\n");
                rendered.push_str(err);
            }
        }
        return rendered;
    }
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
