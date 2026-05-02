use agent_core::{ConversationHistory, ResponseItem};
use agent_protocol::EventMsg;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn visible_message_count(history: &ConversationHistory) -> usize {
    history
        .messages
        .iter()
        .filter(|message| match message {
            ResponseItem::User { content } => !content.trim().is_empty(),
            ResponseItem::Assistant { content, .. } => content
                .as_deref()
                .is_some_and(|content| !content.trim().is_empty()),
            ResponseItem::System { .. } | ResponseItem::Tool { .. } => false,
        })
        .count()
}

pub(crate) fn model_shell_name() -> &'static str {
    if cfg!(windows) { "powershell" } else { "sh" }
}

pub(crate) fn is_turn_interrupted_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string() == crate::TURN_INTERRUPTED_ERROR)
}

pub(crate) fn next_turn_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("turn-{now}")
}

pub(crate) fn emit_event<E>(events: &mut Vec<EventMsg>, on_event: &mut E, event: EventMsg)
where
    E: FnMut(&EventMsg),
{
    events.push(event.clone());
    on_event(&event);
}

pub(crate) fn summarize_arguments(arguments: &Value) -> String {
    let rendered =
        serde_json::to_string(arguments).unwrap_or_else(|_| "<invalid-json>".to_string());
    if rendered.chars().count() > 240 {
        let truncated = rendered.chars().take(240).collect::<String>();
        format!("{truncated}...")
    } else {
        rendered
    }
}
