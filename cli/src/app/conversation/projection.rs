use crate::ui::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_core::conversation::{ConversationTurn, TranscriptItem};
use agent_core::turn::TurnState;

pub(crate) struct ConversationProjection {
    pub(crate) completed_cells: Vec<HistoryTurnCells>,
    pub(crate) last_copyable_output: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct HistoryTurnCells {
    pub(crate) turn_id: String,
    pub(crate) cells: Vec<HistoryCell>,
}

pub(crate) fn project_conversation_history(
    history_snapshot: &[ConversationTurn],
) -> ConversationProjection {
    ConversationProjection {
        completed_cells: collect_completed_turn_cells(history_snapshot),
        last_copyable_output: find_last_copyable_output(history_snapshot),
    }
}

fn collect_completed_turn_cells(history_snapshot: &[ConversationTurn]) -> Vec<HistoryTurnCells> {
    history_snapshot
        .iter()
        .filter(|turn| turn.state != TurnState::Running)
        .map(|turn| HistoryTurnCells {
            turn_id: turn.id.clone(),
            cells: render_turn_items(&turn.items),
        })
        .collect()
}

fn find_last_copyable_output(history_snapshot: &[ConversationTurn]) -> Option<String> {
    history_snapshot
        .iter()
        .rev()
        .flat_map(|turn| turn.items.iter().rev())
        .find_map(|entry| {
            if let TranscriptItem::AgentMessage { text, .. } = entry {
                (!text.trim().is_empty()).then(|| text.clone())
            } else {
                None
            }
        })
}

fn render_turn_items(items: &[TranscriptItem]) -> Vec<HistoryCell> {
    let mut render_context = RenderContext;
    items
        .iter()
        .map(|item| render_history_entry(item, &mut render_context))
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>()
}

