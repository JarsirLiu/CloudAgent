use super::{ToolCall, ToolExecutor, ToolOutputDelta, ToolResult};
use crate::context::ToolExecutionContext;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ParallelToolInvocation {
    pub index: usize,
    pub call: ToolCall,
    pub tool_item_id: String,
    pub delta_kind: crate::turn::TurnItemDeltaKind,
}

pub struct ParallelToolResult {
    pub index: usize,
    pub call: ToolCall,
    pub tool_item_id: String,
    pub delta_kind: crate::turn::TurnItemDeltaKind,
    pub result: ToolResult,
}

pub async fn execute_tool_call_streaming<T, F>(
    tools: &T,
    cancellation_token: &CancellationToken,
    call: ToolCall,
    ctx: &ToolExecutionContext,
    interrupted_error: &str,
    mut on_output_delta: F,
) -> Result<ToolResult>
where
    T: ToolExecutor + ?Sized,
    F: FnMut(ToolOutputDelta) + Send,
{
    let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
    let streaming_ctx = ctx.clone().with_output_tx(output_tx);
    let execution = tools.execute(call, &streaming_ctx);
    tokio::pin!(execution);

    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(anyhow!(interrupted_error.to_string()));
            }
            Some(delta) = output_rx.recv() => {
                on_output_delta(delta);
            }
            response = &mut execution => {
                while let Ok(delta) = output_rx.try_recv() {
                    on_output_delta(delta);
                }
                return response;
            }
        }
    }
}

pub async fn run_parallel_tool_invocations<T>(
    tools: Arc<T>,
    tool_ctx: &ToolExecutionContext,
    cancellation_token: &CancellationToken,
    invocations: Vec<ParallelToolInvocation>,
    interrupted_error: &str,
) -> Result<Vec<ParallelToolResult>>
where
    T: ToolExecutor + Send + Sync + 'static + ?Sized,
{
    let mut join_set = JoinSet::new();
    for invocation in invocations {
        let tools = Arc::clone(&tools);
        let ctx = tool_ctx.clone();
        let turn_cancellation = cancellation_token.clone();
        let interrupted_error = interrupted_error.to_string();
        join_set.spawn(async move {
            let result = tokio::select! {
                _ = turn_cancellation.cancelled() => {
                    return Err(anyhow!(interrupted_error));
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
