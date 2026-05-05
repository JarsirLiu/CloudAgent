use crate::impls::file_read_state::FileReadStateStore;
use crate::impls::file_version::version_token_for_bytes;
use crate::impls::repo::text_read::{
    TextReadFailure, TextReadOptions, TextReadResult, read_text_snippet,
};
use crate::impls::result_format::{finalize, push_fact, push_section};
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec};
use agent_protocol::{ReadFileEntry, ReadFileStatus};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

pub struct ReadFileTool;

#[derive(Debug, Clone, Deserialize)]
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    max_lines: Option<usize>,
}

impl ReadFileTool {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["explore", "edit", "verify", "repo"],
            ToolUsageGuidance {
                selection_priority: 26,
                preferred_for: vec![
                    "confirming repository code facts in one known text file",
                    "inspecting exact source lines before editing",
                    "precise repository follow-up after search results",
                ],
                avoid_for: vec![
                    "broad repository discovery",
                    "batch previews across many files",
                ],
                follow_up_hint: Some(
                    "keep repository inspection on `read_file`; for multiple files, issue multiple `read_file` calls and let the runtime parallelize them",
                ),
                if_truncated_hint: Some(
                    "rerun the same file with the returned `next_start_line` or a narrower `start_line` / `max_lines` slice",
                ),
            },
            ToolSpec {
                name: "read_file".to_string(),
                identity: ToolIdentity::built_in("read_file"),
                description: format!(
                    "Read one known repository text file in a structured code-reading tool call. This is the main repo inspection tool. Use one call per file. When several files need inspection, issue multiple `read_file` calls and let the runtime parallelize them. Output is capped at about {max_read_chars} characters."
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "start_line": { "type": "integer", "minimum": 1 },
                        "max_lines": { "type": "integer", "minimum": 1 }
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
    }
}

pub(crate) struct ReadFileLocalTool {
    pub(crate) max_read_chars: usize,
    pub(crate) read_state: FileReadStateStore,
}

#[async_trait]
impl LocalTool for ReadFileLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFileTool::descriptor(self.max_read_chars).spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ReadFileArgs = invocation.payload.parse_arguments()?;
        let path = resolve_read_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let text = read_text_snippet(
            &path,
            &TextReadOptions::for_single_file(self.max_read_chars, args.start_line, args.max_lines),
        )
        .await?;

        let normalized_path = path.display().to_string();
        let start_line = args.start_line.or(Some(1));

        let (content, entry) = match text {
            Ok(text) => {
                let rendered = text.rendered.clone();
                let version_token = tokio::fs::read(&path)
                    .await
                    .ok()
                    .map(|bytes| version_token_for_bytes(&bytes));
                let is_partial_view =
                    text.truncated || start_line.unwrap_or(1) > 1 || args.max_lines.is_some();
                self.read_state
                    .record_snapshot(
                        &ctx.conversation_id,
                        &path,
                        version_token.clone(),
                        is_partial_view,
                    )
                    .await;
                let entry = ok_entry(normalized_path.clone(), start_line, text, version_token);
                (format!("==> {} <==\n{}", path.display(), rendered), entry)
            }
            Err(failure) => {
                let rendered = failure.render();
                let entry = failure_entry(normalized_path.clone(), start_line, failure);
                (format!("==> {} <==\n{}", path.display(), rendered), entry)
            }
        };

        let summary = match entry.status {
            ReadFileStatus::Ok => {
                if entry.truncated {
                    format!(
                        "Read {} lines from {} and stopped early.",
                        entry.returned_line_count, normalized_path
                    )
                } else {
                    format!(
                        "Read {} lines from {}.",
                        entry.returned_line_count, normalized_path
                    )
                }
            }
            ReadFileStatus::Binary => format!("{normalized_path} is a binary file."),
            ReadFileStatus::TooLarge => format!("{normalized_path} is too large to read directly."),
            ReadFileStatus::UnsupportedEncoding => {
                format!("{normalized_path} uses an unsupported encoding.")
            }
            ReadFileStatus::Failed => format!("Could not read {normalized_path}."),
        };
        let mut rendered = Vec::new();
        push_fact(&mut rendered, "Path", normalized_path.clone());
        push_fact(
            &mut rendered,
            "Status",
            match entry.status {
                ReadFileStatus::Ok => "ok",
                ReadFileStatus::Binary => "binary",
                ReadFileStatus::TooLarge => "too_large",
                ReadFileStatus::UnsupportedEncoding => "unsupported_encoding",
                ReadFileStatus::Failed => "failed",
            },
        );
        if let Some(start_line) = entry.start_line {
            push_fact(&mut rendered, "Start line", start_line.to_string());
        }
        if let Some(end_line) = entry.end_line {
            push_fact(&mut rendered, "End line", end_line.to_string());
        }
        if let Some(total_line_count) = entry.total_line_count {
            push_fact(&mut rendered, "Total lines", total_line_count.to_string());
        }
        push_fact(
            &mut rendered,
            "Returned lines",
            entry.returned_line_count.to_string(),
        );
        push_fact(
            &mut rendered,
            "Returned chars",
            entry.returned_char_count.to_string(),
        );
        if let Some(version_token) = entry.version_token.as_deref() {
            push_fact(&mut rendered, "Version token", version_token.to_string());
        }
        push_section(&mut rendered, "Content", content);
        let next_step = entry.truncated.then(|| {
            let next_start_line = entry.next_start_line.unwrap_or_default();
            format!(
                "rerun `read_file` with `next_start_line: {next_start_line}` or request a narrower `max_lines` slice before making edits"
            )
        });

        Ok(ToolInvocationOutput {
            content: finalize(summary, rendered, next_step.as_deref()),
            structured: Some(agent_protocol::StructuredToolResult::ReadFile {
                path: normalized_path,
                start_line: args.start_line,
                max_lines: args.max_lines,
                total_chars: entry.char_count,
                read: entry,
            }),
        })
    }
}

fn ok_entry(
    path: String,
    start_line: Option<usize>,
    text: TextReadResult,
    version_token: Option<String>,
) -> ReadFileEntry {
    ReadFileEntry {
        path,
        start_line,
        end_line: text.end_line,
        next_start_line: text
            .end_line
            .filter(|_| text.truncated)
            .map(|line| line + 1),
        returned_line_count: text.returned_line_count,
        total_line_count: Some(text.total_line_count),
        returned_char_count: text.rendered_char_count,
        truncated: text.truncated,
        char_count: text.source_char_count,
        status: ReadFileStatus::Ok,
        version_token,
    }
}

fn failure_entry(
    path: String,
    start_line: Option<usize>,
    failure: TextReadFailure,
) -> ReadFileEntry {
    let status = match failure {
        TextReadFailure::Binary => ReadFileStatus::Binary,
        TextReadFailure::TooLarge { .. } => ReadFileStatus::TooLarge,
        TextReadFailure::UnsupportedEncoding(_) => ReadFileStatus::UnsupportedEncoding,
    };
    ReadFileEntry {
        path,
        start_line,
        end_line: None,
        next_start_line: None,
        returned_line_count: 0,
        total_line_count: None,
        returned_char_count: 0,
        truncated: false,
        char_count: 0,
        status,
        version_token: None,
    }
}
