use crate::model::{MemoryConfig, MemoryMode};
use agent_core::conversation::{ConversationHistory, ResponseItem};

pub fn should_persist(config: &MemoryConfig, history: &ConversationHistory) -> bool {
    if !config.enabled || config.mode != MemoryMode::Evolve {
        return false;
    }
    let turns = history
        .messages
        .iter()
        .filter(|m| matches!(m, ResponseItem::User { .. }))
        .count();
    turns >= config.min_turns_to_persist
}
