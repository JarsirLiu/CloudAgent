use super::wire::ChatCompletionStreamChunk;
use crate::error::ProviderStreamError;
use crate::event::{
    ProviderCompletion, ProviderMetadata, ProviderStreamEvent, ProviderToolCallDelta,
};
use agent_core::ModelUsage;
use tokio::sync::mpsc;

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

pub(super) fn parse_stream_frame(
    data: &str,
) -> Result<Vec<ProviderStreamEvent>, ProviderStreamError> {
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

    #[test]
    fn done_frame_maps_to_completed_event() {
        let events = parse_stream_frame("[DONE]").expect("parse done frame");
        assert_eq!(
            events,
            vec![ProviderStreamEvent::Completed(ProviderCompletion::default())]
        );
    }
}
