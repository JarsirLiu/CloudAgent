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
