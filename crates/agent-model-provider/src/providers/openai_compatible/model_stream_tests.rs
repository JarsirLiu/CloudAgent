use super::OpenAiCompatibleModel;
use crate::error::ProviderStreamError;
use agent_core::ResponseItem;
use agent_core::model::{ChatModel, ModelRequest, ModelStreamObserver, ReasoningDelta};
use config::{LlmConfig, default_input_modalities};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

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
async fn streaming_times_out_when_provider_only_sends_non_substantive_events() {
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
            let body = concat!(
                "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",",
                "\"created\":1,\"model\":\"test-model\",\"choices\":[{\"index\":0,\"delta\":{}}]}\n\n"
            );
            stream
                .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                .expect("write chunk size");
            stream.write_all(body.as_bytes()).expect("write chunk body");
            stream.write_all(b"\r\n").expect("write chunk suffix");
            stream.flush().expect("flush metadata event");
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
        .expect_err("streaming should time out before substantive content");

    let stream_error = err
        .downcast_ref::<ProviderStreamError>()
        .expect("provider stream error");
    assert!(matches!(
        stream_error,
        ProviderStreamError::RequestTimeout {
            stage: "first content",
            timeout_ms: 1_000
        }
    ));

    server.join().expect("server thread");
}
