use crate::impls::repo::{
    FindFilesArgs, FindFilesTool, ReadFileToolV2, ReadFilesArgs, ReadFilesTool, SearchTextArgs,
    SearchTextTool, run_search_text,
};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::{ToolExecutionContext, ToolSpec};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::fs;

pub(crate) struct SearchTextLocalTool;
pub(crate) struct FindFilesLocalTool;
pub(crate) struct ReadFilesLocalTool {
    pub(crate) max_read_chars: usize,
}
pub(crate) struct ReadFileTool {
    pub(crate) max_read_chars: usize,
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    max_chars: Option<usize>,
}

#[async_trait]
impl LocalTool for SearchTextLocalTool {
    fn spec(&self) -> ToolSpec {
        SearchTextTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: SearchTextArgs = serde_json::from_value(arguments)?;
        let output = run_search_text(&ctx.workspace_root, args).await?;
        let lines = output.results.iter().map(|m| format!("{}:{}: {}", m.path, m.line, m.preview)).collect::<Vec<_>>().join("\n");
        let content = if lines.is_empty() { "No matches found".to_string() } else { format!("Found {} matches in {} files.\n{}", output.match_count, output.file_count, lines) };
        Ok(ToolInvocationOutput { content, summary: format!("found {} matches across {} files", output.match_count, output.file_count), structured: None })
    }
}

#[async_trait]
impl LocalTool for FindFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        FindFilesTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: FindFilesArgs = serde_json::from_value(arguments)?;
        let pattern = args.pattern.trim().to_lowercase();
        if pattern.is_empty() { bail!("`pattern` must not be empty"); }
        let max_results = args.max_results.unwrap_or(200).clamp(1, 2_000);
        let root = resolve_workspace_path(&ctx.workspace_root, args.path_scope.as_deref())?;
        let mut stack = vec![root];
        let mut matches = Vec::new();
        let ignored = [".git",".hg",".svn","node_modules","dist","build","target","target-verify",".next",".nuxt",".turbo",".cache","coverage",".venv","venv","__pycache__"];
        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await { Ok(entries) => entries, Err(_) => continue };
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = match entry.metadata().await { Ok(metadata) => metadata, Err(_) => continue };
                if metadata.is_dir() {
                    if ignored.contains(&name.as_str()) || (name.starts_with('.') && name != ".cargo") { continue; }
                    stack.push(path); continue;
                }
                if metadata.is_file() && name.to_lowercase().contains(&pattern) {
                    let rel = path.strip_prefix(&ctx.workspace_root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                    matches.push(rel);
                    if matches.len() >= max_results { break; }
                }
            }
            if matches.len() >= max_results { break; }
        }
        matches.sort();
        let content = if matches.is_empty() { "No files found".to_string() } else { matches.join("\n") };
        Ok(ToolInvocationOutput { summary: format!("found {} files", matches.len()), content, structured: None })
    }
}

#[async_trait]
impl LocalTool for ReadFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFilesTool::descriptor(self.max_read_chars).spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadFilesArgs = serde_json::from_value(arguments)?;
        if args.paths.is_empty() { bail!("`paths` must not be empty"); }
        let max_lines = args.max_lines_per_file.unwrap_or(300).clamp(1, 2_000);
        let mut blocks = Vec::new();
        for path in args.paths {
            let resolved = resolve_workspace_path(&ctx.workspace_root, Some(path.as_str()))?;
            let text = fs::read_to_string(&resolved).await?;
            let mut lines = Vec::new();
            for (idx, line) in text.lines().enumerate() {
                if idx >= max_lines { lines.push("[truncated]".to_string()); break; }
                lines.push(line.to_string());
            }
            let rel = resolved.strip_prefix(&ctx.workspace_root).unwrap_or(&resolved).to_string_lossy().replace('\\', "/");
            blocks.push(format!("== {} ==\n{}", rel, lines.join("\n")));
        }
        Ok(ToolInvocationOutput { summary: format!("read {} files", blocks.len()), content: blocks.join("\n\n"), structured: None })
    }
}

#[async_trait]
impl LocalTool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ReadFileToolV2::descriptor(self.max_read_chars).spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let text = fs::read_to_string(&path).await?;
        let max_chars = args.max_chars.unwrap_or(self.max_read_chars).max(128);
        let content = if text.chars().count() > max_chars {
            format!("{}\n\n[truncated]", text.chars().take(max_chars).collect::<String>())
        } else {
            text
        };
        let char_count = content.chars().count();
        let truncated = content.ends_with("\n\n[truncated]");
        Ok(ToolInvocationOutput {
            summary: format!("read {}", path.display()),
            content,
            structured: Some(agent_protocol::StructuredToolResult::ReadFile {
                path: path.display().to_string(),
                truncated,
                char_count,
            }),
        })
    }
}
