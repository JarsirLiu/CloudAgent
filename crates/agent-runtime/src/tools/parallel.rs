use crate::AgentRuntime;
use agent_core::{ToolCall, ToolExecutionContext, ToolExecutor, ToolResult};
use anyhow::Result;
use std::sync::Arc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct ParallelToolInvocation {
    pub(crate) index: usize,
    pub(crate) call: ToolCall,
    pub(crate) tool_item_id: String,
    pub(crate) delta_kind: agent_protocol::TurnItemDeltaKind,
}

pub(crate) struct ParallelToolResult {
    pub(crate) index: usize,
    pub(crate) call: ToolCall,
    pub(crate) tool_item_id: String,
    pub(crate) delta_kind: agent_protocol::TurnItemDeltaKind,
    pub(crate) result: ToolResult,
}

pub(crate) async fn run_parallel_tool_invocations(
    runtime: &AgentRuntime,
    tool_ctx: &ToolExecutionContext,
    cancellation_token: &CancellationToken,
    invocations: Vec<ParallelToolInvocation>,
) -> Result<Vec<ParallelToolResult>> {
    let mut join_set = JoinSet::new();
    for invocation in invocations {
        let tools = Arc::clone(&runtime.tools);
        let ctx = tool_ctx.clone();
        let turn_cancellation = cancellation_token.clone();
        join_set.spawn(async move {
            let result = tokio::select! {
                _ = turn_cancellation.cancelled() => {
                    anyhow::bail!(crate::TURN_INTERRUPTED_ERROR);
                }
                response = tools.execute(invocation.call.clone(), &ctx) => response,
            }?;
            Ok::<_, anyhow::Error>(ParallelToolResult {
                index: invocation.index,
                call: invocation.call,
                tool_item_id: invocation.tool_item_id,
                delta_kind: invocation.delta_kind,
                result,
            })
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        results.push(joined??);
    }
    results.sort_by_key(|result| result.index);
    Ok(results)
}
