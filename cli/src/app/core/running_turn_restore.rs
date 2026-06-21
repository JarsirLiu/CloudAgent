use crate::ui::history_cell::{
    HistoryCell, RenderContext, render_active_runtime_item, render_history_entry,
};
use agent_core::conversation::ConversationTurn;
use agent_core::conversation::TranscriptItem;
use agent_core::{RuntimeItemSnapshot, TurnItemKind};
use std::collections::HashSet;

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
    let runtime_item_ids = turn
        .runtime_items
        .iter()
        .map(|snapshot| snapshot.item.id.as_str())
        .collect::<HashSet<_>>();
    let last_copyable_output =
        runtime_snapshot_copyable_output(&turn.runtime_items).or_else(|| {
            turn.items.iter().rev().find_map(|item| {
                if let TranscriptItem::AgentMessage { text, .. } = item {
                    (!text.trim().is_empty()).then(|| text.clone())
                } else {
                    None
                }
            })
        });

    for item in turn.items {
        if runtime_item_ids.contains(item.id()) {
            continue;
        }
        let cell = render_history_entry(&item, &mut context);
        if cell.is_empty() {
            continue;
        }

        if turn.runtime_items.is_empty() && is_live_tail_candidate(&item) {
            if let Some(previous_cell) = last_live_cell.replace(cell) {
                replay_cells.push(previous_cell);
            }
        } else {
            replay_cells.push(cell);
        }
    }

    if let Some(live_cell) = runtime_live_cell(&turn.runtime_items) {
        live_cells.push(live_cell);
    } else if let Some(live_cell) = last_live_cell {
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

fn runtime_live_cell(runtime_items: &[RuntimeItemSnapshot]) -> Option<HistoryCell> {
    runtime_items
        .iter()
        .rev()
        .find_map(render_runtime_snapshot_cell)
}

fn runtime_snapshot_copyable_output(runtime_items: &[RuntimeItemSnapshot]) -> Option<String> {
    runtime_items.iter().rev().find_map(|snapshot| {
        matches!(snapshot.item.kind, TurnItemKind::AssistantMessage)
            .then(|| snapshot.text_buffer.trim())
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

fn render_runtime_snapshot_cell(snapshot: &RuntimeItemSnapshot) -> Option<HistoryCell> {
    match snapshot.item.kind {
        TurnItemKind::CommandExecution => None,
        TurnItemKind::AssistantMessage => Some(HistoryCell::agent(
            "",
            if snapshot.text_buffer.trim().is_empty() {
                "responding".to_string()
            } else {
                snapshot.text_buffer.clone()
            },
            crate::ui::history_cell::HistoryFormat::Markdown,
        )),
        TurnItemKind::Reasoning => Some(HistoryCell::reasoning(
            "Reasoning",
            if snapshot.reasoning_buffer.trim().is_empty() {
                "thinking".to_string()
            } else {
                snapshot.reasoning_buffer.clone()
            },
        )),
        _ => {
            let mut cell = render_active_runtime_item(&snapshot.item);
            if let Some(body) = runtime_snapshot_body(snapshot)
                && !body.trim().is_empty()
            {
                match cell.body().trim() {
                    "" | "running" => cell.replace_body(body),
                    current if current == body.trim() => {}
                    _ => cell.append_body(&body),
                }
            }
            if !snapshot.patch_buffer.trim().is_empty() {
                cell.append_detail(&snapshot.patch_buffer);
            }
            Some(cell)
        }
    }
}

fn runtime_snapshot_body(snapshot: &RuntimeItemSnapshot) -> Option<String> {
    if !snapshot.tool_output_buffer.trim().is_empty() {
        return Some(snapshot.tool_output_buffer.clone());
    }
    snapshot
        .item
        .progress
        .as_ref()
        .and_then(|progress| progress.message.clone())
        .or_else(|| snapshot.item.summary.clone())
}
