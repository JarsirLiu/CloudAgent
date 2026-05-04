use crate::impls::text_codec::{LineEnding, decode_text_file, encode_text_file};
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_workspace_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{ToolExecutionContext, ToolIdentity, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
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
            false,
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
                preferred_task_kinds: vec![
                    agent_core::TaskKind::CodeEdit,
                    agent_core::TaskKind::WorkspaceFileOperation,
                ],
                preferred_modes: vec![agent_core::ToolMode::Edit],
                avoid_task_kinds: vec![
                    agent_core::TaskKind::RepositoryAnalysis,
                    agent_core::TaskKind::Verification,
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
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::FileChange,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
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
        let mut changed_files = BTreeSet::new();
        for file_patch in file_patches {
            match (file_patch.old_path.as_str(), file_patch.new_path.as_str()) {
                ("/dev/null", new_path) => {
                    let path = resolve_workspace_path(&ctx.workspace_root, Some(new_path))?;
                    if path.exists() {
                        anyhow::bail!("refusing to add existing file {}", path.display());
                    }
                    let next = render_hunks_as_new_file(&file_patch.hunks)?;
                    let Some(parent) = path.parent() else {
                        anyhow::bail!("cannot determine parent directory for {}", path.display());
                    };
                    fs::create_dir_all(parent).await?;
                    fs::write(&path, next.as_bytes()).await?;
                    changed_files.insert(new_path.to_string());
                }
                (old_path, "/dev/null") => {
                    let path = resolve_workspace_path(&ctx.workspace_root, Some(old_path))?;
                    if !path.exists() {
                        anyhow::bail!("refusing to delete missing file {}", path.display());
                    }
                    fs::remove_file(&path).await?;
                    changed_files.insert(old_path.to_string());
                }
                (_, new_path) => {
                    if file_patch.old_path != file_patch.new_path {
                        anyhow::bail!(
                            "file moves/renames are not supported by apply_patch; use a delete and add patch instead"
                        );
                    }
                    let path = resolve_workspace_path(&ctx.workspace_root, Some(new_path))?;
                    if !path.exists() {
                        anyhow::bail!("refusing to update missing file {}", path.display());
                    }
                    let current_bytes = fs::read(&path).await?;
                    let decoded = decode_text_file(&current_bytes).map_err(|err| {
                        anyhow::anyhow!("failed to apply patch for {}: {}", new_path, err.render())
                    })?;
                    let next = apply_hunks(&decoded.text, decoded.line_ending, &file_patch.hunks)
                        .map_err(|err| {
                        anyhow::anyhow!("failed to apply patch for {}: {err}", new_path)
                    })?;
                    if next != decoded.text {
                        let Some(parent) = path.parent() else {
                            anyhow::bail!(
                                "cannot determine parent directory for {}",
                                path.display()
                            );
                        };
                        fs::create_dir_all(parent).await?;
                        fs::write(&path, encode_text_file(&decoded, &next)).await?;
                        changed_files.insert(new_path.to_string());
                    }
                }
            }
        }
        let files_changed = changed_files.len();
        let changed_paths = changed_files.into_iter().collect::<Vec<_>>();

        Ok(ToolInvocationOutput {
            content: format!("Applied patch. files_changed={files_changed}"),
            structured: Some(agent_protocol::StructuredToolResult::EditFile {
                changed_paths,
                files_changed,
                status: agent_protocol::WriteFileStatus::Completed,
                version_token: None,
            }),
        })
    }
}

#[derive(Debug)]
struct FilePatch {
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    old_start_line: Option<usize>,
    lines: Vec<String>,
}

fn parse_unified_patch(patch: &str) -> anyhow::Result<Vec<FilePatch>> {
    let mut file_patches = Vec::new();
    let mut current_old_path: Option<String> = None;
    let mut current_new_path: Option<String> = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk: Option<Hunk> = None;
    let mut in_patch_block = false;

    let flush_current = |file_patches: &mut Vec<FilePatch>,
                         current_old_path: &mut Option<String>,
                         current_new_path: &mut Option<String>,
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
                hunks: std::mem::take(hunks),
            });
        }
    };

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
                &mut hunks,
                &mut current_hunk,
            );
            let path = path.trim().to_string();
            current_old_path = Some(path.clone());
            current_new_path = Some(path);
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
                old_start_line: parse_old_start_line(line),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(hunk) = current_hunk.as_mut() {
            if line.starts_with('\\') {
                continue;
            }
            hunk.lines.push(line.to_string());
        }
    }
    flush_current(
        &mut file_patches,
        &mut current_old_path,
        &mut current_new_path,
        &mut hunks,
        &mut current_hunk,
    );
    Ok(file_patches)
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
    Ok(lines.join("\n"))
}

fn apply_hunks(original: &str, line_ending: LineEnding, hunks: &[Hunk]) -> anyhow::Result<String> {
    let mut content = original.to_string();
    for hunk in hunks {
        content = apply_single_hunk(&content, line_ending, hunk)?;
    }
    Ok(content)
}

fn apply_single_hunk(
    content: &str,
    line_ending: LineEnding,
    hunk: &Hunk,
) -> anyhow::Result<String> {
    let mut old_block: Vec<String> = Vec::new();
    let mut new_block: Vec<String> = Vec::new();
    for line in &hunk.lines {
        if let Some(rest) = line.strip_prefix(' ') {
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

    if old_block.is_empty() {
        anyhow::bail!("empty deletion context is not supported");
    }

    let has_trailing_newline = content.ends_with('\n');
    let mut lines = content.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let start = resolve_hunk_start(&lines, &old_block, hunk.old_start_line)?;
    let end = start + old_block.len();
    lines.splice(start..end, new_block);

    let mut next = lines.join(line_ending.as_str());
    if has_trailing_newline {
        next.push_str(line_ending.as_str());
    }
    Ok(next)
}

fn parse_old_start_line(header: &str) -> Option<usize> {
    let header = header.trim();
    let marker = header.strip_prefix("@@ -")?;
    let number = marker
        .split_whitespace()
        .next()?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;
    Some(number)
}

fn resolve_hunk_start(
    lines: &[String],
    old_block: &[String],
    preferred_start_line: Option<usize>,
) -> anyhow::Result<usize> {
    let Some(start_line) = preferred_start_line else {
        anyhow::bail!("patch hunk is missing a source line position");
    };
    let preferred_index = start_line.saturating_sub(1);
    if block_matches(lines, preferred_index, old_block) {
        return Ok(preferred_index);
    }
    anyhow::bail!("could not find hunk context at the expected source location")
}

fn block_matches(lines: &[String], start: usize, old_block: &[String]) -> bool {
    lines
        .get(start..start.saturating_add(old_block.len()))
        .is_some_and(|slice| slice == old_block)
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
    async fn apply_patch_refuses_to_add_existing_file() {
        let base = test_workspace("apply_patch_refuses_to_add_existing_file");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let result = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Add File: src/lib.rs\n+ new\n*** End Patch"
                })),
                &ctx,
            )
            .await;

        assert!(result.is_err());
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
    async fn apply_patch_refuses_rename_style_patch() {
        let base = test_workspace("apply_patch_refuses_rename_style_patch");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "old\n")
            .await
            .expect("write file");

        let tool = ApplyPatchLocalTool;
        let ctx = tool_context(&base);
        let result = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "patch": "--- a/src/lib.rs\n+++ b/src/main.rs\n@@\n- old\n+ new\n"
                })),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn apply_patch_uses_hunk_position_instead_of_first_text_match() {
        let base = test_workspace("apply_patch_uses_hunk_position_instead_of_first_text_match");
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
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@ -5,3 +5,3 @@\n fn b() {\n-    println!(\"same\");\n+    println!(\"changed\");\n }\n*** End Patch"
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
    fn apply_patch_rejects_hunk_without_position_hint() {
        let content = "same\nsame\n";
        let hunk = Hunk {
            old_start_line: None,
            lines: vec!["-same".to_string(), "+changed".to_string()],
        };

        let err = apply_single_hunk(content, LineEnding::Lf, &hunk)
            .expect_err("should reject missing position");
        assert!(err.to_string().contains("missing a source line position"));
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
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
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
