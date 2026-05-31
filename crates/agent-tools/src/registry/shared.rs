use agent_core::{
    PermissionProfile, StructuredToolResult, ToolExecutionContext, ToolIdentity, ToolSpec,
    WriteFileStatus,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) enum LocalToolSource {
    BuiltIn,
    Mcp,
}

#[derive(Clone, Debug)]
pub(crate) enum LocalToolPayload {
    Function {
        arguments: Value,
    },
    Mcp {
        server: String,
        tool: String,
        arguments: Value,
    },
}

impl LocalToolPayload {
    pub(crate) fn parse_arguments<T: DeserializeOwned>(&self) -> Result<T> {
        match self {
            LocalToolPayload::Function { arguments } => {
                Ok(serde_json::from_value(arguments.clone())?)
            }
            LocalToolPayload::Mcp { .. } => {
                bail!("MCP payloads do not support local argument parsing")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalToolInvocation {
    pub(crate) identity: ToolIdentity,
    pub(crate) source: LocalToolSource,
    pub(crate) payload: LocalToolPayload,
}

#[async_trait]
pub(crate) trait LocalTool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput>;
}

#[derive(Clone, Debug)]
pub(crate) struct ToolInvocationOutput {
    pub(crate) content: String,
    pub(crate) structured: Option<StructuredToolResult>,
}

pub(crate) fn register<T>(
    tools: &mut std::collections::BTreeMap<String, Arc<dyn LocalTool>>,
    tool: T,
) where
    T: LocalTool + 'static,
{
    let spec = tool.spec();
    tools.insert(spec.identity.wire_name.clone(), Arc::new(tool));
}

pub(crate) fn decode_utf8_chunk(buffer: &mut Vec<u8>, flush: bool) -> String {
    if buffer.is_empty() {
        return String::new();
    }

    match std::str::from_utf8(buffer) {
        Ok(valid) => {
            let text = valid.to_string();
            buffer.clear();
            text
        }
        Err(err) if !flush && err.error_len().is_none() => {
            let valid_up_to = err.valid_up_to();
            if valid_up_to == 0 {
                return String::new();
            }
            let text = String::from_utf8_lossy(&buffer[..valid_up_to]).to_string();
            buffer.drain(..valid_up_to);
            text
        }
        Err(err) => {
            let valid_up_to = err.valid_up_to();
            let text = String::from_utf8_lossy(&buffer[..valid_up_to]).to_string();
            let invalid_end = match err.error_len() {
                Some(len) => valid_up_to.saturating_add(len),
                None => buffer.len(),
            };
            let remainder = if invalid_end < buffer.len() {
                buffer[invalid_end..].to_vec()
            } else {
                Vec::new()
            };
            buffer.clear();
            buffer.extend_from_slice(&remainder);
            if flush && !buffer.is_empty() {
                let mut out = text;
                out.push_str(&String::from_utf8_lossy(buffer));
                buffer.clear();
                out
            } else {
                text
            }
        }
    }
}

pub(crate) fn resolve_workspace_path(
    workspace_root: &Path,
    value: Option<&str>,
) -> Result<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let Some(value) = value else {
        return Ok(root);
    };
    let input = Path::new(value.trim());
    if input.as_os_str().is_empty() {
        return Ok(root);
    }
    let absolute_base = input
        .is_absolute()
        .then(|| absolute_path_base(input))
        .transpose()?;
    let mut candidate = if input.is_absolute() {
        absolute_base
            .clone()
            .expect("absolute paths must have a base")
    } else {
        root.clone()
    };
    let mut components = input.components().peekable();
    if input.is_absolute() {
        if matches!(components.peek(), Some(Component::Prefix(_))) {
            components.next();
        }
        if matches!(components.peek(), Some(Component::RootDir)) {
            components.next();
        }
    }
    for component in components {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => candidate.push(segment),
            Component::ParentDir => {
                if !candidate.pop()
                    || (input.is_absolute() && candidate == absolute_base.clone().unwrap())
                {
                    bail!("path escapes the workspace root");
                }
                if !input.is_absolute() && !candidate.starts_with(&root) {
                    bail!("path escapes the workspace root");
                }
            }
            Component::Prefix(_) | Component::RootDir => bail!("unsupported path component"),
        }
    }
    let candidate = normalize_existing_ancestor_path(&candidate);
    if !candidate.starts_with(&root) {
        bail!("path escapes the workspace root");
    }
    Ok(candidate)
}

pub(crate) fn resolve_read_path(
    workspace_root: &Path,
    permission_profile: &PermissionProfile,
    value: Option<&str>,
) -> Result<PathBuf> {
    match permission_profile {
        PermissionProfile::ReadOnly => resolve_workspace_path(workspace_root, value),
        PermissionProfile::WorkspaceWrite | PermissionProfile::FullAccess => {
            resolve_external_read_path(workspace_root, value)
        }
    }
}

pub(crate) fn resolve_external_read_path(
    workspace_root: &Path,
    value: Option<&str>,
) -> Result<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let Some(value) = value else {
        return Ok(root);
    };
    let input = Path::new(value.trim());
    if input.as_os_str().is_empty() {
        return Ok(root);
    }
    let candidate = if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    };
    Ok(candidate)
}

fn resolve_full_access_path(workspace_root: &Path, value: Option<&str>) -> Result<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let Some(value) = value else {
        return Ok(root);
    };
    let input = Path::new(value.trim());
    if input.as_os_str().is_empty() {
        return Ok(root);
    }
    if input.is_absolute() {
        return Ok(input.to_path_buf());
    }

    let mut candidate = root.clone();
    for component in input.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => candidate.push(segment),
            Component::ParentDir => {
                if !candidate.pop() {
                    bail!("path escapes the filesystem root");
                }
            }
            Component::Prefix(_) | Component::RootDir => bail!("unsupported path component"),
        }
    }
    Ok(candidate)
}

fn absolute_path_base(input: &Path) -> Result<PathBuf> {
    let mut components = input.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => {
            let mut base = PathBuf::from(format!(
                "{}{}",
                prefix.as_os_str().to_string_lossy(),
                std::path::MAIN_SEPARATOR
            ));
            if matches!(components.next(), Some(Component::RootDir)) {
                return Ok(base);
            }
            base.pop();
            Ok(base)
        }
        Some(Component::RootDir) => Ok(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
        _ => bail!("unsupported path component"),
    }
}

fn normalize_existing_ancestor_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    let mut suffix = Vec::<OsString>::new();
    let mut current = path;
    loop {
        let Some(name) = current.file_name() else {
            return path.to_path_buf();
        };
        suffix.push(name.to_os_string());
        let Some(parent) = current.parent() else {
            return path.to_path_buf();
        };
        if let Ok(canonical_parent) = parent.canonicalize() {
            let mut normalized = canonical_parent;
            for segment in suffix.iter().rev() {
                normalized.push(segment);
            }
            return normalized;
        }
        current = parent;
    }
}

pub(crate) fn resolve_write_path(
    workspace_root: &Path,
    permission_profile: &PermissionProfile,
    value: Option<&str>,
) -> Result<PathBuf> {
    match permission_profile {
        PermissionProfile::FullAccess => resolve_full_access_path(workspace_root, value),
        PermissionProfile::ReadOnly | PermissionProfile::WorkspaceWrite => {
            resolve_workspace_path(workspace_root, value)
        }
    }
}

pub(crate) fn structured_failure_result(
    invocation: &LocalToolInvocation,
) -> Option<StructuredToolResult> {
    match (&invocation.source, invocation.identity.wire_name.as_str()) {
        (LocalToolSource::BuiltIn, "apply_patch" | "edit_file") => {
            Some(StructuredToolResult::EditFile {
                changed_paths: Vec::new(),
                files_changed: 0,
                status: WriteFileStatus::Failed,
                version_token: None,
            })
        }
        (LocalToolSource::BuiltIn, "copy_path") => Some(StructuredToolResult::CopyPath {
            source_path: String::new(),
            destination_path: String::new(),
            recursive: false,
            status: WriteFileStatus::Failed,
        }),
        (LocalToolSource::BuiltIn, "remove_path") => Some(StructuredToolResult::RemovePath {
            path: String::new(),
            recursive: false,
            force: false,
            removed: false,
            status: WriteFileStatus::Failed,
        }),
        (LocalToolSource::BuiltIn, _) | (LocalToolSource::Mcp, _) => {
            Some(StructuredToolResult::ToolError {
                tool_name: invocation.identity.wire_name.clone(),
                message: "tool execution failed".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_existing_ancestor_path, resolve_read_path, resolve_workspace_path,
        resolve_write_path,
    };
    use agent_core::PermissionProfile;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("agent-tools-{label}-{suffix}"))
    }

    fn create_workspace() -> (PathBuf, PathBuf, PathBuf) {
        let base = unique_test_dir("shared-paths");
        let workspace = base.join("workspace");
        let nested = workspace.join("nested");
        let outside = base.join("outside");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&outside).unwrap();
        (base, workspace, outside)
    }

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn resolve_workspace_path_accepts_absolute_path_inside_workspace() {
        let (base, workspace, _) = create_workspace();
        let target = workspace.join("nested").join("file.txt");

        let resolved = resolve_workspace_path(&workspace, Some(&path_string(&target))).unwrap();

        assert_eq!(
            normalize_existing_ancestor_path(&resolved),
            normalize_existing_ancestor_path(&target)
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_workspace_path_rejects_absolute_path_outside_workspace() {
        let (base, workspace, outside) = create_workspace();
        let target = outside.join("file.txt");

        let err = resolve_workspace_path(&workspace, Some(&path_string(&target))).unwrap_err();

        assert!(err.to_string().contains("path escapes the workspace root"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_write_path_respects_permission_profile_boundaries() {
        let (base, workspace, outside) = create_workspace();
        let inside = workspace.join("nested").join("file.txt");
        let outside_target = outside.join("file.txt");

        let readonly = resolve_write_path(
            &workspace,
            &PermissionProfile::ReadOnly,
            Some(&path_string(&inside)),
        )
        .unwrap();
        let workspace_write = resolve_write_path(
            &workspace,
            &PermissionProfile::WorkspaceWrite,
            Some(&path_string(&inside)),
        )
        .unwrap();
        let full_access = resolve_write_path(
            &workspace,
            &PermissionProfile::FullAccess,
            Some(&path_string(&outside_target)),
        )
        .unwrap();

        assert_eq!(
            normalize_existing_ancestor_path(&readonly),
            normalize_existing_ancestor_path(&inside)
        );
        assert_eq!(
            normalize_existing_ancestor_path(&workspace_write),
            normalize_existing_ancestor_path(&inside)
        );
        assert_eq!(
            normalize_existing_ancestor_path(&full_access),
            normalize_existing_ancestor_path(&outside_target)
        );
        assert!(
            resolve_write_path(
                &workspace,
                &PermissionProfile::WorkspaceWrite,
                Some(&path_string(&outside_target)),
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_read_path_matches_permission_profile_boundaries() {
        let (base, workspace, outside) = create_workspace();
        let inside = workspace.join("nested").join("file.txt");
        let outside_target = outside.join("file.txt");

        let readonly_inside = resolve_read_path(
            &workspace,
            &PermissionProfile::ReadOnly,
            Some(&path_string(&inside)),
        )
        .unwrap();
        let workspace_write_outside = resolve_read_path(
            &workspace,
            &PermissionProfile::WorkspaceWrite,
            Some(&path_string(&outside_target)),
        )
        .unwrap();
        let full_access_outside = resolve_read_path(
            &workspace,
            &PermissionProfile::FullAccess,
            Some(&path_string(&outside_target)),
        )
        .unwrap();

        assert_eq!(
            normalize_existing_ancestor_path(&readonly_inside),
            normalize_existing_ancestor_path(&inside)
        );
        assert_eq!(
            normalize_existing_ancestor_path(&workspace_write_outside),
            normalize_existing_ancestor_path(&outside_target)
        );
        assert_eq!(
            normalize_existing_ancestor_path(&full_access_outside),
            normalize_existing_ancestor_path(&outside_target)
        );
        assert!(
            resolve_read_path(
                &workspace,
                &PermissionProfile::ReadOnly,
                Some(&path_string(&outside_target)),
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(base);
    }
}
