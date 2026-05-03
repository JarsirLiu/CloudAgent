use crate::AgentRuntime;
use crate::observability::{AuditEventEntry, append_audit_event_safe};
use agent_core::ToolCall;
use agent_protocol::ServerRequestDecision;
use serde_json::json;

pub(crate) struct RuntimeAudit<'a> {
    runtime: &'a AgentRuntime,
}

impl<'a> RuntimeAudit<'a> {
    pub(crate) fn new(runtime: &'a AgentRuntime) -> Self {
        Self { runtime }
    }

    pub(crate) fn turn_started(&self, session_id: &str, user_input: &str) {
        self.append(
            session_id,
            None,
            "turn.started",
            "info",
            json!({ "input_preview": user_input.chars().take(300).collect::<String>() }),
        );
    }

    pub(crate) fn turn_completed(
        &self,
        session_id: &str,
        turn_id: &str,
        state: &str,
        events_count: usize,
        model: Option<&str>,
    ) {
        self.append(
            session_id,
            Some(turn_id),
            "turn.completed",
            "info",
            json!({ "state": state, "events_count": events_count, "model": model }),
        );
    }

    pub(crate) fn turn_cancelled(&self, session_id: &str, turn_id: &str, reason: &str) {
        self.append(
            session_id,
            Some(turn_id),
            "turn.cancelled",
            "warn",
            json!({ "reason": reason }),
        );
    }

    pub(crate) fn turn_failed(&self, session_id: &str, turn_id: &str, error: &str) {
        self.append(
            session_id,
            Some(turn_id),
            "turn.failed",
            "error",
            json!({ "error": error.chars().take(1200).collect::<String>() }),
        );
    }

    pub(crate) fn tool_started(&self, session_id: &str, turn_id: &str, call: &ToolCall, arguments_preview: String) {
        self.append(
            session_id,
            Some(turn_id),
            "tool.started",
            "info",
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "arguments_preview": arguments_preview
            }),
        );
    }

    pub(crate) fn tool_completed(
        &self,
        session_id: &str,
        turn_id: &str,
        call: &ToolCall,
        is_error: bool,
        content_preview: String,
    ) {
        self.append(
            session_id,
            Some(turn_id),
            if is_error { "tool.failed" } else { "tool.completed" },
            if is_error { "error" } else { "info" },
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "is_error": is_error,
                "content_preview": content_preview
            }),
        );
    }

    pub(crate) fn approval_requested(
        &self,
        session_id: &str,
        turn_id: &str,
        call: &ToolCall,
        reason: String,
        arguments_preview: String,
    ) {
        self.append(
            session_id,
            Some(turn_id),
            "approval.requested",
            "info",
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "reason": reason,
                "arguments_preview": arguments_preview
            }),
        );
    }

    pub(crate) fn approval_decided(
        &self,
        session_id: &str,
        turn_id: &str,
        call: &ToolCall,
        decision: &ServerRequestDecision,
    ) {
        self.append(
            session_id,
            Some(turn_id),
            "approval.decided",
            "info",
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "decision": format!("{:?}", decision.decision),
                "reason": decision.reason
            }),
        );
    }

    pub(crate) fn model_request_started(
        &self,
        session_id: &str,
        turn_id: &str,
        message_count: usize,
        tool_count: usize,
    ) {
        self.append(
            session_id,
            Some(turn_id),
            "model.requested",
            "info",
            json!({
                "message_count": message_count,
                "tool_count": tool_count
            }),
        );
    }

    pub(crate) fn model_response_received(
        &self,
        session_id: &str,
        turn_id: &str,
        model_name: Option<&str>,
        has_content: bool,
        tool_call_count: usize,
    ) {
        self.append(
            session_id,
            Some(turn_id),
            "model.responded",
            "info",
            json!({
                "model_name": model_name,
                "has_content": has_content,
                "tool_call_count": tool_call_count
            }),
        );
    }

    fn append(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        event_type: &str,
        severity: &str,
        payload: serde_json::Value,
    ) {
        let payload_json = serde_json::to_string(&payload)
            .unwrap_or_else(|_| "{\"error\":\"payload_serialize_failed\"}".to_string());
        let entry = AuditEventEntry {
            session_id,
            turn_id,
            event_type,
            severity,
            payload_json,
        };
        append_audit_event_safe(&self.runtime.context.workspace_root, &entry);
    }
}
