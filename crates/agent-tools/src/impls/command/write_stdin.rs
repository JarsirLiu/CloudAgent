use crate::impls::command::descriptor::WriteStdinTool;
use crate::impls::command::output::effective_max_output_tokens;
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
    yield_time_ms: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
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
        let yield_time_ms = args
            .yield_time_ms
            .unwrap_or(ctx.default_shell_timeout_ms)
            .max(1_000);
        let max_output_tokens =
            effective_max_output_tokens(args.max_output_tokens, ctx.max_tool_output_tokens);
        self.sessions
            .write_stdin(
                &args.session_id,
                &args.chars,
                yield_time_ms,
                max_output_tokens,
                ctx,
            )
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
        assert!(properties.contains_key("yield_time_ms"));
        assert!(properties.contains_key("max_output_tokens"));
        assert_eq!(
            parameters.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
    }
}
