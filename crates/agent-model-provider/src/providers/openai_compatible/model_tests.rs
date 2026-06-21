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
