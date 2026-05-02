use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::fs;

pub struct SearchTextTool;
pub struct FindFilesTool;
pub struct ReadFileToolV2;
pub struct ReadFilesTool;

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

#[derive(Debug, Clone, Deserialize)]
pub struct FindFilesArgs {
    pub pattern: String,
    #[serde(default)]
    pub path_scope: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadFilesArgs {
    pub paths: Vec<String>,
    #[serde(default)]
    pub max_lines_per_file: Option<usize>,
}

const DEFAULT_MAX_RESULTS: usize = 100;
const HARD_MAX_RESULTS: usize = 500;
const DEFAULT_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "dist",
    "build",
    "target",
    "target-verify",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    "coverage",
    ".venv",
    "venv",
    "__pycache__",
];

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

fn resolve_workspace_path(workspace_root: &Path, value: &str) -> Result<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let input = Path::new(value);
    if input.is_absolute() {
        bail!("absolute paths are not allowed; use workspace-relative paths");
    }

    let mut candidate = root.clone();
    for component in input.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => candidate.push(segment),
            std::path::Component::ParentDir => {
                if !candidate.pop() || !candidate.starts_with(&root) {
                    bail!("path escapes the workspace root");
                }
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                bail!("unsupported path component")
            }
        }
    }

    if !candidate.starts_with(&root) {
        bail!("path escapes the workspace root");
    }

    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn search_text_skips_ignored_dirs() {
        let base = test_workspace("search_text_skips_ignored_dirs");
        fs::create_dir_all(base.join("src")).await.expect("create src");
        fs::create_dir_all(base.join("node_modules"))
            .await
            .expect("create node_modules");
        fs::write(base.join("src/main.rs"), "let token = 1;\n").await.expect("write src");
        fs::write(base.join("node_modules/bad.js"), "token token token\n")
            .await
            .expect("write ignored");

        let output = run_search_text(
            &base,
            SearchTextArgs {
                query: "token".to_string(),
                path_scope: None,
                max_results: Some(10),
            },
        )
        .await
        .expect("search works");

        assert_eq!(output.match_count, 1);
        assert_eq!(output.file_count, 1);
        assert!(output.results[0].path.contains("src/main.rs"));
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
                        "max_results": { "type": "integer", "minimum": 1 }
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

impl ReadFileToolV2 {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "edit", "verify", "general"],
            ToolSpec {
                name: "read_file".to_string(),
                description: format!(
                    "Read a known file with optional line offsets. Use this for focused inspection after locating candidate files. Maximum characters per request: {max_read_chars}."
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
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
    }
}

impl ReadFilesTool {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "edit", "general"],
            ToolSpec {
                name: "read_files".to_string(),
                description: format!(
                    "Batch-read multiple candidate files in one round to reduce model roundtrips. Maximum characters per file are constrained by the workspace read limit of {max_read_chars}."
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        },
                        "max_lines_per_file": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["paths"]
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
