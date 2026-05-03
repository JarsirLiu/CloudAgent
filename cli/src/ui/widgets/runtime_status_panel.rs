use crate::state::runtime_projection::{RuntimePhase, RuntimeProjection};

pub(crate) fn status_meta_from_projection(projection: &RuntimeProjection) -> Option<String> {
    match (&projection.phase, &projection.active_tool_title) {
        (Some(RuntimePhase::ToolRunning), Some(title)) => Some(format!("tool: {title}")),
        (Some(RuntimePhase::ToolRunning), None) => Some("tool: running".to_string()),
        (Some(RuntimePhase::ModelStreaming), _) => Some("model: streaming".to_string()),
        (Some(RuntimePhase::WaitingApproval), _) => Some("approval: pending".to_string()),
        _ => None,
    }
}

