use crate::AgentRuntime;
use agent_core::ToolCall;

pub(crate) fn is_tool_approved_for_session(runtime: &AgentRuntime, call: &ToolCall) -> bool {
    runtime
        .session_approvals
        .lock()
        .is_ok_and(|approvals| approvals.contains(&tool_approval_key(call)))
}

pub(crate) fn approve_tool_for_session(runtime: &AgentRuntime, call: &ToolCall) {
    if let Ok(mut approvals) = runtime.session_approvals.lock() {
        approvals.insert(tool_approval_key(call));
    }
}

fn tool_approval_key(call: &ToolCall) -> String {
    let arguments =
        serde_json::to_string(&call.arguments).unwrap_or_else(|_| call.arguments.to_string());
    format!("{}:{arguments}", call.name)
}
