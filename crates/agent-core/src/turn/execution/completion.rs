use super::TurnHost;
use crate::{
    ContextManager, EventMsg, RolloutItem, TurnOutcome, TurnState, emit_assistant_message_item,
    emit_event,
};
use anyhow::Result;

pub(super) fn cancelled_outcome(
    turn_id: &str,
    events: Vec<EventMsg>,
    history: crate::ConversationHistory,
    model_name: Option<String>,
) -> TurnOutcome {
    TurnOutcome {
        turn_id: turn_id.to_string(),
        events,
        history,
        model_name,
        state: TurnState::Cancelled,
    }
}

pub(super) fn completed_outcome(
    turn_id: &str,
    events: Vec<EventMsg>,
    history: crate::ConversationHistory,
    model_name: Option<String>,
) -> TurnOutcome {
    TurnOutcome {
        turn_id: turn_id.to_string(),
        events,
        history,
        model_name,
        state: TurnState::Completed,
    }
}

pub(super) fn failed_outcome(
    turn_id: &str,
    events: Vec<EventMsg>,
    history: crate::ConversationHistory,
    model_name: Option<String>,
) -> TurnOutcome {
    TurnOutcome {
        turn_id: turn_id.to_string(),
        events,
        history,
        model_name,
        state: TurnState::Failed,
    }
}

pub(super) fn emit_turn_completed(
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + ?Sized),
    turn_id: &str,
) {
    emit_event(
        events,
        on_event,
        EventMsg::TurnCompleted {
            turn_id: turn_id.to_string(),
        },
    );
}

pub(super) fn emit_turn_cancelled(
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + ?Sized),
    turn_id: &str,
    reason: &str,
) {
    emit_event(
        events,
        on_event,
        EventMsg::TurnCancelled {
            turn_id: turn_id.to_string(),
            reason: reason.to_string(),
        },
    );
}

pub(super) async fn fail_turn_with_message<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    message: String,
    context_manager: &mut ContextManager,
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + Send + ?Sized),
    assistant_item_seq: &mut usize,
    model_name: Option<String>,
) -> Result<TurnOutcome> {
    emit_assistant_message_item(events, on_event, turn_id, &message, assistant_item_seq);
    let failed_item =
        context_manager.record_assistant_message(Some(message.clone()), None, Vec::new());
    host.persist_rollout_items(conversation_id, &[RolloutItem::from(failed_item)])
        .await?;
    host.save_history(context_manager.history().clone()).await?;
    emit_event(
        events,
        on_event,
        EventMsg::TurnFailed {
            turn_id: turn_id.to_string(),
            error: message,
        },
    );
    Ok(failed_outcome(
        turn_id,
        std::mem::take(events),
        context_manager.history().clone(),
        model_name,
    ))
}

pub(super) async fn complete_turn_with_message<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    message: String,
    context_manager: &mut ContextManager,
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + Send + ?Sized),
    assistant_item_seq: &mut usize,
    model_name: Option<String>,
) -> Result<TurnOutcome> {
    emit_assistant_message_item(events, on_event, turn_id, &message, assistant_item_seq);
    let item = context_manager.record_assistant_message(Some(message), None, Vec::new());
    host.persist_rollout_items(conversation_id, &[RolloutItem::from(item)])
        .await?;
    host.save_history(context_manager.history().clone()).await?;
    emit_turn_completed(events, on_event, turn_id);
    Ok(completed_outcome(
        turn_id,
        std::mem::take(events),
        context_manager.history().clone(),
        model_name,
    ))
}
