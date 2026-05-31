use crate::impls::command::descriptor::WriteStdinTool;
use crate::impls::command::session::ExecSessionStore;
use crate::registry::shared::{LocalTool, LocalToolInvocation, ToolInvocationOutput};
use agent_core::{ToolExecutionContext, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
struct WriteStdinArgs {
    session_id: String,
    chars: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

pub(crate) struct WriteStdinLocalTool {
    sessions: Arc<ExecSessionStore>,
}

impl WriteStdinLocalTool {
    pub(super) fn new(sessions: Arc<ExecSessionStore>) -> Self {
        Self { sessions }
    }
}

#[async_trait]
impl LocalTool for WriteStdinLocalTool {
    fn spec(&self) -> ToolSpec {
        WriteStdinTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: WriteStdinArgs = invocation.payload.parse_arguments()?;
        let timeout_ms = args
            .timeout_ms
            .unwrap_or(ctx.default_shell_timeout_ms)
            .max(1_000);
        self.sessions
            .write_stdin(&args.session_id, &args.chars, timeout_ms, ctx)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::WriteStdinTool;

    #[test]
    fn write_stdin_schema_uses_chars_for_write_and_poll() {
        let parameters = WriteStdinTool::descriptor().spec.parameters;
        let properties = parameters
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("schema properties");

        assert!(properties.contains_key("session_id"));
        assert!(properties.contains_key("chars"));
        assert!(properties.contains_key("timeout_ms"));
        assert_eq!(
            parameters.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
    }
}
