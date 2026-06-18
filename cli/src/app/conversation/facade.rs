use crate::app::TuiApp;
use agent_core::ConversationTurn;
use agent_core::turn::TurnState;

pub(crate) fn rebuild_transcript_from_history(app: &mut TuiApp) {
    app.bottom_pane.clear_views();
    app.bottom_pane.clear_composer();

    let history_snapshot = app.run_state.history_snapshot.clone().unwrap_or_default();
    app.transcript_owner
        .rebuild_from_history_snapshot(&history_snapshot, app.run_state.expand_tool_details);
}

pub(crate) fn replace_transcript_from_history(app: &mut TuiApp) {
    rebuild_transcript_from_history(app);
    app.transcript_scroll.reset();
}

pub(crate) fn upsert_turn_snapshot(app: &mut TuiApp, turn: ConversationTurn) {
    let history = app.run_state.history_snapshot.get_or_insert_with(Vec::new);
    if let Some(existing) = history.iter_mut().find(|existing| existing.id == turn.id) {
        *existing = turn.clone();
    } else {
        history.push(turn.clone());
    }

    if should_rebuild_transcript_after_upsert(app, &turn) {
        rebuild_transcript_from_history(app);
    }
}

pub(crate) fn apply_turn_dispatch(app: &mut TuiApp, dispatch: crate::state::reducer::TurnDispatch) {
    app.apply_turn_dispatch(dispatch);
}

fn should_rebuild_transcript_after_upsert(app: &TuiApp, turn: &ConversationTurn) -> bool {
    if app.transcript_owner.active_turn_id().is_none() && app.transcript_owner.live_is_empty() {
        return true;
    }

    matches!(turn.state, TurnState::Completed | TurnState::Failed)
        && app.transcript_owner.active_turn_id().is_none()
}
