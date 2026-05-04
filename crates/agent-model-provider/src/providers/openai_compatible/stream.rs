use super::wire::ChatCompletionStreamChunk;
use crate::error::ProviderStreamError;
use crate::event::{
    ProviderCompletion, ProviderMetadata, ProviderStreamEvent, ProviderToolCallDelta,
};
use agent_core::ModelUsage;
use tokio::sync::mpsc;

pub(super) struct ParsedStreamFrame {
    pub events: Vec<ProviderStreamEvent>,
    pub completion: Option<ProviderCompletion>,
    pub done: bool,
}

pub(super) struct ProviderEventStream {
    rx: mpsc::Receiver<Result<ProviderStreamEvent, ProviderStreamError>>,
}

impl ProviderEventStream {
    pub(super) fn new(
        rx: mpsc::Receiver<Result<ProviderStreamEvent, ProviderStreamError>>,
    ) -> Self {
        Self { rx }
    }

    pub(super) async fn recv(
        &mut self,
    ) -> Option<Result<ProviderStreamEvent, ProviderStreamError>> {
        self.rx.recv().await
    }
}

pub(super) fn parse_stream_frame(data: &str) -> Result<ParsedStreamFrame, ProviderStreamError> {
    if data == "[DONE]" {
        return Ok(ParsedStreamFrame {
            events: Vec::new(),
            completion: None,
            done: true,
        });
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

    Ok(ParsedStreamFrame {
        events,
        completion: saw_completion.then_some(completion),
        done: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_frame_maps_to_completed_event() {
        let frame = parse_stream_frame("[DONE]").expect("parse done frame");
        assert!(frame.events.is_empty());
        assert!(frame.completion.is_none());
        assert!(frame.done);
    }

    #[test]
    fn finish_reason_is_deferred_until_done() {
        let frame = parse_stream_frame(
            r#"{"id":"resp_1","object":"chat.completion.chunk","created":0,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        )
        .expect("parse finish_reason frame");
        assert!(
            frame
                .events
                .iter()
                .all(|event| !matches!(event, ProviderStreamEvent::Completed(_)))
        );
        assert_eq!(
            frame.completion,
            Some(ProviderCompletion {
                finish_reason: Some("stop".to_string()),
                end_turn: None,
            })
        );
        assert!(!frame.done);
    }
}
