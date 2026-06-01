use super::stream::{ProviderEventStream, parse_stream_frame};
use super::wire::{
    ChatApiMessage, ChatCompletionRequest, ChatCompletionResponse, ChatCompletionStreamOptions,
    ChatToolSpec,
};
use crate::config::ProviderRuntimeConfig;
use crate::error::{ProviderRequestError, ProviderStreamError};
use crate::event::{ProviderCompletion, ProviderReasoningDelta, ProviderStreamEvent};
use crate::request::ProviderRequest;
use agent_core::model::{
    ChatModel, ModelRequest, ModelResponse, ModelRetryDecision, ModelStreamObserver, ModelUsage,
    ReasoningDelta,
};
use agent_core::tool::{ToolCall, ToolIdentity, ToolSpec};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use config::LlmConfig;
use infra_http::{build_http_client, spawn_sse_frame_stream};
use reqwest::Client;
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
        let provider_request =
            ProviderRequest::from_model_request(request, self.runtime.supports_image_input())
                .await
                .map_err(|err| ProviderStreamError::Transport {
                    message: format!("failed to prepare LLM request: {err}"),
                })?;
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: provider_request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Vec<_>>(),
            tools: provider_request
                .tools
                .iter()
                .map(ChatToolSpec::from_spec)
                .collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            temperature: provider_request.temperature,
            reasoning_effort: provider_request.reasoning_effort,
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
            let mut pending_completion: Option<ProviderCompletion> = None;
            while let Some(frame) = sse_frames.recv().await {
                match frame {
                    Ok(data) => match parse_stream_frame(&data) {
                        Ok(frame) => {
                            for event in frame.events {
                                if tx.send(Ok(event)).await.is_err() {
                                    return;
                                }
                            }
                            if let Some(completion) = frame.completion {
                                pending_completion = Some(completion);
                            }
                            if frame.done {
                                let completion = pending_completion.take().unwrap_or_default();
                                let _ = tx
                                    .send(Ok(ProviderStreamEvent::Completed(completion)))
                                    .await;
                                return;
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

        Ok(ProviderEventStream::new(rx))
    }
}

#[async_trait]
impl ChatModel for OpenAiCompatibleModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let provider_request =
            ProviderRequest::from_model_request(&request, self.runtime.supports_image_input())
                .await?;
        let tool_spec_index = Self::tool_spec_index(&provider_request.tools);
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: provider_request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Vec<_>>(),
            tools: provider_request
                .tools
                .iter()
                .map(ChatToolSpec::from_spec)
                .collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            temperature: provider_request.temperature,
            reasoning_effort: provider_request.reasoning_effort,
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
            reasoning: choice.message.reasoning_content,
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
                ProviderStreamError::FirstFrameTimeout
                | ProviderStreamError::IdleTimeout
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
        observer: &mut dyn ModelStreamObserver,
    ) -> Result<ModelResponse> {
        let tool_spec_index = Self::tool_spec_index(&request.tools);
        let mut stream = self
            .start_stream(&request)
            .await
            .map_err(anyhow::Error::new)?;

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut model_name: Option<String> = None;
        let mut usage: Option<ModelUsage> = None;
        let mut completion: Option<ProviderCompletion> = None;
        let mut tool_calls_acc: HashMap<usize, StreamingToolCallAcc> = HashMap::new();

        while let Some(event) = stream.recv().await {
            let event = event.map_err(anyhow::Error::new)?;
            match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    if !delta.is_empty() {
                        observer.on_text_delta(delta.clone());
                        content.push_str(&delta);
                    }
                }
                ProviderStreamEvent::ReasoningDelta(delta) => match delta {
                    ProviderReasoningDelta::SummaryText {
                        summary_index,
                        delta,
                    } => {
                        if !delta.is_empty() {
                            observer.on_reasoning_delta(ReasoningDelta::SummaryText {
                                summary_index,
                                delta: delta.clone(),
                            });
                            reasoning.push_str(&delta);
                        }
                    }
                    ProviderReasoningDelta::Text {
                        content_index,
                        delta,
                    } => {
                        if !delta.is_empty() {
                            observer.on_reasoning_delta(ReasoningDelta::Text {
                                content_index,
                                delta: delta.clone(),
                            });
                            reasoning.push_str(&delta);
                        }
                    }
                },
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
            reasoning: if reasoning.is_empty() {
                None
            } else {
                Some(reasoning)
            },
            tool_calls,
            model_name,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ModelStreamObserver;
    use agent_core::ResponseItem;
    use config::default_input_modalities;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[derive(Default)]
    struct TestObserver {
        text: String,
        reasoning_text: String,
    }

    impl ModelStreamObserver for TestObserver {
        fn on_text_delta(&mut self, delta: String) {
            self.text.push_str(&delta);
        }

        fn on_reasoning_delta(&mut self, delta: ReasoningDelta) {
            match delta {
                ReasoningDelta::SummaryText { delta, .. } | ReasoningDelta::Text { delta, .. } => {
                    self.reasoning_text.push_str(&delta);
                }
            }
        }
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
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            reasoning_effort: "medium".to_string(),
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let request = ModelRequest {
            messages: vec![ResponseItem::User {
                content: agent_core::text_input_items("hello"),
            }],
            tools: Vec::new(),
            temperature: 0.0,
            reasoning_effort: None,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let response = tokio::time::timeout(Duration::from_secs(1), async {
            let mut observer = TestObserver::default();
            model
                .complete_streaming(request, &mut observer)
                .await
                .map(|response| (response, observer))
        })
        .await
        .expect("stream should finish before socket closes")
        .expect("streaming request should succeed");

        assert_eq!(response.0.content.as_deref(), Some("hi"));
        assert_eq!(response.1.text, "hi");
        assert!(response.1.reasoning_text.is_empty());

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn streaming_preserves_usage_after_finish_reason_before_done() {
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
                "data: {\"id\":\"resp_1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "data: {\"id\":\"resp_1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"id\":\"resp_1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15,\"prompt_tokens_details\":{\"cached_tokens\":2},\"completion_tokens_details\":{\"reasoning_tokens\":1}}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Transfer-Encoding: chunked\r\n",
                "Connection: close\r\n",
                "\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write headers");
            stream
                .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                .expect("write chunk size");
            stream.write_all(body.as_bytes()).expect("write chunk body");
            stream
                .write_all(b"\r\n0\r\n\r\n")
                .expect("write terminating chunk");
            stream.flush().expect("flush response");
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            reasoning_effort: "medium".to_string(),
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let request = ModelRequest {
            messages: vec![ResponseItem::User {
                content: agent_core::text_input_items("hello"),
            }],
            tools: Vec::new(),
            temperature: 0.0,
            reasoning_effort: None,
            tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
        };

        let mut observer = TestObserver::default();
        let response = model
            .complete_streaming(request, &mut observer)
            .await
            .expect("streaming request should succeed");

        assert_eq!(response.content.as_deref(), Some("hi"));
        assert!(observer.reasoning_text.is_empty());
        let usage = response.usage.expect("usage should be preserved");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.cached_input_tokens, 2);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.reasoning_output_tokens, 1);
        assert_eq!(usage.total_tokens, 15);

        server.join().expect("server thread");
    }

}
