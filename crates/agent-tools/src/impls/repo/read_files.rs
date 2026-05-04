use crate::impls::repo::text_read::{TextReadOptions, read_text_snippet};
use crate::registry::shared::{LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk};
use agent_core::{ToolExecutionContext, ToolIdentity, ToolSpec};
use agent_protocol::{ReadFileEntry, ReadFileStatus};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

pub struct ReadFilesTool;

#[derive(Debug, Clone, Deserialize)]
struct ReadFilesArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    max_lines: Option<usize>,
}

impl ReadFilesTool {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            true,
            vec!["explore", "edit", "verify", "repo", "fs"],
            ToolSpec {
                name: "read_files".to_string(),
                identity: ToolIdentity::built_in("read_files"),
                description: format!(
                    "Read one or more known files in one structured tool call. Use this after search or file discovery when you need to confirm code facts or compare related files. Total characters per request are capped at about {max_read_chars}."
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        },
                        "start_line": { "type": "integer", "minimum": 1 },
                        "max_lines": { "type": "integer", "minimum": 1 }
                    },
                    "anyOf": [
                        { "required": ["path"] },
                        { "required": ["paths"] }
                    ]
                }),
                mutating: false,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
    }
}

pub(crate) struct ReadFilesLocalTool {
    pub(crate) max_read_chars: usize,
}

#[async_trait]
impl LocalTool for ReadFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFilesTool::descriptor(self.max_read_chars).spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ReadFilesArgs = invocation.payload.parse_arguments()?;
        let mut requested_paths = Vec::new();
        if let Some(path) = args.path {
            requested_paths.push(path);
        }
        requested_paths.extend(args.paths);
        if requested_paths.is_empty() {
            bail!("either `path` or `paths` must be provided");
        }
        if requested_paths.len() > 64 {
            bail!("at most 64 files can be read in one call");
        }

        let per_file_chars = (self.max_read_chars / requested_paths.len().max(1)).max(512);
        let mut rendered = Vec::new();
        let mut normalized_paths = Vec::new();
        let mut reads = Vec::new();
        let mut failed_count = 0usize;
        let mut truncated_count = 0usize;
        let mut total_chars = 0usize;

        for raw_path in &requested_paths {
            let path = resolve_read_path(&ctx.workspace_root, Some(raw_path.as_str()))?;
            normalized_paths.push(path.display().to_string());
            let read_result = read_text_snippet(
                &path,
                &TextReadOptions::for_single_file(per_file_chars, args.start_line, args.max_lines),
            )
            .await?;
            let content = match read_result {
                Ok(text) => {
                    if text.truncated {
                        truncated_count += 1;
                    }
                    total_chars += text.source_char_count;
                    reads.push(ReadFileEntry {
                        path: path.display().to_string(),
                        start_line: args.start_line.or(Some(1)),
                        end_line: text.end_line,
                        truncated: text.truncated,
                        char_count: text.source_char_count,
                        status: ReadFileStatus::Ok,
                    });
                    text.rendered
                }
                Err(failure) => {
                    failed_count += 1;
                    let status = match &failure {
                        crate::impls::repo::text_read::TextReadFailure::Binary => {
                            ReadFileStatus::Binary
                        }
                        crate::impls::repo::text_read::TextReadFailure::TooLarge { .. } => {
                            ReadFileStatus::TooLarge
                        }
                        crate::impls::repo::text_read::TextReadFailure::UnsupportedEncoding(_) => {
                            ReadFileStatus::UnsupportedEncoding
                        }
                    };
                    reads.push(ReadFileEntry {
                        path: path.display().to_string(),
                        start_line: args.start_line.or(Some(1)),
                        end_line: None,
                        truncated: false,
                        char_count: 0,
                        status,
                    });
                    failure.render()
                }
            };
            rendered.push(format!("==> {} <==\n{}", path.display(), content));
        }

        Ok(ToolInvocationOutput {
            content: rendered.join("\n\n"),
            structured: Some(agent_protocol::StructuredToolResult::ReadFiles {
                paths: normalized_paths,
                start_line: args.start_line,
                max_lines: args.max_lines,
                file_count: requested_paths.len(),
                failed_count,
                truncated_count,
                total_chars,
                reads,
            }),
        })
    }
}
