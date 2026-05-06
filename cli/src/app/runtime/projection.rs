use crate::app::TuiApp;
use crate::state::reducer::ServerAction;

pub(crate) fn apply_runtime_projection_update(app: &mut TuiApp, action: &ServerAction) {
    match action {
        ServerAction::SetMode(mode) => app.runtime_projection.on_mode_changed(*mode),
        ServerAction::ClearCurrentTurnUsage => app.runtime_projection.on_turn_started(),
        ServerAction::ClearLastToolName => app.runtime_projection.on_tool_finished(),
        ServerAction::SetRetryStatus {
            stage,
            attempt,
            next_delay_ms,
        } => app
            .runtime_projection
            .on_model_retrying(stage.clone(), *attempt, *next_delay_ms),
        ServerAction::TurnDispatch(_) => app.runtime_projection.on_turn_finished(),
        _ => {}
    }
}
