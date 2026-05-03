use crate::app::TuiApp;
use crate::ui::widgets::history_cell::render_history_entry;
use agent_protocol::TranscriptItem;
use crate::state::reducer::ItemDispatch;

pub(crate) fn rebuild_transcript_from_history(app: &mut TuiApp) {
    app.transcript_state = crate::state::TranscriptState::default();
    app.input_pane.clear_views();

    let history_snapshot = app.run_state.history_snapshot.clone().unwrap_or_default();
    if !history_snapshot.is_empty() {
        let cells = history_snapshot
            .iter()
            .flat_map(|turn| turn.items.iter())
            .map(render_history_entry)
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

pub(crate) fn complete_control_item(app: &mut TuiApp, item_id: &str, item: &TranscriptItem) {
    app.handle_control_item_completed(item_id, render_history_entry(item));
}

pub(crate) fn apply_item_dispatch(app: &mut TuiApp, dispatch: ItemDispatch) {
    match dispatch {
        ItemDispatch::AssistantStarted { turn_id, item_id } => {
            app.handle_assistant_item_started(&turn_id, &item_id);
        }
        ItemDispatch::ReasoningStarted { item_id, title } => {
            app.handle_reasoning_item_started(&item_id, &title);
        }
        ItemDispatch::ControlStarted {
            item_id,
            kind,
            title,
        } => {
            app.handle_control_item_started(&item_id, kind, &title);
        }
        ItemDispatch::AssistantDelta { item_id, delta } => {
            app.handle_assistant_item_delta(&item_id, &delta);
        }
        ItemDispatch::ReasoningDelta { item_id, delta } => {
            app.handle_reasoning_item_delta(&item_id, &delta);
        }
        ItemDispatch::ControlDelta { item_id, delta } => {
            app.handle_control_item_delta(&item_id, &delta);
        }
        ItemDispatch::AssistantCompleted { item } => {
            if let TranscriptItem::AgentMessage { id, text } = item {
                app.handle_assistant_item_completed(&id, &text);
            }
        }
        ItemDispatch::ReasoningCompleted { item } => match item {
            TranscriptItem::Reasoning { id, text, .. } => {
                app.handle_reasoning_item_completed(&id, "reasoning", &text);
            }
            TranscriptItem::UserMessage { .. }
            | TranscriptItem::SystemMessage { .. }
            | TranscriptItem::AgentMessage { .. }
            | TranscriptItem::CommandExecution { .. }
            | TranscriptItem::FileChange { .. }
            | TranscriptItem::ToolResult { .. } => {}
        },
        ItemDispatch::ControlCompleted { item } => match item {
            TranscriptItem::CommandExecution { ref id, .. }
            | TranscriptItem::FileChange { ref id, .. }
            | TranscriptItem::ToolResult { ref id, .. } => {
                complete_control_item(app, id, &item);
            }
            TranscriptItem::UserMessage { .. }
            | TranscriptItem::SystemMessage { .. }
            | TranscriptItem::AgentMessage { .. }
            | TranscriptItem::Reasoning { .. } => {}
        },
    }
}

pub(crate) fn apply_turn_dispatch(app: &mut TuiApp, dispatch: crate::state::reducer::TurnDispatch) {
    app.apply_turn_dispatch(dispatch);
}
