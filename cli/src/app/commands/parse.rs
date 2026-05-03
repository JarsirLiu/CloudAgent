use crate::state::reducer::{UiInputEvent, apply_ui_event};
use agent_protocol::FrontendMode;

pub(crate) type ParsedInput = UiInputEvent;

pub(crate) fn parse_line(line: &str, conversation_id: &str, mode: FrontendMode) -> ParsedInput {
    apply_ui_event(line, conversation_id, mode)
}
