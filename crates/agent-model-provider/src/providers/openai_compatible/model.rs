use super::history::RequestHistory;
use super::stream::{ProviderEventStream, WireApi, parse_stream_frame};
use super::transform::{
    StreamingToolCallAcc, build_chat_request, build_responses_request, finalize_stream_tool_calls,
    parse_chat_response, parse_responses_response, tool_spec_index,
};
use super::wire::{ChatCompletionResponse, ResponsesResponse};
use crate::config::ProviderRuntimeConfig;
use crate::error::{ProviderRequestError, ProviderStreamError};
use crate::event::{ProviderCompletion, ProviderReasoningDelta, ProviderStreamEvent};
use crate::request::ProviderRequest;
use agent_core::model::{
    ChatModel, ModelRequest, ModelResponse, ModelRetryDecision, ModelStreamObserver, ModelUsage,
    ReasoningDelta,
};
use anyhow::Result;
use async_trait::async_trait;
use config::LlmConfig;
use infra_http::{build_http_client, spawn_sse_frame_stream};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

pub struct OpenAiCompatibleModel {
    client: Client,
    config: LlmConfig,
    runtime: ProviderRuntimeConfig,
    history: RequestHistory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WireStrategy {
    Chat,
    Responses,
}

impl OpenAiCompatibleModel {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = build_http_client()?;
        let runtime = ProviderRuntimeConfig::from(&config);
        Ok(Self {
            client,
            config,
            runtime,
            history: RequestHistory::default(),
        })
    }

    fn strategy(&self) -> WireStrategy {
        let base_url = self
            .config
            .base_url
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if base_url.ends_with("/chat/completions") || base_url.contains("/chat/completions") {
            WireStrategy::Chat
        } else {
            WireStrategy::Responses
        }
    }

    fn endpoint(&self, strategy: WireStrategy) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        match strategy {
            WireStrategy::Chat => {
                if base.to_ascii_lowercase().ends_with("/chat/completions") {
                    base.to_string()
                } else {
                    format!("{base}/chat/completions")
                }
            }
            WireStrategy::Responses => {
                if base.to_ascii_lowercase().ends_with("/responses") {
                    base.to_string()
                } else {
                    format!("{base}/responses")
                }
            }
        }
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

    fn stream_error_from_response(
        status: reqwest::StatusCode,
        body: String,
    ) -> ProviderStreamError {
        ProviderStreamError::Http {
            status: status.as_u16(),
            body,
        }
    }

    async fn prepare_provider_request(&self, request: &ModelRequest) -> Result<ProviderRequest> {
        ProviderRequest::from_model_request(request, self.runtime.supports_image_input()).await
    }

    async fn start_stream(
        &self,
        request: &ModelRequest,
        strategy: WireStrategy,
    ) -> Result<ProviderEventStream, ProviderStreamError> {
        let prepared_request = match strategy {
            WireStrategy::Chat => self.history.enrich_chat_request(request.clone()),
            WireStrategy::Responses => request.clone(),
        };
        let provider_request = self
            .prepare_provider_request(&prepared_request)
            .await
            .map_err(|err| ProviderStreamError::Transport {
                message: format!("failed to prepare LLM request: {err}"),
            })?;

        let request_builder = self
            .client
            .post(self.endpoint(strategy))
            .bearer_auth(&self.config.api_key);

        let request_builder = match strategy {
            WireStrategy::Chat => request_builder.json(&build_chat_request(
                &provider_request,
                &self.config.model,
                true,
            )),
            WireStrategy::Responses => request_builder.json(&build_responses_request(
                &provider_request,
                &self.config.model,
                true,
            )),
        };

        let timeout_ms = self.runtime.stream_idle_timeout.as_millis() as u64;
        let response = timeout(self.runtime.stream_idle_timeout, request_builder.send())
            .await
            .map_err(|_| ProviderStreamError::RequestTimeout {
                stage: "send",
                timeout_ms,
            })?
            .map_err(|err| ProviderStreamError::Transport {
                message: format!("failed to send streaming LLM request: {err}"),
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::stream_error_from_response(status, body));
        }

        let wire_api = match strategy {
            WireStrategy::Chat => WireApi::Chat,
            WireStrategy::Responses => WireApi::Responses,
        };
        let mut sse_frames = spawn_sse_frame_stream(response, self.runtime.stream_idle_timeout);
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut pending_completion: Option<ProviderCompletion> = None;
            while let Some(frame) = sse_frames.recv().await {
                match frame {
                    Ok(block) => match parse_stream_frame(wire_api, &block) {
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

    async fn complete_with_chat(&self, request: ModelRequest) -> Result<ModelResponse> {
        let request = self.history.enrich_chat_request(request);
        let provider_request = self.prepare_provider_request(&request).await?;
        let tool_index = tool_spec_index(&provider_request.tools);
        let payload = build_chat_request(&provider_request, &self.config.model, false);

        let timeout_ms = self.runtime.stream_idle_timeout.as_millis() as u64;
        let response = timeout(
            self.runtime.stream_idle_timeout,
            self.client
                .post(self.endpoint(WireStrategy::Chat))
                .bearer_auth(&self.config.api_key)
                .json(&payload)
                .send(),
        )
        .await
        .map_err(|_| {
            anyhow::Error::new(ProviderRequestError::Timeout {
                stage: "send",
                timeout_ms,
            })
        })?
        .map_err(|err| {
            anyhow::Error::new(ProviderRequestError::Transport {
                message: format!("failed to send LLM request: {err}"),
            })
        })?;

        let status = response.status();
        let body = timeout(self.runtime.stream_idle_timeout, response.text())
            .await
            .map_err(|_| {
                anyhow::Error::new(ProviderRequestError::Timeout {
                    stage: "read body",
                    timeout_ms,
                })
            })?
            .map_err(|err| {
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
        parse_chat_response(parsed, &tool_index)
    }

    async fn complete_with_responses(&self, request: ModelRequest) -> Result<ModelResponse> {
        let provider_request = self.prepare_provider_request(&request).await?;
        let tool_index = tool_spec_index(&provider_request.tools);
        let payload = build_responses_request(&provider_request, &self.config.model, false);

        let timeout_ms = self.runtime.stream_idle_timeout.as_millis() as u64;
        let response = timeout(
            self.runtime.stream_idle_timeout,
            self.client
                .post(self.endpoint(WireStrategy::Responses))
                .bearer_auth(&self.config.api_key)
                .json(&payload)
                .send(),
        )
        .await
        .map_err(|_| {
            anyhow::Error::new(ProviderRequestError::Timeout {
                stage: "send",
                timeout_ms,
            })
        })?
        .map_err(|err| {
            anyhow::Error::new(ProviderRequestError::Transport {
                message: format!("failed to send LLM request: {err}"),
            })
        })?;

        let status = response.status();
        let body = timeout(self.runtime.stream_idle_timeout, response.text())
            .await
            .map_err(|_| {
                anyhow::Error::new(ProviderRequestError::Timeout {
                    stage: "read body",
                    timeout_ms,
                })
            })?
            .map_err(|err| {
                anyhow::Error::new(ProviderRequestError::Transport {
                    message: format!("failed to read LLM body: {err}"),
                })
            })?;
        if !status.is_success() {
            return Err(anyhow::Error::new(Self::request_error_from_response(
                status, body,
            )));
        }

        let parsed: ResponsesResponse = serde_json::from_str(&body).map_err(|err| {
            anyhow::Error::new(ProviderRequestError::Protocol {
                message: format!("failed to parse responses response: {err}"),
            })
        })?;
        Ok(parse_responses_response(parsed, &tool_index))
    }

    async fn stream_with_strategy(
        &self,
        request: ModelRequest,
        strategy: WireStrategy,
        observer: &mut dyn ModelStreamObserver,
    ) -> Result<ModelResponse> {
        let tool_index = tool_spec_index(&request.tools);
        let mut stream = self
            .start_stream(&request, strategy)
            .await
            .map_err(anyhow::Error::new)?;

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut model_name: Option<String> = None;
        let mut usage: Option<ModelUsage> = None;
        let mut completion: Option<ProviderCompletion> = None;
        let mut tool_calls_acc: HashMap<usize, StreamingToolCallAcc> = HashMap::new();
        let mut saw_provider_event = false;
        let timeout_ms = self.runtime.stream_idle_timeout.as_millis() as u64;

        loop {
            let maybe_event = timeout(self.runtime.stream_idle_timeout, stream.recv())
                .await
                .map_err(|_| {
                    anyhow::Error::new(ProviderStreamError::RequestTimeout {
                        stage: if saw_provider_event {
                            "next event"
                        } else {
                            "first event"
                        },
                        timeout_ms,
                    })
                })?;
            let Some(event) = maybe_event else {
                break;
            };
            saw_provider_event = true;
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
                        if delta.arguments_replace {
                            acc.arguments = arguments_delta;
                        } else {
                            acc.arguments.push_str(&arguments_delta);
                        }
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

        let completion = completion
            .ok_or_else(|| anyhow::Error::new(ProviderStreamError::ClosedBeforeCompletion))?;

        Ok(ModelResponse {
            content: (!content.is_empty()).then_some(content),
            reasoning: (!reasoning.is_empty()).then_some(reasoning),
            tool_calls: finalize_stream_tool_calls(tool_calls_acc, &tool_index),
            finish_reason: completion.finish_reason,
            model_name,
            usage,
        })
    }
}

#[async_trait]
impl ChatModel for OpenAiCompatibleModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        match self.strategy() {
            WireStrategy::Chat => self.complete_with_chat(request).await,
            WireStrategy::Responses => match self.complete_with_responses(request.clone()).await {
                Ok(response) => Ok(response),
                Err(err) => {
                    if err
                        .downcast_ref::<ProviderRequestError>()
                        .is_some_and(|error| matches!(
                            error,
                            ProviderRequestError::Http { status, .. } if *status == 404 || *status == 400
                        ))
                    {
                        self.complete_with_chat(request).await
                    } else {
                        Err(err)
                    }
                }
            },
        }
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
                ProviderRequestError::Timeout { .. } => ModelRetryDecision::retry(None),
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
                ProviderStreamError::RequestTimeout { .. }
                | ProviderStreamError::FirstFrameTimeout
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
        match self.strategy() {
            WireStrategy::Chat => {
                self.stream_with_strategy(request, WireStrategy::Chat, observer)
                    .await
            }
            WireStrategy::Responses => {
                match self
                    .stream_with_strategy(request.clone(), WireStrategy::Responses, observer)
                    .await
                {
                    Ok(response) => Ok(response),
                    Err(err) => {
                        if err
                            .downcast_ref::<ProviderStreamError>()
                            .is_some_and(|error| matches!(
                                error,
                                ProviderStreamError::Http { status, .. } if *status == 404 || *status == 400
                            ))
                        {
                            self.stream_with_strategy(request, WireStrategy::Chat, observer)
                                .await
                        } else {
                            Err(err)
                        }
                    }
                }
            }
        }
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
    async fn streaming_times_out_when_provider_sends_no_model_events() {
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
            for _ in 0..6 {
                let body = "data: \n\n";
                stream
                    .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                    .expect("write chunk size");
                stream.write_all(body.as_bytes()).expect("write chunk body");
                stream.write_all(b"\r\n").expect("write chunk suffix");
                stream.flush().expect("flush empty event");
                thread::sleep(Duration::from_millis(250));
            }
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}/chat/completions"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let mut observer = TestObserver::default();
        let err = model
            .complete_streaming(
                ModelRequest {
                    messages: vec![ResponseItem::User {
                        content: agent_core::text_input_items("hello"),
                    }],
                    tools: Vec::new(),
                    temperature: 0.0,
                    reasoning_effort: None,
                    tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
                },
                &mut observer,
            )
            .await
            .expect_err("streaming should time out without provider events");

        let stream_error = err
            .downcast_ref::<ProviderStreamError>()
            .expect("provider stream error");
        assert!(matches!(
            stream_error,
            ProviderStreamError::RequestTimeout {
                stage: "first event",
                timeout_ms: 1_000
            }
        ));

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn streaming_times_out_when_provider_never_returns_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
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
            thread::sleep(Duration::from_millis(1_500));
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}/chat/completions"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let mut observer = TestObserver::default();
        let err = model
            .complete_streaming(
                ModelRequest {
                    messages: vec![ResponseItem::User {
                        content: agent_core::text_input_items("hello"),
                    }],
                    tools: Vec::new(),
                    temperature: 0.0,
                    reasoning_effort: None,
                    tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
                },
                &mut observer,
            )
            .await
            .expect_err("streaming should time out before response headers");

        let stream_error = err
            .downcast_ref::<ProviderStreamError>()
            .expect("provider stream error");
        assert!(matches!(
            stream_error,
            ProviderStreamError::RequestTimeout {
                stage: "send",
                timeout_ms: 1_000
            }
        ));
        assert!(
            err.to_string()
                .contains("provider stream request timeout during send after 1000ms")
        );

        server.join().expect("server thread");
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
            base_url: format!("http://{addr}/chat/completions"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
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
            base_url: format!("http://{addr}/chat/completions"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
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

    #[tokio::test]
    async fn complete_uses_responses_when_available() {
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
            let mut buf = [0u8; 4096];
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

            let body = r#"{"model":"test-model","status":"completed","output":[{"type":"reasoning","summary":[{"type":"summary_text","text":"think"}]},{"type":"message","content":[{"type":"output_text","text":"hello from responses"}]}],"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush response");
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let response = model
            .complete(ModelRequest {
                messages: vec![ResponseItem::User {
                    content: agent_core::text_input_items("hello"),
                }],
                tools: Vec::new(),
                temperature: 0.0,
                reasoning_effort: None,
                tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
            })
            .await
            .expect("responses request should succeed");

        assert_eq!(response.content.as_deref(), Some("hello from responses"));
        assert_eq!(response.reasoning.as_deref(), Some("think"));
        assert_eq!(response.model_name.as_deref(), Some("test-model"));
        let usage = response.usage.expect("usage");
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, 3);

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn complete_falls_back_to_chat_when_responses_is_unsupported() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept client");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("set read timeout");
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .expect("set write timeout");

                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
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

                if attempt == 0 {
                    let body = r#"{"error":"unsupported"}"#;
                    let response = format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write 404 response");
                } else {
                    let body = r#"{"model":"test-model","choices":[{"message":{"content":"hello from chat","reasoning_content":"fallback think","tool_calls":[]},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write chat response");
                }
                stream.flush().expect("flush response");
            }
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let response = model
            .complete(ModelRequest {
                messages: vec![ResponseItem::User {
                    content: agent_core::text_input_items("hello"),
                }],
                tools: Vec::new(),
                temperature: 0.0,
                reasoning_effort: None,
                tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
            })
            .await
            .expect("fallback to chat should succeed");

        assert_eq!(response.content.as_deref(), Some("hello from chat"));
        assert_eq!(response.reasoning.as_deref(), Some("fallback think"));
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn streaming_uses_responses_when_available() {
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
                "event: response.created\n",
                "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"test-model\"}}\n\n",
                "event: response.reasoning_summary_text.delta\n",
                "data: {\"type\":\"response.reasoning_summary_text.delta\",\"summary_index\":0,\"delta\":\"think\"}\n\n",
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello from responses\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"test-model\",\"usage\":{\"input_tokens\":4,\"output_tokens\":5,\"total_tokens\":9}}}\n\n"
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
            model_reasoning_effort: config::ReasoningEffort::Medium,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let mut observer = TestObserver::default();
        let response = model
            .complete_streaming(
                ModelRequest {
                    messages: vec![ResponseItem::User {
                        content: agent_core::text_input_items("hello"),
                    }],
                    tools: Vec::new(),
                    temperature: 0.0,
                    reasoning_effort: None,
                    tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
                },
                &mut observer,
            )
            .await
            .expect("responses stream should succeed");

        assert_eq!(response.content.as_deref(), Some("hello from responses"));
        assert_eq!(response.reasoning.as_deref(), Some("think"));
        assert_eq!(observer.text, "hello from responses");
        assert_eq!(observer.reasoning_text, "think");
        assert_eq!(response.model_name.as_deref(), Some("test-model"));
        let usage = response.usage.expect("usage");
        assert_eq!(usage.input_tokens, 4);
        assert_eq!(usage.output_tokens, 5);

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn streaming_falls_back_to_chat_when_responses_stream_is_unsupported() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            for attempt in 0..2 {
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

                if attempt == 0 {
                    let body = r#"{"error":"unsupported"}"#;
                    let response = format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write 404 response");
                } else {
                    let body = concat!(
                        "data: {\"id\":\"resp_1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"fallback think\"}}]}\n\n",
                        "data: {\"id\":\"resp_1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello from chat\"}}]}\n\n",
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
                }
                stream.flush().expect("flush response");
            }
        });

        let model = OpenAiCompatibleModel::new(LlmConfig {
            base_url: format!("http://{addr}"),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            input_modalities: default_input_modalities(),
            temperature: 0.0,
            model_reasoning_effort: config::ReasoningEffort::Medium,
            request_max_retries: 0,
            stream_max_retries: 0,
            stream_idle_timeout_ms: 1_000,
        })
        .expect("build model");

        let mut observer = TestObserver::default();
        let response = model
            .complete_streaming(
                ModelRequest {
                    messages: vec![ResponseItem::User {
                        content: agent_core::text_input_items("hello"),
                    }],
                    tools: Vec::new(),
                    temperature: 0.0,
                    reasoning_effort: None,
                    tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
                },
                &mut observer,
            )
            .await
            .expect("fallback stream should succeed");

        assert_eq!(response.content.as_deref(), Some("hello from chat"));
        assert_eq!(response.reasoning.as_deref(), Some("fallback think"));
        assert_eq!(observer.text, "hello from chat");
        assert_eq!(observer.reasoning_text, "fallback think");

        server.join().expect("server thread");
    }
}
