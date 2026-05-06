use crate::app::TuiApp;
use agent_protocol::ConversationTurn;

pub(crate) fn rebuild_transcript_from_history(app: &mut TuiApp) {
    app.transcript_state = crate::state::TranscriptState::default();
    app.input_pane.clear_views();

    let history_snapshot = app.run_state.history_snapshot.clone().unwrap_or_default();
    app.transcript_owner
        .rebuild_from_history_snapshot(&history_snapshot, app.run_state.expand_tool_details);
    app.run_state.history_loaded = app.run_state.history_snapshot.is_some();
}

pub(crate) fn upsert_turn_snapshot(app: &mut TuiApp, turn: ConversationTurn) {
    let history = app.run_state.history_snapshot.get_or_insert_with(Vec::new);
    if let Some(existing) = history.iter_mut().find(|existing| existing.id == turn.id) {
        *existing = turn.clone();
    } else {
        history.push(turn.clone());
    }

    if app.transcript_owner.active_turn_id().is_none() && app.live_cells().is_empty() {
        rebuild_transcript_from_history(app);
    }
}

pub(crate) fn apply_turn_dispatch(app: &mut TuiApp, dispatch: crate::state::reducer::TurnDispatch) {
    app.apply_turn_dispatch(dispatch);
}
