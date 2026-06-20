use super::{ToolCall, ToolExecutor, ToolOutputDelta, ToolResult};
use crate::TurnInterruptedError;
use crate::context::ToolExecutionContext;
use anyhow::{Error, Result};
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

pub struct ParallelToolRunOutcome {
    pub results: Vec<ParallelToolResult>,
    pub cancelled: bool,
}

pub async fn execute_tool_call_streaming<T, F>(
    tools: &T,
    cancellation_token: &CancellationToken,
    call: ToolCall,
    ctx: &ToolExecutionContext,
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
                return Err(Error::new(TurnInterruptedError));
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
) -> Result<ParallelToolRunOutcome>
where
    T: ToolExecutor + Send + Sync + 'static + ?Sized,
{
    let mut join_set = JoinSet::new();
    for invocation in invocations {
        let tools = Arc::clone(&tools);
        let ctx = tool_ctx.clone();
        let turn_cancellation = cancellation_token.clone();
        join_set.spawn(async move {
            let result = tokio::select! {
                _ = turn_cancellation.cancelled() => {
                    return Err(Error::new(TurnInterruptedError));
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
    let mut cancelled = false;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok(result)) => results.push(result),
            Ok(Err(_err)) if cancellation_token.is_cancelled() => {
                cancelled = true;
                join_set.abort_all();
                break;
            }
            Ok(Err(err)) => {
                join_set.abort_all();
                return Err(err);
            }
            Err(join_err) if join_err.is_cancelled() && cancellation_token.is_cancelled() => {
                cancelled = true;
                break;
            }
            Err(join_err) => {
                join_set.abort_all();
                return Err(join_err.into());
            }
        }
    }

    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok(result)) => results.push(result),
            Ok(Err(_err)) if cancellation_token.is_cancelled() => {
                cancelled = true;
            }
            Ok(Err(err)) => return Err(err),
            Err(join_err) if join_err.is_cancelled() && cancellation_token.is_cancelled() => {
                cancelled = true;
            }
            Err(join_err) => return Err(join_err.into()),
        }
    }

    results.sort_by_key(|result| result.index);
    Ok(ParallelToolRunOutcome { results, cancelled })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionProfile;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;
    use std::path::PathBuf;
    use tokio::time::{Duration, sleep};

    struct TestExecutor;

    #[async_trait]
    impl ToolExecutor for TestExecutor {
        fn specs(&self) -> Vec<crate::ToolSpec> {
            Vec::new()
        }

        async fn execute(&self, call: ToolCall, _ctx: &ToolExecutionContext) -> Result<ToolResult> {
            match call.name.as_str() {
                "fast" => sleep(Duration::from_millis(10)).await,
                "slow" => sleep(Duration::from_millis(200)).await,
                _ => {}
            }
            Ok(ToolResult {
                tool_call_id: call.id.clone(),
                name: call.name,
                content: "ok".to_string(),
                is_error: false,
                structured: None,
            })
        }
    }

    #[tokio::test]
    async fn parallel_invocations_keep_completed_results_when_cancelled() {
        let executor = Arc::new(TestExecutor);
        let cancellation_token = CancellationToken::new();
        let cancel_clone = cancellation_token.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });
        let ctx = ToolExecutionContext {
            conversation_id: "conv".to_string(),
            workspace_root: PathBuf::from("D:\\learn\\gifti\\cloudagent"),
            conversation_store_dir: PathBuf::from("D:\\learn\\gifti\\cloudagent\\.cloudagent"),
            permission_profile: PermissionProfile::WorkspaceWrite,
            default_shell_timeout_ms: 30_000,
            max_tool_output_tokens: ToolExecutionContext::default_max_tool_output_tokens(),
            cancellation_token: cancellation_token.clone(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        };

        let outcome = run_parallel_tool_invocations(
            executor,
            &ctx,
            &cancellation_token,
            vec![
                ParallelToolInvocation {
                    index: 0,
                    call: ToolCall {
                        id: "fast-1".to_string(),
                        name: "fast".to_string(),
                        identity: crate::ToolIdentity::built_in("fast"),
                        arguments: json!({}),
                    },
                    tool_item_id: "tool:fast-1".to_string(),
                    delta_kind: crate::TurnItemDeltaKind::ToolOutput,
                },
                ParallelToolInvocation {
                    index: 1,
                    call: ToolCall {
                        id: "slow-1".to_string(),
                        name: "slow".to_string(),
                        identity: crate::ToolIdentity::built_in("slow"),
                        arguments: json!({}),
                    },
                    tool_item_id: "tool:slow-1".to_string(),
                    delta_kind: crate::TurnItemDeltaKind::ToolOutput,
                },
            ],
        )
        .await
        .expect("parallel execution should surface cancellation as an outcome");

        assert!(outcome.cancelled);
        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].call.id, "fast-1");
    }
}
