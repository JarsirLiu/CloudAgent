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
