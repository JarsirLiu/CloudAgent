use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_IGNORED_DIRS: &[&str] = &[
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

pub(crate) async fn load_gitignore_patterns(workspace_root: &Path) -> Vec<String> {
    let gitignore_path = workspace_root.join(".gitignore");
    match tokio::fs::read_to_string(gitignore_path).await {
        Ok(content) => content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.trim_end_matches('/').to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn should_ignore_name(name: &str, gitignore_patterns: &[String]) -> bool {
    if DEFAULT_IGNORED_DIRS.contains(&name) {
        return true;
    }
    if name.starts_with('.') && name != ".cargo" {
        return true;
    }
    gitignore_patterns.iter().any(|p| p == name)
}

pub(crate) fn resolve_workspace_path(workspace_root: &Path, value: &str) -> Result<PathBuf> {
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
