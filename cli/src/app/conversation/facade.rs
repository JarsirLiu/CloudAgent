use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{RenderContext, render_history_entry};
use agent_protocol::{ConversationTurn, TranscriptItem, TurnState};

pub(crate) fn rebuild_transcript_from_history(app: &mut TuiApp) {
    app.transcript_state = crate::state::TranscriptState::default();
    app.input_pane.clear_views();

    let history_snapshot = app.run_state.history_snapshot.clone().unwrap_or_default();
    if !history_snapshot.is_empty() {
        let mut render_context = RenderContext;
        let cells = history_snapshot
            .iter()
            .flat_map(|turn| turn.items.iter())
            .map(|item| render_history_entry(item, &mut render_context))
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>();
        app.replace_history_cells(cells);
        app.transcript_state.last_copyable_output = history_snapshot
            .iter()
            .rev()
            .flat_map(|turn| turn.items.iter().rev())
            .find_map(|entry| {
                if let TranscriptItem::AgentMessage { text, .. } = entry {
                    (!text.trim().is_empty()).then(|| text.clone())
                } else {
                    None
                }
            });
    }
    app.run_state.history_loaded = app.run_state.history_snapshot.is_some();
}

pub(crate) fn upsert_turn_snapshot(app: &mut TuiApp, turn: ConversationTurn) {
    let history = app.run_state.history_snapshot.get_or_insert_with(Vec::new);
    if let Some(existing) = history.iter_mut().find(|existing| existing.id == turn.id) {
        *existing = turn.clone();
    } else {
        history.push(turn.clone());
    }

    if turn.state == TurnState::Running {
        app.transcript_state.last_copyable_output = turn.items.iter().rev().find_map(|entry| {
            if let TranscriptItem::AgentMessage { text, .. } = entry {
                (!text.trim().is_empty()).then(|| text.clone())
            } else {
                None
            }
        });
    }
    rebuild_transcript_from_history(app);
}

pub(crate) fn apply_turn_dispatch(app: &mut TuiApp, dispatch: crate::state::reducer::TurnDispatch) {
    app.apply_turn_dispatch(dispatch);
}
