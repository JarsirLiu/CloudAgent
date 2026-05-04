use crate::config::ProviderRuntimeConfig;
use crate::error::{ProviderRequestError, ProviderStreamError};
use crate::event::{
    ProviderCompletion, ProviderMetadata, ProviderStreamEvent, ProviderToolCallDelta,
};
use agent_core::{
    ChatModel, ModelRequest, ModelResponse, ModelRetryDecision, ModelUsage, ResponseItem,
    StructuredToolResult, ToolCall, ToolIdentity, ToolSpec,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use config::LlmConfig;
use infra_http::{build_http_client, spawn_sse_frame_stream};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct OpenAiCompatibleModel {
    client: Client,
    config: LlmConfig,
    runtime: ProviderRuntimeConfig,
}

#[derive(Default)]
struct StreamingToolCallAcc {
    id: String,
    name: String,
    arguments: String,
}

struct ProviderEventStream {
    rx: mpsc::Receiver<Result<ProviderStreamEvent, ProviderStreamError>>,
}

impl ProviderEventStream {
    async fn recv(&mut self) -> Option<Result<ProviderStreamEvent, ProviderStreamError>> {
        self.rx.recv().await
    }
}

impl OpenAiCompatibleModel {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = build_http_client()?;
        let runtime = ProviderRuntimeConfig::from(&config);
        Ok(Self {
            client,
            config,
            runtime,
        })
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    fn tool_spec_index(tools: &[ToolSpec]) -> HashMap<String, ToolSpec> {
        tools
            .iter()
            .cloned()
            .map(|spec| (spec.identity.wire_name.clone(), spec))
            .collect()
    }

    fn request_error_from_response(
        status: reqwest::StatusCode,
        body: String,
    ) -> ProviderRequestError {
        ProviderRequestError::Http {
            status: status.as_u16(),
            body,
        }
    }

    async fn start_stream(
        &self,
        request: &ModelRequest,
    ) -> Result<ProviderEventStream, ProviderStreamError> {
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Result<Vec<_>>>()
                .map_err(|err| ProviderStreamError::Protocol {
                    message: err.to_string(),
                })?,
            tools: request.tools.iter().map(ChatToolSpec::from_spec).collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            temperature: request.temperature,
            stream: Some(true),
            stream_options: Some(ChatCompletionStreamOptions {
                include_usage: true,
            }),
        };

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|err| ProviderStreamError::Transport {
                message: format!("failed to send streaming LLM request: {err}"),
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderStreamError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let mut sse_frames = spawn_sse_frame_stream(response, self.runtime.stream_idle_timeout);
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            while let Some(frame) = sse_frames.recv().await {
                match frame {
                    Ok(data) => match parse_stream_frame(&data) {
                        Ok(events) => {
                            for event in events {
                                let is_completed =
                                    matches!(event, ProviderStreamEvent::Completed(_));
                                if tx.send(Ok(event)).await.is_err() {
                                    return;
                                }
                                if is_completed {
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(Err(err)).await;
                            return;
                        }
                    },
                    Err(err) => {
                        let _ = tx.send(Err(ProviderStreamError::from(err))).await;
                        return;
                    }
                }
            }
        });

        Ok(ProviderEventStream { rx })
    }
}

#[async_trait]
impl ChatModel for OpenAiCompatibleModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let tool_spec_index = Self::tool_spec_index(&request.tools);
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Result<Vec<_>>>()?,
            tools: request.tools.iter().map(ChatToolSpec::from_spec).collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
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
            .map_err(|err| {
                anyhow::Error::new(ProviderRequestError::Transport {
                    message: format!("failed to send LLM request: {err}"),
                })
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|err| {
            anyhow::Error::new(ProviderRequestError::Transport {
                message: format!("failed to read LLM body: {err}"),
            })
        })?;
        if !status.is_success() {
            return Err(anyhow::Error::new(Self::request_error_from_response(
                status, body,
            )));
        }

        let parsed: ChatCompletionResponse = serde_json::from_str(&body).map_err(|err| {
            anyhow::Error::new(ProviderRequestError::Protocol {
                message: format!("failed to parse LLM response: {err}"),
            })
        })?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("LLM response contained no choices"))?;

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
                    name: tool_spec_index
                        .get(&call.function.name)
                        .map(|spec| spec.name.clone())
                        .unwrap_or_else(|| call.function.name.clone()),
                    identity: tool_spec_index
                        .get(&call.function.name)
                        .map(|spec| spec.identity.clone())
                        .unwrap_or_else(|| ToolIdentity::built_in(call.function.name.clone())),
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

    fn request_max_retries(&self) -> u64 {
        self.runtime.request_max_retries
    }

    fn stream_max_retries(&self) -> u64 {
        self.runtime.stream_max_retries
    }

    fn classify_request_error(&self, err: &anyhow::Error) -> ModelRetryDecision {
        if let Some(err) = err.downcast_ref::<ProviderRequestError>() {
            match err {
                ProviderRequestError::Http { status, .. } if *status == 429 || *status >= 500 => {
                    ModelRetryDecision::retry(None)
                }
                ProviderRequestError::Transport { .. } => ModelRetryDecision::retry(None),
                ProviderRequestError::Provider { retry_after_ms, .. } => {
                    ModelRetryDecision::retry(retry_after_ms.map(Duration::from_millis))
                }
                ProviderRequestError::Http { .. } | ProviderRequestError::Protocol { .. } => {
                    ModelRetryDecision::no_retry()
                }
            }
        } else {
            ModelRetryDecision::no_retry()
        }
    }

    fn classify_stream_error(&self, err: &anyhow::Error) -> ModelRetryDecision {
        if let Some(err) = err.downcast_ref::<ProviderStreamError>() {
            match err {
                ProviderStreamError::IdleTimeout
                | ProviderStreamError::ClosedBeforeCompletion
                | ProviderStreamError::Transport { .. } => ModelRetryDecision::retry(None),
                ProviderStreamError::Http { status, .. } if *status == 429 || *status >= 500 => {
                    ModelRetryDecision::retry(None)
                }
                ProviderStreamError::Provider { retry_after_ms, .. } => {
                    ModelRetryDecision::retry(retry_after_ms.map(Duration::from_millis))
                }
                ProviderStreamError::Http { .. } | ProviderStreamError::Protocol { .. } => {
                    ModelRetryDecision::no_retry()
                }
            }
        } else {
            ModelRetryDecision::no_retry()
        }
    }

    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_text_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ModelResponse> {
        let tool_spec_index = Self::tool_spec_index(&request.tools);
        let mut stream = self
            .start_stream(&request)
            .await
            .map_err(anyhow::Error::new)?;

        let mut content = String::new();
        let mut model_name: Option<String> = None;
        let mut usage: Option<ModelUsage> = None;
        let mut completion: Option<ProviderCompletion> = None;
        let mut tool_calls_acc: HashMap<usize, StreamingToolCallAcc> = HashMap::new();

        while let Some(event) = stream.recv().await {
            let event = event.map_err(anyhow::Error::new)?;
            match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    if !delta.is_empty() {
                        on_text_delta(delta.clone());
                        content.push_str(&delta);
                    }
                }
                ProviderStreamEvent::ToolCallDelta(delta) => {
                    let acc = tool_calls_acc.entry(delta.index).or_default();
                    if let Some(id) = delta.id {
                        acc.id = id;
                    }
                    if let Some(name) = delta.name {
                        acc.name = name;
                    }
                    if let Some(arguments_delta) = delta.arguments_delta {
                        acc.arguments.push_str(&arguments_delta);
                    }
                }
                ProviderStreamEvent::Usage(chunk_usage) => {
                    usage = Some(chunk_usage);
                }
                ProviderStreamEvent::Metadata(metadata) => {
                    if model_name.is_none() {
                        model_name = metadata.model_name;
                    }
                }
                ProviderStreamEvent::Completed(done) => {
                    completion = Some(done);
                    break;
                }
            }
        }

        if completion.is_none() {
            return Err(anyhow::Error::new(
                ProviderStreamError::ClosedBeforeCompletion,
            ));
        }

        let mut tool_calls = Vec::new();
        let mut ordered: Vec<(usize, StreamingToolCallAcc)> = tool_calls_acc.into_iter().collect();
        ordered.sort_by_key(|(index, _)| *index);
        for (_, acc) in ordered {
            if acc.id.is_empty() || acc.name.is_empty() {
                continue;
            }
            let arguments = serde_json::from_str::<Value>(&acc.arguments)
                .unwrap_or_else(|_| Value::String(acc.arguments.clone()));
            tool_calls.push(ToolCall {
                id: acc.id,
                name: tool_spec_index
                    .get(&acc.name)
                    .map(|spec| spec.name.clone())
                    .unwrap_or_else(|| acc.name.clone()),
                identity: tool_spec_index
                    .get(&acc.name)
                    .map(|spec| spec.identity.clone())
                    .unwrap_or_else(|| ToolIdentity::built_in(acc.name.clone())),
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

fn parse_stream_frame(data: &str) -> Result<Vec<ProviderStreamEvent>, ProviderStreamError> {
    if data == "[DONE]" {
        return Ok(vec![ProviderStreamEvent::Completed(
            ProviderCompletion::default(),
        )]);
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
                }));
            }
        }
        if let Some(finish_reason) = choice.finish_reason {
            completion.finish_reason = Some(finish_reason);
            saw_completion = true;
        }
    }
    if saw_completion {
        events.push(ProviderStreamEvent::Completed(completion));
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn done_frame_maps_to_completed_event() {
        let events = parse_stream_frame("[DONE]").expect("parse done frame");
        assert_eq!(
            events,
            vec![ProviderStreamEvent::Completed(ProviderCompletion::default())]
        );
    }

    #[tokio::test]
    async fn streaming_returns_after_done_even_if_connection_stays_open() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .expect("set write timeout");
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = concat!(
                "data: {\"id\":\"resp_1\",\"object\":\"chat.completion.chunk\",\"created\":0,",
                "\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "data: [DONE]\n\n"
            );
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Transfer-Encoding: chunked\r\n",
                "Connection: keep-alive\r\n",
                "\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write headers");
            stream
                .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                .expect("write chunk size");
            stream.write_all(body.as_bytes()).expect("write chunk body");
            stream.write_all(b"\r\n").expect("write chunk suffix");
            stream.flush().expect("flush response");
            thread::sleep(Duration::from_secs(3));
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            temperature: 0.0,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let request = ModelRequest {
            messages: vec![ResponseItem::User {
                content: "hello".to_string(),
            }],
            tools: Vec::new(),
            temperature: 0.0,
        };

        let response = tokio::time::timeout(Duration::from_secs(1), async {
            let mut streamed = String::new();
            model
                .complete_streaming(request, &mut |delta| streamed.push_str(&delta))
                .await
                .map(|response| (response, streamed))
        })
        .await
        .expect("stream should finish before socket closes")
        .expect("streaming request should succeed");

        assert_eq!(response.0.content.as_deref(), Some("hi"));
        assert_eq!(response.1, "hi");

        server.join().expect("server thread");
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
struct ChatCompletionResponse {
    model: String,
    choices: Vec<ChatCompletionChoice>,
    usage: Option<ChatCompletionUsage>,
}

#[derive(Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Deserialize)]
struct ChatCompletionMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolFunctionCall,
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
struct ChatToolFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatCompletionStreamChunk {
    model: String,
    choices: Vec<ChatCompletionStreamChoice>,
    usage: Option<ChatCompletionUsage>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamChoice {
    delta: ChatCompletionStreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<ChatCompletionStreamToolCallDelta>>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<ChatCompletionStreamFunctionDelta>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Clone, Deserialize)]
struct ChatCompletionUsage {
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
