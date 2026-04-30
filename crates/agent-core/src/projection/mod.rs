mod transcript;
mod turn_output;

pub use transcript::{
    ConversationHistoryBuilder, TranscriptBuilder, build_turns_from_rollout_items,
    conversation_history_from_rollout_items, flatten_conversation_turns,
    transcript_item_from_response_item, transcript_items_from_response_items,
    transcript_items_from_rollout_items,
};
pub use turn_output::{agent_turn_output_from_events, tool_events_from_turn_events};

pub fn module_name() -> &'static str {
    "agent-core::projection"
}
