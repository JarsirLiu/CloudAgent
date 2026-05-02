use super::common::{DEFAULT_IGNORED_DIRS, resolve_workspace_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::fs;

pub struct SearchTextTool;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchTextArgs {
    pub query: String,
    #[serde(default)]
    pub path_scope: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
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
                        "file_glob": { "type": "string" },
                        "regex": { "type": "boolean" },
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

pub async fn run_search_text(workspace_root: &Path, args: SearchTextArgs) -> Result<SearchTextOutput> {
    let query = args.query.trim();
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
    let ignored: BTreeSet<&str> = DEFAULT_IGNORED_DIRS.iter().copied().collect();

    let mut files = Vec::new();
    collect_text_files(&base, &base, &ignored, &mut files).await?;

    let mut results = Vec::new();
    let mut files_with_matches = BTreeSet::new();

    for file_path in files {
        if results.len() >= max_results {
            break;
        }
        let text = match fs::read_to_string(&file_path).await {
            Ok(text) => text,
            Err(_) => continue,
        };

        for (idx, line) in text.lines().enumerate() {
            if !line.contains(query) {
                continue;
            }
            let rel = file_path
                .strip_prefix(workspace_root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .replace('\\', "/");
            files_with_matches.insert(rel.clone());
            results.push(SearchTextMatch {
                path: rel,
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
}

async fn collect_text_files(
    workspace_root: &Path,
    current: &Path,
    ignored: &BTreeSet<&str>,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut stack = vec![current.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir)
            .await
            .with_context(|| format!("failed to read directory {}", dir.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if ignored.contains(name.as_str()) {
                    continue;
                }
                if name.starts_with('.') && name != ".cargo" {
                    continue;
                }
                stack.push(path);
                continue;
            }

            if metadata.is_file() && is_probably_text_file(&path) && path.starts_with(workspace_root)
            {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn is_probably_text_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return true;
    };
    !matches!(
        ext.to_ascii_lowercase().as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "pdf"
            | "zip"
            | "gz"
            | "xz"
            | "tar"
            | "7z"
            | "exe"
            | "dll"
            | "so"
            | "dylib"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
            | "mp4"
            | "mp3"
            | "wav"
    )
}
