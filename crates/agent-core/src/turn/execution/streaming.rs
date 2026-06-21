use super::TurnHost;
use crate::model::{
    ModelRequest, ModelResponse, ModelStreamObserver, ReasoningDelta, WebSearchAction,
};
use crate::web_search_presentation::web_search_transcript_item;
use crate::{EventMsg, TranscriptItem, TurnItemDeltaKind, TurnItemKind, emit_event};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(super) struct StreamedModelResponse {
    pub(super) response: ModelResponse,
    pub(super) had_streaming_assistant_item: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn observe_model_response_stream<H, E>(
    host: &H,
    cancellation_token: &CancellationToken,
    model_request: ModelRequest,
    turn_id: &str,
    assistant_item_seq: &mut usize,
    reasoning_item_seq: &mut usize,
    events: &mut Vec<EventMsg>,
    on_event: &mut E,
) -> Result<StreamedModelResponse>
where
    H: TurnHost,
    E: FnMut(&EventMsg) + Send + ?Sized,
{
    let mut streaming_assistant_item_id: Option<String> = None;
    let mut streaming_reasoning_item_id: Option<String> = None;
    let mut reasoning_text_buffer = String::new();
    let mut stream_observer = TurnStreamObserver {
        turn_id,
        assistant_item_seq,
        streaming_assistant_item_id: &mut streaming_assistant_item_id,
        reasoning_item_seq,
        streaming_reasoning_item_id: &mut streaming_reasoning_item_id,
        reasoning_text_buffer: &mut reasoning_text_buffer,
        events,
        on_event,
    };
    let response = host
        .complete_model_request_streaming(cancellation_token, model_request, &mut stream_observer)
        .await?;

    let had_streaming_assistant_item = streaming_assistant_item_id.is_some();
    if let Some(item_id) = streaming_assistant_item_id.take() {
        emit_event(
            events,
            on_event,
            EventMsg::ItemCompleted {
                turn_id: turn_id.to_string(),
                item_id: item_id.clone(),
                call_id: None,
                item: TranscriptItem::AgentMessage {
                    id: item_id,
                    text: response.content.clone().unwrap_or_default(),
                },
            },
        );
    }
    if let Some(item_id) = streaming_reasoning_item_id.take() {
        emit_event(
            events,
            on_event,
            EventMsg::ItemCompleted {
                turn_id: turn_id.to_string(),
                item_id: item_id.clone(),
                call_id: None,
                item: TranscriptItem::Reasoning {
                    id: item_id,
                    title: "reasoning".to_string(),
                    text: response
                        .reasoning
                        .clone()
                        .unwrap_or_else(|| reasoning_text_buffer.clone()),
                },
            },
        );
    }

    Ok(StreamedModelResponse {
        response,
        had_streaming_assistant_item,
    })
}

struct TurnStreamObserver<'a, E: FnMut(&EventMsg) + ?Sized> {
    turn_id: &'a str,
    assistant_item_seq: &'a mut usize,
    streaming_assistant_item_id: &'a mut Option<String>,
    reasoning_item_seq: &'a mut usize,
    streaming_reasoning_item_id: &'a mut Option<String>,
    reasoning_text_buffer: &'a mut String,
    events: &'a mut Vec<EventMsg>,
    on_event: &'a mut E,
}

impl<E: FnMut(&EventMsg) + Send + ?Sized> ModelStreamObserver for TurnStreamObserver<'_, E> {
    fn on_text_delta(&mut self, delta: String) {
        if delta.is_empty() {
            return;
        }
        let item_id = self.streaming_assistant_item_id.get_or_insert_with(|| {
            let id = format!("assistant:{}:{}", self.turn_id, *self.assistant_item_seq);
            *self.assistant_item_seq += 1;
            emit_event(
                self.events,
                self.on_event,
                EventMsg::ItemStarted {
                    turn_id: self.turn_id.to_string(),
                    item_id: id.clone(),
                    call_id: None,
                    kind: TurnItemKind::AssistantMessage,
                    title: Some("assistant_message".to_string()),
                },
            );
            id
        });
        emit_event(
            self.events,
            self.on_event,
            EventMsg::ItemDelta {
                turn_id: self.turn_id.to_string(),
                item_id: item_id.clone(),
                call_id: None,
                kind: TurnItemDeltaKind::Text,
                segment_index: None,
                delta,
            },
        );
    }

    fn on_reasoning_delta(&mut self, delta: ReasoningDelta) {
        let (kind, segment_index, delta): (TurnItemDeltaKind, Option<usize>, String) = match delta {
            ReasoningDelta::SummaryText {
                summary_index,
                delta,
            } => (
                TurnItemDeltaKind::ReasoningSummary,
                Some(summary_index),
                delta,
            ),
            ReasoningDelta::Text {
                content_index,
                delta,
            } => (TurnItemDeltaKind::ReasoningText, Some(content_index), delta),
        };
        if delta.is_empty() {
            return;
        }
        let item_id = self.streaming_reasoning_item_id.get_or_insert_with(|| {
            let id = format!("reasoning:{}:{}", self.turn_id, *self.reasoning_item_seq);
            *self.reasoning_item_seq += 1;
            emit_event(
                self.events,
                self.on_event,
                EventMsg::ItemStarted {
                    turn_id: self.turn_id.to_string(),
                    item_id: id.clone(),
                    call_id: None,
                    kind: TurnItemKind::Reasoning,
                    title: Some("reasoning".to_string()),
                },
            );
            id
        });
        self.reasoning_text_buffer.push_str(&delta);
        emit_event(
            self.events,
            self.on_event,
            EventMsg::ItemDelta {
                turn_id: self.turn_id.to_string(),
                item_id: item_id.clone(),
                call_id: None,
                kind,
                segment_index,
                delta,
            },
        );
    }

    fn on_retry(
        &mut self,
        stage: crate::ModelRetryStage,
        attempt: u64,
        delay: std::time::Duration,
    ) {
        emit_event(
            self.events,
            self.on_event,
            EventMsg::ModelRetrying {
                turn_id: self.turn_id.to_string(),
                stage,
                attempt,
                next_delay_ms: delay.as_millis().try_into().unwrap_or(u64::MAX),
            },
        );
    }

    fn on_web_search_started(&mut self, id: String, query: String) {
        emit_event(
            self.events,
            self.on_event,
            EventMsg::ItemStarted {
                turn_id: self.turn_id.to_string(),
                item_id: id.clone(),
                call_id: Some(id.clone()),
                kind: TurnItemKind::ToolResult,
                title: Some("web_search".to_string()),
            },
        );
        if !query.trim().is_empty() {
            emit_event(
                self.events,
                self.on_event,
                EventMsg::ItemDelta {
                    turn_id: self.turn_id.to_string(),
                    item_id: id.clone(),
                    call_id: Some(id),
                    kind: TurnItemDeltaKind::ToolOutput,
                    segment_index: None,
                    delta: query,
                },
            );
        }
    }

    fn on_web_search_completed(
        &mut self,
        id: String,
        query: String,
        action: Option<WebSearchAction>,
    ) {
        emit_event(
            self.events,
            self.on_event,
            EventMsg::ItemCompleted {
                turn_id: self.turn_id.to_string(),
                item_id: id.clone(),
                call_id: Some(id.clone()),
                item: web_search_transcript_item(id, query, action),
            },
        );
    }
}
