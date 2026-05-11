use super::types::FeishuMessageEnvelope;
use crate::message::ReplyContext;

pub fn build_reply_context(envelope: &FeishuMessageEnvelope) -> Option<ReplyContext> {
    let message_id = envelope.message.message_id.clone()?;
    let thread_id = envelope
        .message
        .root_id
        .clone()
        .or_else(|| envelope.message.parent_id.clone());

    Some(ReplyContext {
        message_id,
        thread_id,
    })
}
