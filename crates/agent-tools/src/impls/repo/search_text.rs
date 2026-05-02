use super::common::{
    DEFAULT_IGNORED_DIRS, collect_repo_entries, is_probably_text_file, read_text_lossy,
    resolve_workspace_path,
};
use crate::registry::shared::{LocalTool, ToolInvocationOutput};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::{ToolExecutionContext, ToolSpec};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub struct SearchTextTool;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchTextArgs {
    pub query: String,
    #[serde(default)]
    pub path_scope: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchTextMatch {
    pub path: String,
    pub line: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchTextOutput {
    pub match_count: usize,
    pub file_count: usize,
    pub truncated: bool,
    pub results: Vec<SearchTextMatch>,
}

const DEFAULT_MAX_RESULTS: usize = 100;
const HARD_MAX_RESULTS: usize = 500;

impl SearchTextTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "general"],
            ToolSpec {
                name: "search_text".to_string(),
                description: "Search workspace text by keyword or regex. Prefer this over directory-by-directory traversal when locating implementations. By default, repository search should respect ignore rules and skip common dependency and build output directories such as .git, node_modules, dist, and target.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path_scope": { "type": "string" },
                        "case_sensitive": { "type": "boolean" },
                        "max_results": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["query"]
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

pub(crate) struct SearchTextLocalTool;

#[async_trait]
impl LocalTool for SearchTextLocalTool {
    fn spec(&self) -> ToolSpec {
        SearchTextTool::descriptor().spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: SearchTextArgs = serde_json::from_value(arguments)?;
        let output = run_search_text(&ctx.workspace_root, args).await?;
        let lines = output
            .results
            .iter()
            .map(|m| format!("{}:{}: {}", m.path, m.line, m.preview))
            .collect::<Vec<_>>()
            .join("\n\n");
        let content = if lines.is_empty() {
            "No matches found".to_string()
        } else {
            format!(
                "Found {} matches in {} files.\n{}",
                output.match_count, output.file_count, lines
            )
        };
        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::SearchText {
                match_count: output.match_count,
                file_count: output.file_count,
                truncated: output.truncated,
            }),
        })
    }
}

pub async fn run_search_text(
    workspace_root: &Path,
    args: SearchTextArgs,
) -> Result<SearchTextOutput> {
    let query = args.query.trim().to_string();
    if query.is_empty() {
        bail!("`query` must not be empty");
    }

    let base = match args.path_scope.as_deref() {
        Some(scope) => resolve_workspace_path(workspace_root, scope)?,
        None => workspace_root.to_path_buf(),
    };

    let max_results = args
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, HARD_MAX_RESULTS);
    let case_sensitive = args.case_sensitive.unwrap_or(false);
    let workspace_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if let Some(output) =
        run_search_text_with_ripgrep(&workspace_root, &base, &query, case_sensitive, max_results)
            .await?
    {
        return Ok(output);
    }

    tokio::task::spawn_blocking(move || {
        let entries = collect_repo_entries(&workspace_root, &base)?;
        let mut results = Vec::new();
        let mut files_with_matches = BTreeSet::new();
        let normalized_query = if case_sensitive {
            query.to_string()
        } else {
            query.to_ascii_lowercase()
        };

        for entry in entries {
            if results.len() >= max_results || !is_probably_text_file(&entry.absolute_path) {
                continue;
            }
            let text = match read_text_lossy(&entry.absolute_path) {
                Ok(text) => text,
                Err(_) => continue,
            };
            for (idx, line) in text.lines().enumerate() {
                if !line_matches(&normalized_query, line, case_sensitive) {
                    continue;
                }
                files_with_matches.insert(entry.relative_path.clone());
                results.push(SearchTextMatch {
                    path: entry.relative_path.clone(),
                    line: idx + 1,
                    preview: line.trim().to_string(),
                });
                if results.len() >= max_results {
                    break;
                }
            }
        }

        Ok(SearchTextOutput {
            match_count: results.len(),
            file_count: files_with_matches.len(),
            truncated: results.len() >= max_results,
            results,
        })
    })
    .await?
}

fn line_matches(query: &str, line: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        line.contains(query)
    } else {
        line.to_ascii_lowercase().contains(query)
    }
}

async fn run_search_text_with_ripgrep(
    workspace_root: &Path,
    base: &Path,
    query: &str,
    case_sensitive: bool,
    max_results: usize,
) -> Result<Option<SearchTextOutput>> {
    let mut command = Command::new("rg");
    command
        .arg("--json")
        .arg("--line-number")
        .arg("--color")
        .arg("never")
        .arg("--hidden")
        .arg("--follow")
        .arg("--no-messages");
    if !case_sensitive {
        command.arg("--ignore-case");
    }
    for ignored_dir in DEFAULT_IGNORED_DIRS {
        command.arg("--glob").arg(format!("!**/{ignored_dir}/**"));
    }
    command.arg(query).arg(base);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Ok(None),
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill().await;
            return Ok(None);
        }
    };

    let mut reader = BufReader::new(stdout).lines();
    let mut results = Vec::new();
    let mut files_with_matches = BTreeSet::new();

    while let Some(line) = reader.next_line().await? {
        if let Some(search_match) = parse_ripgrep_match_line(&line, workspace_root)? {
            files_with_matches.insert(search_match.path.clone());
            results.push(search_match);
            if results.len() >= max_results {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Ok(Some(SearchTextOutput {
                    match_count: results.len(),
                    file_count: files_with_matches.len(),
                    truncated: true,
                    results,
                }));
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() && status.code() != Some(1) {
        return Ok(None);
    }
    Ok(Some(SearchTextOutput {
        match_count: results.len(),
        file_count: files_with_matches.len(),
        truncated: false,
        results,
    }))
}

fn parse_ripgrep_match_line(line: &str, workspace_root: &Path) -> Result<Option<SearchTextMatch>> {
    let payload: Value = match serde_json::from_str(line) {
        Ok(payload) => payload,
        Err(_) => return Ok(None),
    };
    if payload.get("type").and_then(Value::as_str) != Some("match") {
        return Ok(None);
    }

    let absolute_path = payload
        .get("data")
        .and_then(|data| data.get("path"))
        .and_then(|path| path.get("text"))
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let Some(absolute_path) = absolute_path else {
        return Ok(None);
    };
    let relative_path = absolute_path
        .strip_prefix(workspace_root)
        .unwrap_or(&absolute_path)
        .to_string_lossy()
        .replace('\\', "/");
    let line_number = payload
        .get("data")
        .and_then(|data| data.get("line_number"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let preview = payload
        .get("data")
        .and_then(|data| data.get("lines"))
        .and_then(|lines| lines.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    Ok(Some(SearchTextMatch {
        path: relative_path,
        line: line_number,
        preview,
    }))
}
