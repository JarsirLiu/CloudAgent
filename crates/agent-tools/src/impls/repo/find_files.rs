use super::common::{load_gitignore_patterns, should_ignore_name};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::ToolSpec;
use agent_core::ToolExecutionContext;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::fs;

pub struct FindFilesTool;

#[derive(Debug, Clone, Deserialize)]
pub struct FindFilesArgs {
    pub pattern: String,
    #[serde(default)]
    pub path_scope: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    #[serde(default)]
    pub offset: Option<usize>,
}

impl FindFilesTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "general"],
            ToolSpec {
                name: "find_files".to_string(),
                description: "Find candidate files by name, extension, or glob pattern. Use this before broad directory walking. By default, file discovery should respect ignore rules and skip common dependency and build output directories such as .git, node_modules, dist, and target.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path_scope": { "type": "string" },
                        "max_results": { "type": "integer", "minimum": 1 },
                        "case_sensitive": { "type": "boolean" },
                        "offset": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["pattern"]
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

pub(crate) struct FindFilesLocalTool;

#[async_trait]
impl LocalTool for FindFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        FindFilesTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: FindFilesArgs = serde_json::from_value(arguments)?;
        let pattern = args.pattern.trim().to_string();
        if pattern.is_empty() { bail!("`pattern` must not be empty"); }
        let max_results = args.max_results.unwrap_or(200).clamp(1, 2_000);
        let offset = args.offset.unwrap_or(0);
        let case_sensitive = args.case_sensitive.unwrap_or(false);
        let root = resolve_workspace_path(&ctx.workspace_root, args.path_scope.as_deref())?;
        let gitignore_patterns = load_gitignore_patterns(&ctx.workspace_root).await;
        let mut stack = vec![root];
        let mut matches = Vec::new();
        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await { Ok(entries) => entries, Err(_) => continue };
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = match entry.metadata().await { Ok(metadata) => metadata, Err(_) => continue };
                if metadata.is_dir() {
                    if should_ignore_name(&name, &gitignore_patterns) { continue; }
                    stack.push(path); continue;
                }
                if metadata.is_file() && file_name_matches(&name, &pattern, case_sensitive) {
                    let rel = path.strip_prefix(&ctx.workspace_root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                    matches.push(rel);
                    if matches.len() >= max_results + offset { break; }
                }
            }
            if matches.len() >= max_results + offset { break; }
        }
        matches.sort();
        let matches = matches.into_iter().skip(offset).take(max_results).collect::<Vec<_>>();
        let content = if matches.is_empty() { "No files found".to_string() } else { matches.join("\n") };
        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::FindFiles {
                file_count: matches.len(),
            }),
        })
    }
}

fn file_name_matches(name: &str, pattern: &str, case_sensitive: bool) -> bool {
    let (name, pattern) = if case_sensitive { (name.to_string(), pattern.to_string()) } else { (name.to_lowercase(), pattern.to_lowercase()) };
    wildcard_match(&pattern, &name) || name.contains(&pattern)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; s.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() { if p[i - 1] == '*' { dp[i][0] = dp[i - 1][0]; } }
    for i in 1..=p.len() {
        for j in 1..=s.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == s[j - 1],
            };
        }
    }
    dp[p.len()][s.len()]
}
