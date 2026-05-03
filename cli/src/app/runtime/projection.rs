use crate::app::TuiApp;
use crate::state::reducer::{ItemDispatch, ServerAction};

pub(crate) fn apply_runtime_projection_update(app: &mut TuiApp, action: &ServerAction) {
    match action {
        ServerAction::SetMode(mode) => app.runtime_projection.on_mode_changed(*mode),
        ServerAction::ClearLastToolName => app.runtime_projection.on_tool_finished(),
        ServerAction::ItemDispatch(ItemDispatch::ControlStarted { title, .. }) => {
            app.runtime_projection.on_tool_started(title.clone());
        }
        ServerAction::TurnDispatch(_) => app.runtime_projection.on_turn_finished(),
        _ => {}
    }
}

