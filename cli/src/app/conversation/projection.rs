use crate::ui::widgets::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_protocol::{ConversationTurn, TranscriptItem, TurnState};

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
    let mut cells = items
        .iter()
        .map(|item| render_history_entry(item, &mut render_context))
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>();
    mark_turn_stream_continuations(&mut cells);
    cells
}

pub(crate) fn mark_turn_stream_continuations(cells: &mut [HistoryCell]) {
    let mut previous_was_agent_message = false;
    for cell in cells {
        if cell.is_empty() {
            cell.set_stream_continuation(false);
            previous_was_agent_message = false;
            continue;
        }
        let is_agent_message = matches!(
            cell.tone,
            crate::ui::widgets::history_cell::HistoryTone::Agent
        ) && matches!(cell.kind(), crate::ui::widgets::history_cell::HistoryKind::Message);
        cell.set_stream_continuation(is_agent_message && previous_was_agent_message);
        previous_was_agent_message = is_agent_message;
    }
}
