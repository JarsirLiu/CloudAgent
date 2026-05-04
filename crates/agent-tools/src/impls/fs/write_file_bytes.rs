use crate::impls::file_version::version_token_for_bytes;
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_write_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{
    ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec, WriteFileStatus,
};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

pub struct WriteFileBytesTool;

impl WriteFileBytesTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Medium,
            ToolPermissionTier::WorkspaceWrite,
            vec!["edit", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 0,
                preferred_for: vec![
                    "writing raw bytes to one known file",
                    "creating binary or non-text files without text normalization",
                ],
                avoid_for: vec![
                    "normal source-code editing",
                    "small text replacements",
                ],
                follow_up_hint: Some(
                    "use `edit_file` for targeted source edits; reserve this for raw whole-file writes",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "write_file_bytes".to_string(),
                identity: ToolIdentity::built_in("write_file_bytes"),
                description:
                    "Write raw base64-encoded bytes to one known file without applying text decoding or line-ending normalization."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "data_base64": { "type": "string" },
                        "create_parents": { "type": "boolean" }
                    },
                    "required": ["path", "data_base64"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
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
struct WriteFileBytesArgs {
    path: String,
    data_base64: String,
    #[serde(default)]
    create_parents: Option<bool>,
}

pub(crate) struct WriteFileBytesLocalTool;

#[async_trait]
impl LocalTool for WriteFileBytesLocalTool {
    fn spec(&self) -> ToolSpec {
        WriteFileBytesTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: WriteFileBytesArgs = invocation.payload.parse_arguments()?;
        let path = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.path.as_str()),
        )?;
        if args.create_parents.unwrap_or(true)
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent).await?;
        }

        let bytes = STANDARD.decode(args.data_base64.as_bytes())?;
        fs::write(&path, &bytes).await?;
        let version_token = version_token_for_bytes(&bytes);

        Ok(ToolInvocationOutput {
            content: format!("Wrote {} raw bytes to `{}`.", bytes.len(), path.display()),
            structured: Some(agent_protocol::StructuredToolResult::WriteFileBytes {
                path: path.display().to_string(),
                bytes_written: bytes.len(),
                status: WriteFileStatus::Completed,
                version_token: Some(version_token),
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
    async fn write_file_bytes_writes_raw_payload() {
        let base = test_workspace("write_file_bytes_writes_raw_payload");
        let tool = WriteFileBytesLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("write_file_bytes"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": "blob.bin",
                            "data_base64": "AQIDBA=="
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("write bytes");

        let bytes = fs::read(base.join("blob.bin")).await.expect("read");
        assert_eq!(bytes, vec![1_u8, 2, 3, 4]);
        assert!(matches!(
            output.structured,
            Some(agent_protocol::StructuredToolResult::WriteFileBytes {
                bytes_written,
                status: WriteFileStatus::Completed,
                ..
            }) if bytes_written == 4
        ));
    }

    fn tool_context(workspace_root: &std::path::Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::WorkspaceWrite,
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
