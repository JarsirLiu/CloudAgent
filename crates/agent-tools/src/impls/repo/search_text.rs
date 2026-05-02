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
    #[serde(default)]
    pub regex: Option<bool>,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    #[serde(default)]
    pub file_glob: Option<String>,
    #[serde(default)]
    pub context_lines: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchTextMatch {
    pub path: String,
    pub line: usize,
    pub preview: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
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
                        "context_lines": { "type": "integer", "minimum": 0, "maximum": 5 },
                        "max_results": { "type": "integer", "minimum": 1 },
                        "offset": { "type": "integer", "minimum": 0 }
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
    let offset = args.offset.unwrap_or(0);
    let use_regex = args.regex.unwrap_or(false);
    let case_sensitive = args.case_sensitive.unwrap_or(true);
    let context_lines = args.context_lines.unwrap_or(0).min(5);
    let file_glob = args.file_glob.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let ignored: BTreeSet<&str> = DEFAULT_IGNORED_DIRS.iter().copied().collect();

    let mut files = Vec::new();
    collect_text_files(&base, &base, &ignored, &mut files).await?;

    let mut results = Vec::new();
    let mut skipped = 0usize;
    let mut files_with_matches = BTreeSet::new();

    for file_path in files {
        if results.len() >= max_results {
            break;
        }
        let text = match fs::read_to_string(&file_path).await {
            Ok(text) => text,
            Err(_) => continue,
        };

        let rel = file_path
            .strip_prefix(workspace_root)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(glob) = file_glob && !glob_match(glob, &rel) {
            continue;
        }

        let lines: Vec<&str> = text.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            if !line_matches(query, line, use_regex, case_sensitive) {
                continue;
            }
            if skipped < offset {
                skipped += 1;
                continue;
            }
            files_with_matches.insert(rel.clone());
            let before_start = idx.saturating_sub(context_lines);
            let before = lines[before_start..idx]
                .iter()
                .map(|s| (*s).to_string())
                .collect();
            let after_end = usize::min(idx + 1 + context_lines, lines.len());
            let after = lines[idx + 1..after_end]
                .iter()
                .map(|s| (*s).to_string())
                .collect();
            results.push(SearchTextMatch {
                path: rel.clone(),
                line: idx + 1,
                preview: line.trim().to_string(),
                before,
                after,
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

fn line_matches(query: &str, line: &str, use_regex: bool, case_sensitive: bool) -> bool {
    if use_regex {
        regex_match(query, line, case_sensitive)
    } else if case_sensitive {
        line.contains(query)
    } else {
        line.to_lowercase().contains(&query.to_lowercase())
    }
}

fn glob_match(pattern: &str, value: &str) -> bool {
    wildcard_match(pattern, value)
}

fn regex_match(pattern: &str, text: &str, case_sensitive: bool) -> bool {
    let (p, t) = if case_sensitive {
        (pattern.to_string(), text.to_string())
    } else {
        (pattern.to_lowercase(), text.to_lowercase())
    };
    wildcard_match(&p.replace('.', "?"), &t)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; s.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
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
