use crate::app::TuiApp;
use crate::state::live_state::LivePhase;
use crate::state::reducer::{ItemDispatch, TurnDispatch};

pub(crate) fn apply_item_dispatch_live_state(app: &mut TuiApp, dispatch: &ItemDispatch) {
    app.run_state.live_state.phase = match dispatch {
        ItemDispatch::AssistantStarted { .. } | ItemDispatch::AssistantDelta { .. } => {
            LivePhase::AssistantResponding
        }
        ItemDispatch::ReasoningStarted { .. } | ItemDispatch::ReasoningDelta { .. } => {
            LivePhase::Reasoning
        }
        ItemDispatch::ControlStarted { title, .. } => LivePhase::ToolRunning {
            title: title.clone(),
        },
        ItemDispatch::ControlDelta { .. } => app.run_state.live_state.phase.clone(),
        ItemDispatch::AssistantCompleted { .. }
        | ItemDispatch::ReasoningCompleted { .. }
        | ItemDispatch::ControlCompleted { .. } => app.run_state.live_state.phase.clone(),
    };
}

pub(crate) fn apply_turn_dispatch_live_state(app: &mut TuiApp, _dispatch: &TurnDispatch) {
    app.run_state.live_state.phase = LivePhase::Idle;
}
