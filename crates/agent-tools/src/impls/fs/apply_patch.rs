use crate::impls::text_codec::{LineEnding, decode_text_file, encode_text_file};
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_workspace_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{
    StructuredToolResult, ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec,
    TurnItemDeltaKind, TurnItemKind, WriteFileStatus,
};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::PathBuf;
use tokio::fs;

pub struct ApplyPatchTool;

const APPLY_PATCH_TOOL_DESCRIPTION: &str = r#"Apply a focused patch to workspace files.
Use the CloudAgent patch format below and pass the entire patch as the `patch` string.

*** Begin Patch
[ one or more file sections ]
*** End Patch

Each file section must start with one of:
- `*** Add File: <path>`
- `*** Delete File: <path>`
- `*** Update File: <path>`

Within an update section, include one or more hunks introduced by `@@`.
Use `@@ <context>` to narrow the search to a nearby class, function, or section.
Use `*** Move to: <path>` immediately after an update header to rename a file.
Use `*** End of File` at the end of a hunk to require an end-of-file match.
Within a hunk, each line must start with:
- space for unchanged context
- `-` for removed lines
- `+` for added lines

Example:
*** Begin Patch
*** Update File: src/app.rs
@@
-old
+new
*** End Patch

Rules:
- Use relative workspace paths only
- Include enough unchanged context for each hunk to apply safely
- Do not pass a standard git diff with `diff --git`, `---`, or `+++`
"#;

impl ApplyPatchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Medium,
            ToolPermissionTier::WorkspaceWrite,
            vec!["edit", "fs"],
            ToolUsageGuidance {
                selection_priority: 20,
                preferred_for: vec![
                    "focused workspace file edits",
                    "minimal code changes instead of whole-file rewrites",
                ],
                avoid_for: vec![
                    "raw git unified diffs",
                    "directory discovery",
                    "build or runtime verification",
                ],
                follow_up_hint: Some(
                    "after editing, inspect the diff and run the narrowest relevant verification",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "apply_patch".to_string(),
                identity: ToolIdentity::built_in("apply_patch"),
                description: APPLY_PATCH_TOOL_DESCRIPTION.to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "patch": { "type": "string" }
                    },
                    "required": ["patch"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: true,
                item_kind: TurnItemKind::FileChange,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: Some("Applying patches can modify workspace files.".to_string()),
            },
        )
    }
}

#[derive(Deserialize)]
struct ApplyPatchArgs {
    patch: String,
}

pub(crate) struct ApplyPatchLocalTool;

#[async_trait]
impl LocalTool for ApplyPatchLocalTool {
    fn spec(&self) -> ToolSpec {
        ApplyPatchTool::descriptor().spec
    }
    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ApplyPatchArgs = invocation.payload.parse_arguments()?;
        let file_patches = parse_unified_patch(&args.patch)?;
        if file_patches.is_empty() {
            anyhow::bail!(invalid_patch_message(&args.patch));
        }
        let planned_changes = plan_file_changes(&ctx.workspace_root, file_patches).await?;
        let mut changed_files = BTreeSet::new();
        for planned_change in planned_changes {
            match planned_change {
                PlannedFileChange::Write {
                    display_path,
                    path,
                    bytes,
                    remove_source,
                } => {
                    let Some(parent) = path.parent() else {
                        anyhow::bail!("cannot determine parent directory for {}", path.display());
                    };
                    fs::create_dir_all(parent).await?;
                    fs::write(&path, bytes).await?;
                    if let Some(source_path) = remove_source {
                        fs::remove_file(&source_path).await?;
                    }
                    changed_files.insert(display_path);
                }
                PlannedFileChange::Delete { display_path, path } => {
                    fs::remove_file(&path).await?;
                    changed_files.insert(display_path);
                }
            }
        }
        let files_changed = changed_files.len();
        let changed_paths = changed_files.into_iter().collect::<Vec<_>>();

        Ok(ToolInvocationOutput {
            content: format!(
                "Applied patch. files_changed={files_changed}. If the user request is not fully closed, continue with the next tool call before answering."
            ),
            structured: Some(StructuredToolResult::EditFile {
                changed_paths,
                files_changed,
                status: WriteFileStatus::Completed,
                version_token: None,
            }),
        })
    }
}

#[derive(Debug)]
enum PlannedFileChange {
    Write {
        display_path: String,
        path: PathBuf,
        bytes: Vec<u8>,
        remove_source: Option<PathBuf>,
    },
    Delete {
        display_path: String,
        path: PathBuf,
    },
}

async fn plan_file_changes(
    workspace_root: &std::path::Path,
    file_patches: Vec<FilePatch>,
) -> anyhow::Result<Vec<PlannedFileChange>> {
    let mut planned_changes = Vec::new();
    for file_patch in file_patches {
        match (file_patch.old_path.as_str(), file_patch.new_path.as_str()) {
            ("/dev/null", new_path) => {
                let path = resolve_workspace_path(workspace_root, Some(new_path))?;
                ensure_not_directory(&path).await?;
                let next = render_hunks_as_new_file(&file_patch.hunks)?;
                planned_changes.push(PlannedFileChange::Write {
                    display_path: new_path.to_string(),
                    path,
                    bytes: next.into_bytes(),
                    remove_source: None,
                });
            }
            (old_path, "/dev/null") => {
                let path = resolve_workspace_path(workspace_root, Some(old_path))?;
                if !path.exists() {
                    anyhow::bail!("refusing to delete missing file {}", path.display());
                }
                ensure_not_directory(&path).await?;
                planned_changes.push(PlannedFileChange::Delete {
                    display_path: old_path.to_string(),
                    path,
                });
            }
            (_, new_path) => {
                let old_path = resolve_workspace_path(workspace_root, Some(&file_patch.old_path))?;
                if !old_path.exists() {
                    anyhow::bail!("refusing to update missing file {}", old_path.display());
                }
                ensure_not_directory(&old_path).await?;
                let current_bytes = fs::read(&old_path).await?;
                let decoded = decode_text_file(&current_bytes).map_err(|err| {
                    anyhow::anyhow!("failed to apply patch for {}: {}", new_path, err.render())
                })?;
                let next = apply_hunks(&decoded.text, decoded.line_ending, &file_patch.hunks)
                    .map_err(|err| {
                        anyhow::anyhow!("failed to apply patch for {}: {err}", new_path)
                    })?;
                let destination_path = resolve_workspace_path(
                    workspace_root,
                    Some(file_patch.move_path.as_deref().unwrap_or(new_path)),
                )?;
                ensure_not_directory(&destination_path).await?;
                if next != decoded.text || destination_path != old_path {
                    let display_path = file_patch
                        .move_path
                        .as_deref()
                        .unwrap_or(new_path)
                        .to_string();
                    let remove_source = (destination_path != old_path).then_some(old_path);
                    planned_changes.push(PlannedFileChange::Write {
                        display_path,
                        path: destination_path,
                        bytes: encode_text_file(&decoded, &next),
                        remove_source,
                    });
                }
            }
        }
    }
    Ok(planned_changes)
}

async fn ensure_not_directory(path: &std::path::Path) -> anyhow::Result<()> {
    if fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_dir())
    {
        anyhow::bail!("path is a directory: {}", path.display());
    }
    Ok(())
}

#[derive(Debug)]
struct FilePatch {
    old_path: String,
    new_path: String,
    move_path: Option<String>,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    change_context: Option<String>,
    is_end_of_file: bool,
    lines: Vec<String>,
}

fn parse_unified_patch(patch: &str) -> anyhow::Result<Vec<FilePatch>> {
    let mut file_patches = Vec::new();
    let mut current_old_path: Option<String> = None;
    let mut current_new_path: Option<String> = None;
    let mut current_move_path: Option<String> = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk: Option<Hunk> = None;
    let mut in_patch_block = false;

    let flush_current = |file_patches: &mut Vec<FilePatch>,
                         current_old_path: &mut Option<String>,
                         current_new_path: &mut Option<String>,
                         current_move_path: &mut Option<String>,
                         hunks: &mut Vec<Hunk>,
                         current_hunk: &mut Option<Hunk>| {
        if let Some(h) = current_hunk.take() {
            hunks.push(h);
        }
        if let (Some(old_path), Some(new_path)) = (current_old_path.take(), current_new_path.take())
        {
            file_patches.push(FilePatch {
                old_path,
                new_path,
                move_path: current_move_path.take(),
                hunks: std::mem::take(hunks),
            });
        }
        *current_move_path = None;
    };

    let patch = patch.trim();
    if !patch.starts_with("*** Begin Patch") {
        anyhow::bail!(invalid_patch_message(patch));
    }
    if !patch.ends_with("*** End Patch") {
        anyhow::bail!("patch must end with `*** End Patch`");
    }

    for line in patch.lines() {
        if line == "*** Begin Patch" {
            in_patch_block = true;
            continue;
        }
        if line == "*** End Patch" {
            flush_current(
                &mut file_patches,
                &mut current_old_path,
                &mut current_new_path,
                &mut current_move_path,
                &mut hunks,
                &mut current_hunk,
            );
            in_patch_block = false;
            continue;
        }
        if !in_patch_block && !line.starts_with("diff --git ") {
            continue;
        }
        if line.starts_with("diff --git ") {
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            flush_current(
                &mut file_patches,
                &mut current_old_path,
                &mut current_new_path,
                &mut current_move_path,
                &mut hunks,
                &mut current_hunk,
            );
            current_old_path = Some("/dev/null".to_string());
            current_new_path = Some(path.trim().to_string());
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            flush_current(
                &mut file_patches,
                &mut current_old_path,
                &mut current_new_path,
                &mut current_move_path,
                &mut hunks,
                &mut current_hunk,
            );
            current_old_path = Some(path.trim().to_string());
            current_new_path = Some("/dev/null".to_string());
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            flush_current(
                &mut file_patches,
                &mut current_old_path,
                &mut current_new_path,
                &mut current_move_path,
                &mut hunks,
                &mut current_hunk,
            );
            let path = path.trim().to_string();
            current_old_path = Some(path.clone());
            current_new_path = Some(path);
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Move to: ") {
            let Some(old_path) = current_old_path.as_deref() else {
                anyhow::bail!("move target found before update file");
            };
            if old_path == "/dev/null" || current_new_path.as_deref() == Some("/dev/null") {
                anyhow::bail!("move target is only valid in update file sections");
            }
            if current_move_path.is_some() {
                anyhow::bail!("update file section contains multiple move targets");
            }
            if current_hunk.is_some() || !hunks.is_empty() {
                anyhow::bail!("move target must appear before update hunks");
            }
            let Some(new_path) = current_new_path.as_mut() else {
                anyhow::bail!("move target found before update file");
            };
            let path = path.trim().to_string();
            current_move_path = Some(path.clone());
            *new_path = path;
            continue;
        }
        if line.starts_with("@@") {
            if current_new_path.is_none() {
                anyhow::bail!("hunk found before target file");
            }
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            current_hunk = Some(Hunk {
                change_context: parse_change_context(line),
                is_end_of_file: false,
                lines: Vec::new(),
            });
            continue;
        }
        if current_hunk.is_none()
            && current_new_path.is_some()
            && (line.starts_with(' ') || line.starts_with('+') || line.starts_with('-'))
        {
            current_hunk = Some(Hunk {
                change_context: None,
                is_end_of_file: false,
                lines: Vec::new(),
            });
        }
        if let Some(hunk) = current_hunk.as_mut() {
            if line.starts_with('\\') {
                continue;
            }
            if line.trim() == "*** End of File" {
                hunk.is_end_of_file = true;
                continue;
            }
            hunk.lines.push(line.to_string());
            continue;
        }
        if current_new_path.is_some() && !line.trim().is_empty() {
            anyhow::bail!("unexpected line in patch section: {line}");
        }
    }
    flush_current(
        &mut file_patches,
        &mut current_old_path,
        &mut current_new_path,
        &mut current_move_path,
        &mut hunks,
        &mut current_hunk,
    );
    validate_file_patches(&file_patches)?;
    Ok(file_patches)
}

fn validate_file_patches(file_patches: &[FilePatch]) -> anyhow::Result<()> {
    let mut affected_paths = BTreeSet::new();
    for file_patch in file_patches {
        let mut section_paths = BTreeSet::new();
        for path in [
            file_patch.old_path.as_str(),
            file_patch.new_path.as_str(),
            file_patch.move_path.as_deref().unwrap_or(""),
        ] {
            if path.is_empty() || path == "/dev/null" {
                continue;
            }
            section_paths.insert(path.to_string());
        }
        for path in section_paths {
            if !affected_paths.insert(path.clone()) {
                anyhow::bail!("patch contains duplicate file section for `{path}`");
            }
        }
        match (file_patch.old_path.as_str(), file_patch.new_path.as_str()) {
            ("/dev/null", _) => {}
            (_, "/dev/null") if file_patch.hunks.is_empty() => {}
            (_, "/dev/null") => {
                anyhow::bail!(
                    "delete file section for `{}` must not contain hunks",
                    file_patch.old_path
                );
            }
            (_, _) if file_patch.hunks.is_empty() => {
                anyhow::bail!(
                    "update file section for `{}` does not contain any hunks",
                    file_patch.old_path
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn invalid_patch_message(patch: &str) -> String {
    let trimmed = patch.trim();
    let likely_unified = trimmed.contains("@@")
        || trimmed
            .lines()
            .any(|line| line.starts_with("--- ") || line.starts_with("+++ "));
    if likely_unified {
        "patch did not contain any editable file hunks; `apply_patch` only accepts the CloudAgent patch format (`*** Begin Patch` / `*** Update File:` / `@@`), not raw git unified diffs".to_string()
    } else {
        "patch did not contain any editable file hunks; use the CloudAgent patch format (`*** Begin Patch` / `*** Update File:` / `@@`)".to_string()
    }
}

fn render_hunks_as_new_file(hunks: &[Hunk]) -> anyhow::Result<String> {
    let mut lines = Vec::new();
    for hunk in hunks {
        for line in &hunk.lines {
            if let Some(rest) = line.strip_prefix('+') {
                lines.push(rest.to_string());
            } else if line.starts_with(' ') || line.starts_with('-') {
                continue;
            } else {
                anyhow::bail!("unsupported hunk line for new file: {line}");
            }
        }
    }
    let mut content = lines.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    Ok(content)
}

fn apply_hunks(original: &str, line_ending: LineEnding, hunks: &[Hunk]) -> anyhow::Result<String> {
    let mut lines = original
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    let replacements = compute_replacements(&lines, hunks)?;
    let mut next_lines = apply_replacements(lines, &replacements);
    if !next_lines.last().is_some_and(String::is_empty) {
        next_lines.push(String::new());
    }
    Ok(next_lines.join(line_ending.as_str()))
}

#[cfg(test)]
fn apply_single_hunk(
    content: &str,
    line_ending: LineEnding,
    hunk: &Hunk,
) -> anyhow::Result<String> {
    apply_hunks(content, line_ending, std::slice::from_ref(hunk))
}

fn hunk_blocks(hunk: &Hunk) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let mut old_block: Vec<String> = Vec::new();
    let mut new_block: Vec<String> = Vec::new();
    for line in &hunk.lines {
        if line.is_empty() {
            old_block.push(String::new());
            new_block.push(String::new());
        } else if let Some(rest) = line.strip_prefix(' ') {
            old_block.push(rest.to_string());
            new_block.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('-') {
            old_block.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('+') {
            new_block.push(rest.to_string());
        } else {
            anyhow::bail!("unsupported hunk line: {line}");
        }
    }
    Ok((old_block, new_block))
}

fn compute_replacements(
    original_lines: &[String],
    hunks: &[Hunk],
) -> anyhow::Result<Vec<(usize, usize, Vec<String>)>> {
    let mut replacements = Vec::new();
    let mut line_index = 0usize;

    for hunk in hunks {
        if let Some(context) = &hunk.change_context {
            let Some(context_index) = seek_sequence(
                original_lines,
                std::slice::from_ref(context),
                line_index,
                false,
            ) else {
                anyhow::bail!("Failed to find context '{context}'");
            };
            line_index = context_index + 1;
        }

        let (old_block, new_block) = hunk_blocks(hunk)?;
        if old_block.is_empty() {
            replacements.push((original_lines.len(), 0, new_block));
            continue;
        }

        let mut pattern = old_block.as_slice();
        let mut new_slice = new_block.as_slice();
        let mut found = seek_sequence(original_lines, pattern, line_index, hunk.is_end_of_file);

        if found.is_none() && pattern.last().is_some_and(String::is_empty) {
            pattern = &pattern[..pattern.len() - 1];
            if new_slice.last().is_some_and(String::is_empty) {
                new_slice = &new_slice[..new_slice.len() - 1];
            }
            found = seek_sequence(original_lines, pattern, line_index, hunk.is_end_of_file);
        }

        let Some(start_index) = found else {
            anyhow::bail!("Failed to find expected lines:\n{}", old_block.join("\n"));
        };
        replacements.push((start_index, pattern.len(), new_slice.to_vec()));
        line_index = start_index + pattern.len();
    }

    replacements.sort_by_key(|(start_index, _, _)| *start_index);
    Ok(replacements)
}

fn apply_replacements(
    mut lines: Vec<String>,
    replacements: &[(usize, usize, Vec<String>)],
) -> Vec<String> {
    for (start_index, old_len, new_segment) in replacements.iter().rev() {
        for _ in 0..*old_len {
            if *start_index < lines.len() {
                lines.remove(*start_index);
            }
        }
        for (offset, new_line) in new_segment.iter().enumerate() {
            lines.insert(*start_index + offset, new_line.clone());
        }
    }
    lines
}

fn parse_change_context(header: &str) -> Option<String> {
    let header = header.trim();
    let context = header.strip_prefix("@@")?.trim();
    if context.is_empty() || context.starts_with('-') {
        None
    } else {
        Some(context.to_string())
    }
}

fn seek_sequence(
    lines: &[String],
    pattern: &[String],
    start: usize,
    is_end_of_file: bool,
) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start.min(lines.len()));
    }
    if pattern.len() > lines.len() {
        return None;
    }

    let search_start = if is_end_of_file {
        lines.len().saturating_sub(pattern.len())
    } else {
        start.min(lines.len().saturating_sub(pattern.len()))
    };

    for matcher in [
        lines_match_exact as fn(&[String], &[String], usize) -> bool,
        lines_match_trim_end,
        lines_match_trim,
    ] {
        for index in search_start..=lines.len().saturating_sub(pattern.len()) {
            if matcher(lines, pattern, index) {
                return Some(index);
            }
        }
    }

    None
}

fn lines_match_exact(lines: &[String], pattern: &[String], index: usize) -> bool {
    lines
        .get(index..index.saturating_add(pattern.len()))
        .is_some_and(|slice| slice == pattern)
}

fn lines_match_trim_end(lines: &[String], pattern: &[String], index: usize) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(offset, expected)| lines[index + offset].trim_end() == expected.trim_end())
}

fn lines_match_trim(lines: &[String], pattern: &[String], index: usize) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(offset, expected)| lines[index + offset].trim() == expected.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{
        LocalTool, LocalToolInvocation, LocalToolPayload, LocalToolSource,
    };
    use agent_core::ToolExecutionContext;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn apply_patch_overwrites_existing_file_with_add_file() {
        let base = test_workspace("apply_patch_overwrites_existing_file_with_add_file");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        tool.invoke(
            tool_invocation(serde_json::json!({
                "patch": "*** Begin Patch\n*** Add File: src/lib.rs\n+new\n*** End Patch"
            })),
            &ctx,
        )
        .await
        .expect("apply patch");

        let updated = fs::read_to_string(base.join("src/lib.rs"))
            .await
            .expect("read updated file");
        assert_eq!(updated, "new\n");
    }

    #[tokio::test]
    async fn apply_patch_refuses_to_delete_missing_file() {
        let base = test_workspace("apply_patch_refuses_to_delete_missing_file");
        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let result = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Delete File: src/missing.rs\n*** End Patch"
                })),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn apply_patch_rejects_delete_section_with_hunks() {
        let base = test_workspace("apply_patch_rejects_delete_section_with_hunks");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Delete File: src/lib.rs\n-old\n*** End Patch"
                })),
                &ctx,
            )
            .await
            .expect_err("delete section with hunk should fail");

        assert!(
            err.to_string()
                .contains("delete file section for `src/lib.rs` must not contain hunks")
        );
    }

    #[tokio::test]
    async fn apply_patch_rejects_plain_unified_diff_with_actionable_error() {
        let base = test_workspace("apply_patch_rejects_plain_unified_diff_with_actionable_error");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "before\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+after\n"
                })),
                &ctx,
            )
            .await
            .expect_err("plain unified diff should be rejected");

        assert!(
            err.to_string()
                .contains("only accepts the CloudAgent patch format")
        );
    }

    #[tokio::test]
    async fn apply_patch_rejects_empty_update_section() {
        let base = test_workspace("apply_patch_rejects_empty_update_section");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch"
                })),
                &ctx,
            )
            .await
            .expect_err("empty update section should fail");

        assert!(
            err.to_string()
                .contains("update file section for `src/lib.rs` does not contain any hunks")
        );
    }

    #[tokio::test]
    async fn apply_patch_rejects_duplicate_file_sections() {
        let base = test_workspace("apply_patch_rejects_duplicate_file_sections");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** Update File: src/lib.rs\n@@\n-new\n+newer\n*** End Patch"
                })),
                &ctx,
            )
            .await
            .expect_err("duplicate sections should fail");

        assert!(
            err.to_string()
                .contains("patch contains duplicate file section for `src/lib.rs`")
        );
    }

    #[tokio::test]
    async fn apply_patch_does_not_write_partial_multi_file_patch() {
        let base = test_workspace("apply_patch_does_not_write_partial_multi_file_patch");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/first.rs"), "old\n")
            .await
            .expect("write first file");
        fs::write(base.join("src/second.rs"), "actual\n")
            .await
            .expect("write second file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/first.rs\n@@\n-old\n+new\n*** Update File: src/second.rs\n@@\n-expected\n+changed\n*** End Patch"
                })),
                &ctx,
            )
            .await
            .expect_err("second file mismatch should fail entire patch before writing");

        assert!(err.to_string().contains("Failed to find expected lines"));
        let first = fs::read_to_string(base.join("src/first.rs"))
            .await
            .expect("read first file");
        let second = fs::read_to_string(base.join("src/second.rs"))
            .await
            .expect("read second file");
        assert_eq!(first, "old\n");
        assert_eq!(second, "actual\n");
    }

    #[tokio::test]
    async fn apply_patch_supports_move_to() {
        let base = test_workspace("apply_patch_supports_move_to");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n*** Move to: src/main.rs\n@@\n-old\n+new\n*** End Patch"
                })),
                &ctx,
            )
            .await
            .expect("apply patch");

        assert!(!base.join("src/lib.rs").exists());
        let moved = fs::read_to_string(base.join("src/main.rs"))
            .await
            .expect("read moved file");
        assert_eq!(moved, "new\n");
    }

    #[tokio::test]
    async fn apply_patch_rejects_move_after_hunk() {
        let base = test_workspace("apply_patch_rejects_move_after_hunk");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** Move to: src/main.rs\n*** End Patch"
                })),
                &ctx,
            )
            .await
            .expect_err("move after hunk should fail");

        assert!(
            err.to_string()
                .contains("move target must appear before update hunks")
        );
    }

    #[tokio::test]
    async fn apply_patch_uses_change_context_instead_of_line_numbers() {
        let base = test_workspace("apply_patch_uses_change_context_instead_of_line_numbers");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            "fn a() {\n    println!(\"same\");\n}\n\nfn b() {\n    println!(\"same\");\n}\n",
        )
        .await
        .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        tool.invoke(
                tool_invocation(serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@ fn b() {\n-    println!(\"same\");\n+    println!(\"changed\");\n*** End Patch"
            })),
            &ctx,
        )
        .await
        .expect("apply patch");

        let updated = fs::read_to_string(base.join("src/lib.rs"))
            .await
            .expect("read updated file");
        assert!(updated.contains("fn a() {\n    println!(\"same\");\n}"));
        assert!(updated.contains("fn b() {\n    println!(\"changed\");\n}"));
    }

    #[test]
    fn apply_patch_can_apply_hunk_without_position_hint() {
        let content = "same\nsame\n";
        let hunk = Hunk {
            change_context: None,
            is_end_of_file: false,
            lines: vec!["-same".to_string(), "+changed".to_string()],
        };

        let updated = apply_single_hunk(content, LineEnding::Lf, &hunk).expect("apply hunk");
        assert_eq!(updated, "changed\nsame\n");
    }

    #[test]
    fn apply_patch_uses_change_context_to_disambiguate() {
        let content = "fn a() {\n    same\n}\n\nfn b() {\n    same\n}\n";
        let hunk = Hunk {
            change_context: Some("fn b() {".to_string()),
            is_end_of_file: false,
            lines: vec!["-    same".to_string(), "+    changed".to_string()],
        };

        let updated = apply_single_hunk(content, LineEnding::Lf, &hunk).expect("apply hunk");
        assert!(updated.contains("fn a() {\n    same\n}"));
        assert!(updated.contains("fn b() {\n    changed\n}"));
    }

    #[test]
    fn apply_patch_can_append_at_end_of_file() {
        let content = "one\n";
        let hunk = Hunk {
            change_context: None,
            is_end_of_file: true,
            lines: vec!["+two".to_string()],
        };

        let updated = apply_single_hunk(content, LineEnding::Lf, &hunk).expect("apply hunk");
        assert_eq!(updated, "one\ntwo\n");
    }

    #[test]
    fn apply_patch_computes_replacements_before_applying_them() {
        let content = "line1\nline2\nline3\n";
        let hunks = vec![
            Hunk {
                change_context: None,
                is_end_of_file: false,
                lines: vec!["+after-context".to_string(), "+second-line".to_string()],
            },
            Hunk {
                change_context: None,
                is_end_of_file: false,
                lines: vec![
                    " line1".to_string(),
                    "-line2".to_string(),
                    "-line3".to_string(),
                    "+line2-replacement".to_string(),
                ],
            },
        ];

        let updated = apply_hunks(content, LineEnding::Lf, &hunks).expect("apply hunks");
        assert_eq!(
            updated,
            "line1\nline2-replacement\nafter-context\nsecond-line\n"
        );
    }

    #[test]
    fn apply_patch_treats_blank_hunk_lines_as_blank_context() {
        let content = "before\n\nafter\n";
        let hunk = Hunk {
            change_context: None,
            is_end_of_file: false,
            lines: vec![
                " before".to_string(),
                String::new(),
                "-after".to_string(),
                "+done".to_string(),
            ],
        };

        let updated = apply_single_hunk(content, LineEnding::Lf, &hunk).expect("apply hunk");
        assert_eq!(updated, "before\n\ndone\n");
    }

    #[tokio::test]
    async fn apply_patch_preserves_crlf_line_endings() {
        let base = test_workspace("apply_patch_preserves_crlf_line_endings");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            b"fn a() {\r\n    println!(\"same\");\r\n}\r\n",
        )
        .await
        .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        tool.invoke(
            tool_invocation(serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@ -1,3 +1,3 @@\n fn a() {\n-    println!(\"same\");\n+    println!(\"changed\");\n }\n*** End Patch"
            })),
            &ctx,
        )
        .await
        .expect("apply patch");

        let updated = fs::read(base.join("src/lib.rs"))
            .await
            .expect("read updated file");
        assert_eq!(
            String::from_utf8(updated).expect("utf8"),
            "fn a() {\r\n    println!(\"changed\");\r\n}\r\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_preserves_utf8_bom() {
        let base = test_workspace("apply_patch_preserves_utf8_bom");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            [0xEF, 0xBB, 0xBF]
                .into_iter()
                .chain(b"fn a() {\n    println!(\"same\");\n}\n".iter().copied())
                .collect::<Vec<_>>(),
        )
        .await
        .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        tool.invoke(
            tool_invocation(serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@ -1,3 +1,3 @@\n fn a() {\n-    println!(\"same\");\n+    println!(\"changed\");\n }\n*** End Patch"
            })),
            &ctx,
        )
        .await
        .expect("apply patch");

        let updated = fs::read(base.join("src/lib.rs"))
            .await
            .expect("read updated file");
        assert!(updated.starts_with(&[0xEF, 0xBB, 0xBF]));
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

    fn tool_context(workspace_root: &std::path::Path) -> ToolExecutionContext {
        ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::WorkspaceWrite,
            default_shell_timeout_ms: 5_000,
            max_tool_output_tokens: ToolExecutionContext::default_max_tool_output_tokens(),
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn tool_invocation(arguments: serde_json::Value) -> LocalToolInvocation {
        LocalToolInvocation {
            identity: agent_core::ToolIdentity::built_in("apply_patch"),
            source: LocalToolSource::BuiltIn,
            payload: LocalToolPayload::Function { arguments },
        }
    }
}
