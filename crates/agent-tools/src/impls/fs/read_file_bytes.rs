use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_workspace_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

const DEFAULT_MAX_BYTES: usize = 256 * 1024;

pub struct ReadFileBytesTool;

impl ReadFileBytesTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["explore", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 0,
                preferred_for: vec![
                    "reading raw bytes from one known file",
                    "handling files that should not be interpreted as text",
                ],
                avoid_for: vec![
                    "normal source-code reading",
                    "symbol or repo discovery",
                ],
                follow_up_hint: Some(
                    "use `read_file` for text/code and continue with `next_offset` when the byte result is truncated",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "read_file_bytes".to_string(),
                identity: ToolIdentity::built_in("read_file_bytes"),
                description:
                    "Read raw bytes from one known file and return a base64 payload with offset-based continuation."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "offset": { "type": "integer", "minimum": 0 },
                        "max_bytes": { "type": "integer", "minimum": 1, "maximum": 1048576 }
                    },
                    "required": ["path"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::ParallelSafe,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Debug, Deserialize)]
struct ReadFileBytesArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    max_bytes: Option<usize>,
}

pub(crate) struct ReadFileBytesLocalTool;

#[async_trait]
impl LocalTool for ReadFileBytesLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFileBytesTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ReadFileBytesArgs = invocation.payload.parse_arguments()?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let offset = args.offset.unwrap_or(0);
        let max_bytes = args
            .max_bytes
            .unwrap_or(DEFAULT_MAX_BYTES)
            .clamp(1, 1024 * 1024);

        let bytes = fs::read(&path).await?;
        let total_bytes = bytes.len();
        let start = offset.min(total_bytes);
        let end = start.saturating_add(max_bytes).min(total_bytes);
        let chunk = &bytes[start..end];
        let truncated = end < total_bytes;
        let next_offset = truncated.then_some(end);
        let data_base64 = STANDARD.encode(chunk);

        let content = serde_json::to_string_pretty(&json!({
            "path": path.display().to_string(),
            "offset": start,
            "bytes_read": chunk.len(),
            "total_bytes": total_bytes,
            "truncated": truncated,
            "next_offset": next_offset,
            "data_base64": data_base64,
        }))?;

        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::ReadFileBytes {
                path: path.display().to_string(),
                offset: start,
                bytes_read: chunk.len(),
                total_bytes,
                truncated,
                next_offset,
                data_base64,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{LocalToolPayload, LocalToolSource};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn read_file_bytes_returns_base64_chunk() {
        let base = test_workspace("read_file_bytes_returns_base64_chunk");
        fs::write(base.join("blob.bin"), [1_u8, 2, 3, 4, 5])
            .await
            .expect("write");

        let tool = ReadFileBytesLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("read_file_bytes"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": "blob.bin",
                            "offset": 1,
                            "max_bytes": 2
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("read bytes");

        assert!(matches!(
            output.structured,
            Some(agent_protocol::StructuredToolResult::ReadFileBytes {
                offset,
                bytes_read,
                total_bytes,
                truncated,
                next_offset,
                ..
            }) if offset == 1
                && bytes_read == 2
                && total_bytes == 5
                && truncated
                && next_offset == Some(3)
        ));
    }

    fn tool_context(workspace_root: &std::path::Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::ReadOnly,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn test_workspace(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        path.push(format!("cloudagent_{name}_{stamp}"));
        std::fs::create_dir_all(&path).expect("create temp workspace");
        path
    }
}

