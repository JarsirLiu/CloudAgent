use agent_core::{ToolExecutionContext, ToolOutputDelta, ToolOutputStream, ToolSpec};
use agent_protocol::{StructuredToolResult, WriteFileStatus};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncReadExt;

#[async_trait]
pub(crate) trait LocalTool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput>;
}

#[derive(Clone, Debug)]
pub(crate) struct ToolInvocationOutput {
    pub(crate) content: String,
    pub(crate) summary: String,
    pub(crate) structured: Option<StructuredToolResult>,
}

pub(crate) fn register<T>(
    tools: &mut std::collections::BTreeMap<String, Arc<dyn LocalTool>>,
    tool: T,
) where
    T: LocalTool + 'static,
{
    tools.insert(tool.spec().name.clone(), Arc::new(tool));
}

pub(crate) async fn read_streaming_pipe<R>(
    mut reader: R,
    stream: ToolOutputStream,
    output_tx: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputDelta>>,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut collected = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let chunk = buffer[..read].to_vec();
        collected.extend_from_slice(&chunk);
        if let Some(output_tx) = &output_tx {
            let _ = output_tx.send(ToolOutputDelta {
                stream: stream.clone(),
                chunk: String::from_utf8_lossy(&chunk).to_string(),
            });
        }
    }
    Ok(collected)
}

pub(crate) fn resolve_workspace_path(workspace_root: &Path, value: Option<&str>) -> Result<PathBuf> {
    let root = workspace_root.canonicalize().unwrap_or_else(|_| workspace_root.to_path_buf());
    let Some(value) = value else {
        return Ok(root);
    };
    let input = Path::new(value);
    if input.is_absolute() {
        bail!("absolute paths are not allowed; use workspace-relative paths");
    }
    let mut candidate = root.clone();
    for component in input.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => candidate.push(segment),
            Component::ParentDir => {
                if !candidate.pop() || !candidate.starts_with(&root) {
                    bail!("path escapes the workspace root");
                }
            }
            Component::Prefix(_) | Component::RootDir => bail!("unsupported path component"),
        }
    }
    if !candidate.starts_with(&root) {
        bail!("path escapes the workspace root");
    }
    Ok(candidate)
}

pub(crate) fn structured_failure_result(
    tool_name: &str,
    arguments: &Value,
) -> Option<StructuredToolResult> {
    match tool_name {
        "write_file" => {
            let path = arguments
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            Some(StructuredToolResult::WriteFile {
                path,
                bytes_written: 0,
                status: WriteFileStatus::Failed,
            })
        }
        _ => None,
    }
}
