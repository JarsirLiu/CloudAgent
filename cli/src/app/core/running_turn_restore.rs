use crate::ui::widgets::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_core::conversation::ConversationTurn;
use agent_core::conversation::TranscriptItem;

pub(crate) struct RunningTurnRestoreResult {
    pub(crate) replay_cells: Vec<HistoryCell>,
    pub(crate) live_cells: Vec<HistoryCell>,
    pub(crate) last_copyable_output: Option<String>,
}

pub(crate) fn restore_running_turn_cells(turn: ConversationTurn) -> RunningTurnRestoreResult {
    let mut replay_cells = Vec::new();
    let mut live_cells = Vec::new();
    let mut last_live_cell: Option<HistoryCell> = None;
    let mut context = RenderContext;
    let last_copyable_output = turn.items.iter().rev().find_map(|item| {
        if let TranscriptItem::AgentMessage { text, .. } = item {
            (!text.trim().is_empty()).then(|| text.clone())
        } else {
            None
        }
    });

    for item in turn.items {
        let cell = render_history_entry(&item, &mut context);
        if cell.is_empty() {
            continue;
        }

        if is_live_tail_candidate(&item) {
            if let Some(previous_cell) = last_live_cell.replace(cell) {
                replay_cells.push(previous_cell);
            }
        } else {
            replay_cells.push(cell);
        }
    }

    if let Some(live_cell) = last_live_cell {
        live_cells.push(live_cell);
    }

    RunningTurnRestoreResult {
        replay_cells,
        live_cells,
        last_copyable_output,
    }
}

pub(crate) fn is_live_tail_candidate(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::AgentMessage { .. }
            | TranscriptItem::Reasoning { .. }
            | TranscriptItem::CommandExecution { .. }
            | TranscriptItem::ToolResult { .. }
            | TranscriptItem::FileChange { .. }
    )
}
