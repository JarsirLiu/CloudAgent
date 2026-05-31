use crate::impls::file_read_state::FileReadStateStore;
use crate::impls::file_version::version_token_for_bytes;
use crate::impls::result_format::{finalize, push_fact, push_list_section};
use crate::impls::text_codec::{LineEnding, decode_text_file, encode_text_file};
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_workspace_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{
    StructuredToolResult, ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec,
    TurnItemDeltaKind, TurnItemKind, WriteFileStatus,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

pub struct EditFileTool;

const MAX_EDIT_FILE_BYTES: u64 = 2 * 1024 * 1024;
const LEFT_SINGLE_CURLY_QUOTE: char = '‘';
const RIGHT_SINGLE_CURLY_QUOTE: char = '’';
const LEFT_DOUBLE_CURLY_QUOTE: char = '“';
const RIGHT_DOUBLE_CURLY_QUOTE: char = '”';

const EDIT_FILE_TOOL_DESCRIPTION: &str = r#"Edit a workspace file by replacing an exact string with new content.
Use this for most code and text edits instead of writing a patch by hand.

Provide:
- `path`: a relative workspace path
- `edits`: one or more exact replacements to apply in order within that file
  - `old_string`: the exact text to replace
  - `new_string`: the replacement text
  - `replace_all`: optional, defaults to false

Rules:
- One `edit_file` call edits exactly one file; use multiple tool calls for multi-file changes
- Use relative workspace paths only
- Every edit's `old_string` and `new_string` must differ
- If the target file does not exist, provide exactly one edit with `old_string: ""` to create it
- Existing files must have a prior `read_file` witness in this conversation, and the current file must still match that witnessed version
- Use the smallest exact `old_string` that is still unique, usually 2-4 adjacent lines
- Preserve exact indentation and do not include the leading line numbers shown by `read_file`
- If `replace_all` is false, `old_string` must match exactly once at the moment that edit runs
- If `old_string` matches multiple locations, provide more surrounding context or set `replace_all` to true
- The edits run in order and must not depend on accidentally re-matching text introduced by earlier edits
- If the file uses CRLF or a BOM, they are preserved automatically
"#;

impl EditFileTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Medium,
            ToolPermissionTier::WorkspaceWrite,
            vec!["edit", "fs"],
            ToolUsageGuidance {
                selection_priority: 20,
                preferred_for: vec![
                    "single-file code edits",
                    "replacing exact text in known files",
                    "most workspace file modifications",
                ],
                avoid_for: vec![
                    "broad repository discovery",
                    "build or runtime verification",
                    "multi-file patch authoring",
                ],
                follow_up_hint: Some(
                    "after editing, inspect the diff and run the narrowest relevant verification",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "edit_file".to_string(),
                identity: ToolIdentity::built_in("edit_file"),
                description: EDIT_FILE_TOOL_DESCRIPTION.to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "edits": {
                            "type": "array",
                            "minItems": 1,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_string": { "type": "string" },
                                    "new_string": { "type": "string" },
                                    "replace_all": { "type": "boolean", "default": false }
                                },
                                "required": ["old_string", "new_string"]
                            }
                        }
                    },
                    "required": ["path", "edits"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: true,
                item_kind: TurnItemKind::FileChange,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: Some("Editing files can modify workspace contents.".to_string()),
            },
        )
    }
}

#[derive(Debug, Deserialize)]
struct EditFileArgs {
    path: String,
    edits: Vec<EditInstruction>,
}

#[derive(Debug, Clone, Deserialize)]
struct EditInstruction {
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

pub(crate) struct EditFileLocalTool {
    pub(crate) read_state: FileReadStateStore,
}

struct PreparedEdit {
    changed_path: String,
    path: std::path::PathBuf,
    decoded: crate::impls::text_codec::DecodedTextFile,
    edits: Vec<PreparedInstruction>,
}

#[derive(Debug, Clone)]
struct PreparedInstruction {
    old_string: String,
    new_string: String,
    replace_all: bool,
}

#[async_trait]
impl LocalTool for EditFileLocalTool {
    fn spec(&self) -> ToolSpec {
        EditFileTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: EditFileArgs = invocation.payload.parse_arguments()?;
        let plan = self.prepare_edit(args, ctx).await?;
        self.apply_edit(plan, ctx).await
    }
}

impl EditFileLocalTool {
    async fn prepare_edit(
        &self,
        args: EditFileArgs,
        ctx: &ToolExecutionContext,
    ) -> Result<PreparedEdit> {
        if args.edits.is_empty() {
            bail!("`edits` must contain at least one replacement");
        }
        for edit in &args.edits {
            if edit.old_string == edit.new_string {
                bail!("each edit must change the file; `old_string` and `new_string` must differ");
            }
            reject_line_number_prefixed_text("old_string", &edit.old_string)?;
            reject_line_number_prefixed_text("new_string", &edit.new_string)?;
        }

        let changed_path = args.path.clone();
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let file_exists = fs::try_exists(&path).await?;

        if !file_exists {
            if args.edits.len() != 1 || !args.edits[0].old_string.is_empty() {
                bail!(
                    "file does not exist; provide exactly one edit with `old_string: \"\"` to create it. If you meant to edit an existing file, rerun `read_file` and copy the exact current text."
                );
            }
            let decoded = crate::impls::text_codec::DecodedTextFile {
                text: String::new(),
                encoding: crate::impls::text_codec::TextEncoding::Utf8,
                line_ending: LineEnding::Lf,
            };
            return Ok(PreparedEdit {
                changed_path,
                path,
                decoded,
                edits: vec![PreparedInstruction {
                    old_string: String::new(),
                    new_string: args.edits[0].new_string.clone(),
                    replace_all: false,
                }],
            });
        }

        let metadata = fs::metadata(&path).await?;
        if metadata.len() > MAX_EDIT_FILE_BYTES {
            bail!(
                "refusing to edit {} because it is larger than {} bytes; narrow the target or use a more specialized workflow",
                path.display(),
                MAX_EDIT_FILE_BYTES
            );
        }

        let Some(snapshot) = self
            .read_state
            .get_or_restore(
                &ctx.conversation_id,
                &ctx.workspace_root,
                &ctx.conversation_store_dir,
                &path,
            )
            .await?
        else {
            bail!(
                "edit_file requires a prior `read_file` witness for {}; run `read_file` on the file before editing",
                path.display()
            );
        };
        if snapshot.version_token.is_none() {
            bail!(
                "latest available read of {} did not capture a reusable file version; rerun `read_file` on the file before editing",
                path.display()
            );
        }
        if snapshot.is_partial_view {
            bail!(
                "latest available read of {} was partial; rerun `read_file` without `start_line` or `max_lines` before editing",
                path.display()
            );
        }

        let current_bytes = fs::read(&path).await?;
        let current_version_token = version_token_for_bytes(&current_bytes);
        if snapshot.version_token.as_deref() != Some(current_version_token.as_str()) {
            bail!(
                "file changed since it was last read; rerun `read_file` for {} before editing",
                path.display()
            );
        }
        let decoded = decode_text_file(&current_bytes).map_err(|err| {
            anyhow::anyhow!("failed to edit {}: {}", path.display(), err.render())
        })?;

        let prepared_edits = prepare_instruction_sequence(&decoded, &args.edits, &path)?;

        Ok(PreparedEdit {
            changed_path,
            path,
            decoded,
            edits: prepared_edits,
        })
    }

    async fn apply_edit(
        &self,
        plan: PreparedEdit,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        if !fs::try_exists(&plan.path).await? {
            let Some(parent) = plan.path.parent() else {
                bail!(
                    "cannot determine parent directory for {}",
                    plan.path.display()
                );
            };
            fs::create_dir_all(parent).await?;
            let created_bytes = plan.edits[0].new_string.as_bytes().to_vec();
            let version_token = version_token_for_bytes(&created_bytes);
            fs::write(&plan.path, &created_bytes).await?;
            self.read_state
                .record_full(&ctx.conversation_id, &plan.path, version_token.clone())
                .await;
            return Ok(completed_edit_output(plan.changed_path, version_token));
        }

        let mut updated = plan.decoded.text.clone();
        for edit in &plan.edits {
            updated = if edit.old_string.is_empty() {
                edit.new_string.clone()
            } else if edit.replace_all {
                updated.replace(&edit.old_string, &edit.new_string)
            } else {
                updated.replacen(&edit.old_string, &edit.new_string, 1)
            };
        }

        if updated == plan.decoded.text {
            bail!(
                "edit produced no change for {}; verify that `old_string` and `new_string` reflect the intended update",
                plan.path.display()
            );
        }

        let updated_bytes = encode_text_file(&plan.decoded, &updated);
        let version_token = version_token_for_bytes(&updated_bytes);
        fs::write(&plan.path, &updated_bytes).await?;
        self.read_state
            .record_full(&ctx.conversation_id, &plan.path, version_token.clone())
            .await;
        Ok(completed_edit_output(plan.changed_path, version_token))
    }
}

fn completed_edit_output(changed_path: String, version_token: String) -> ToolInvocationOutput {
    let mut lines = Vec::new();
    push_fact(&mut lines, "Status", "completed");
    push_fact(&mut lines, "Files changed", "1");
    push_list_section(
        &mut lines,
        "Changed paths",
        std::slice::from_ref(&changed_path),
    );
    push_fact(&mut lines, "Version token", version_token.clone());
    ToolInvocationOutput {
        content: finalize(
            format!("Edited {} successfully.", changed_path),
            lines,
            Some("inspect the diff and run the narrowest relevant verification"),
        ),
        structured: Some(StructuredToolResult::EditFile {
            changed_paths: vec![changed_path],
            files_changed: 1,
            status: WriteFileStatus::Completed,
            version_token: Some(version_token),
        }),
    }
}

fn normalize_old_string_for_file(
    old_string: &str,
    file_text: &str,
    line_ending: LineEnding,
) -> String {
    if old_string.is_empty() {
        return String::new();
    }
    if file_text.contains(old_string) {
        return old_string.to_string();
    }
    if line_ending == LineEnding::CrLf && old_string.contains('\n') && !old_string.contains("\r\n")
    {
        let crlf = old_string.replace('\n', "\r\n");
        if file_text.contains(&crlf) {
            return crlf;
        }
    }
    if let Some(actual) = find_actual_string_with_normalized_quotes(file_text, old_string) {
        return actual;
    }
    old_string.to_string()
}

fn normalize_new_string_for_file(
    new_string: &str,
    original_old_string: &str,
    actual_old_string: &str,
    line_ending: LineEnding,
) -> String {
    let mut normalized = preserve_quote_style(original_old_string, actual_old_string, new_string);
    if line_ending == LineEnding::CrLf {
        normalized = normalized.replace('\n', "\r\n");
    }
    normalized
}

fn count_matches(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

fn repeated_match_line_hint(haystack: &str, needle: &str) -> String {
    let mut lines = Vec::new();
    for (offset, _) in haystack.match_indices(needle).take(4) {
        let line = haystack[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        lines.push(line.to_string());
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!(" (for example near line {})", lines.join(", "))
    }
}

fn reject_line_number_prefixed_text(field_name: &str, value: &str) -> Result<()> {
    for line in value.lines().filter(|line| !line.trim().is_empty()) {
        let digit_count = line
            .chars()
            .take_while(|ch| ch.is_ascii_whitespace())
            .count();
        let trimmed = line.trim_start();
        let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
        if digits == 0 {
            continue;
        }
        let remainder = &trimmed[digits..];
        if remainder.starts_with("  ") && digit_count + digits + 2 < line.len() {
            bail!(
                "`{field_name}` appears to include leading line numbers from `read_file`. Copy only the file text after the line number prefix."
            );
        }
    }
    Ok(())
}

fn prepare_instruction_sequence(
    decoded: &crate::impls::text_codec::DecodedTextFile,
    edits: &[EditInstruction],
    path: &std::path::Path,
) -> Result<Vec<PreparedInstruction>> {
    let mut prepared = Vec::with_capacity(edits.len());
    let mut virtual_text = decoded.text.clone();
    let mut applied_new_strings: Vec<String> = Vec::new();

    for edit in edits {
        let old_string =
            normalize_old_string_for_file(&edit.old_string, &virtual_text, decoded.line_ending);
        let new_string = normalize_new_string_for_file(
            &edit.new_string,
            &edit.old_string,
            &old_string,
            decoded.line_ending,
        );

        if old_string.is_empty() {
            if !virtual_text.is_empty() {
                bail!(
                    "refusing to use an empty `old_string` on a non-empty file; provide the exact existing text to replace"
                );
            }
        } else {
            for previous_new_string in &applied_new_strings {
                let old_without_trailing_newlines = old_string.trim_end_matches(['\n', '\r']);
                if !old_without_trailing_newlines.is_empty()
                    && previous_new_string.contains(old_without_trailing_newlines)
                {
                    bail!(
                        "cannot safely apply sequential edits in {}; a later `old_string` is a substring of text introduced by an earlier edit. Merge the surrounding context into one edit or make the later `old_string` more specific.",
                        path.display()
                    );
                }
            }

            let match_count = count_matches(&virtual_text, &old_string);
            if match_count == 0 {
                bail!(
                    "`old_string` was not found in {}. Rerun `read_file` and copy the exact current text, including indentation and surrounding lines.",
                    path.display()
                );
            }
            if match_count > 1 && !edit.replace_all {
                let line_hint = repeated_match_line_hint(&virtual_text, &old_string);
                bail!(
                    "`old_string` matched {match_count} locations in {}{}. Provide a more specific 2-4 line match with nearby context, or set `replace_all` to true if every occurrence should change.",
                    path.display(),
                    line_hint
                );
            }
        }

        let next_virtual_text = if old_string.is_empty() {
            new_string.clone()
        } else if edit.replace_all {
            virtual_text.replace(&old_string, &new_string)
        } else {
            virtual_text.replacen(&old_string, &new_string, 1)
        };
        if next_virtual_text == virtual_text {
            bail!(
                "edit produced no change for {}; verify that `old_string` and `new_string` reflect the intended update",
                path.display()
            );
        }

        applied_new_strings.push(new_string.clone());
        virtual_text = next_virtual_text;
        prepared.push(PreparedInstruction {
            old_string,
            new_string,
            replace_all: edit.replace_all,
        });
    }

    Ok(prepared)
}

fn normalize_quotes(value: &str) -> String {
    value
        .replace([LEFT_SINGLE_CURLY_QUOTE, RIGHT_SINGLE_CURLY_QUOTE], "'")
        .replace([LEFT_DOUBLE_CURLY_QUOTE, RIGHT_DOUBLE_CURLY_QUOTE], "\"")
}

fn find_actual_string_with_normalized_quotes(file_text: &str, search: &str) -> Option<String> {
    let normalized_search = normalize_quotes(search);
    let normalized_file = normalize_quotes(file_text);
    let start = normalized_file.find(&normalized_search)?;
    file_text
        .chars()
        .skip(start)
        .take(search.chars().count())
        .collect::<String>()
        .into()
}

fn preserve_quote_style(original_old: &str, actual_old: &str, new_string: &str) -> String {
    if original_old == actual_old {
        return new_string.to_string();
    }

    let has_double_quotes = actual_old.contains(LEFT_DOUBLE_CURLY_QUOTE)
        || actual_old.contains(RIGHT_DOUBLE_CURLY_QUOTE);
    let has_single_quotes = actual_old.contains(LEFT_SINGLE_CURLY_QUOTE)
        || actual_old.contains(RIGHT_SINGLE_CURLY_QUOTE);

    let mut result = new_string.to_string();
    if has_double_quotes {
        result = apply_curly_double_quotes(&result);
    }
    if has_single_quotes {
        result = apply_curly_single_quotes(&result);
    }
    result
}

fn apply_curly_double_quotes(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(value.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '"' {
            out.push(if is_opening_quote_context(&chars, index) {
                LEFT_DOUBLE_CURLY_QUOTE
            } else {
                RIGHT_DOUBLE_CURLY_QUOTE
            });
        } else {
            out.push(*ch);
        }
    }
    out
}

fn apply_curly_single_quotes(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(value.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '\'' {
            let prev = index.checked_sub(1).and_then(|i| chars.get(i));
            let next = chars.get(index + 1);
            let prev_is_letter = prev.is_some_and(|ch| ch.is_alphabetic());
            let next_is_letter = next.is_some_and(|ch| ch.is_alphabetic());
            if prev_is_letter && next_is_letter {
                out.push(RIGHT_SINGLE_CURLY_QUOTE);
            } else {
                out.push(if is_opening_quote_context(&chars, index) {
                    LEFT_SINGLE_CURLY_QUOTE
                } else {
                    RIGHT_SINGLE_CURLY_QUOTE
                });
            }
        } else {
            out.push(*ch);
        }
    }
    out
}

fn is_opening_quote_context(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    matches!(
        chars[index - 1],
        ' ' | '\t' | '\n' | '\r' | '(' | '[' | '{' | '\u{2014}' | '\u{2013}'
    )
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
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn edit_file_replaces_single_match() {
        let base = test_workspace("edit_file_replaces_single_match");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "before\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &base.join("src/lib.rs")).await;
        let tool = EditFileLocalTool { read_state };
        tool.invoke(
            tool_invocation(serde_json::json!({
                "path": "src/lib.rs",
                "edits": [{
                    "old_string": "before\n",
                    "new_string": "after\n"
                }]
            })),
            &tool_context(&base),
        )
        .await
        .expect("edit file");

        let updated = fs::read_to_string(base.join("src/lib.rs"))
            .await
            .expect("read file");
        assert_eq!(updated, "after\n");
    }

    #[tokio::test]
    async fn edit_file_creates_missing_file_when_old_string_empty() {
        let base = test_workspace("edit_file_creates_missing_file_when_old_string_empty");
        let tool = EditFileLocalTool {
            read_state: FileReadStateStore::new(),
        };
        tool.invoke(
            tool_invocation(serde_json::json!({
                "path": "src/new.rs",
                "edits": [{
                    "old_string": "",
                    "new_string": "created\n"
                }]
            })),
            &tool_context(&base),
        )
        .await
        .expect("create file");

        let updated = fs::read_to_string(base.join("src/new.rs"))
            .await
            .expect("read file");
        assert_eq!(updated, "created\n");
    }

    #[tokio::test]
    async fn edit_file_requires_replace_all_for_multiple_matches() {
        let base = test_workspace("edit_file_requires_replace_all_for_multiple_matches");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "same\nsame\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &base.join("src/lib.rs")).await;
        let tool = EditFileLocalTool { read_state };
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "edits": [{
                        "old_string": "same\n",
                        "new_string": "changed\n"
                    }]
                })),
                &tool_context(&base),
            )
            .await
            .expect_err("should reject multiple matches");

        assert!(err.to_string().contains("matched 2 locations"));
        assert!(err.to_string().contains("near line"));
    }

    #[tokio::test]
    async fn edit_file_preserves_crlf_and_bom() {
        let base = test_workspace("edit_file_preserves_crlf_and_bom");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            [0xEF, 0xBB, 0xBF]
                .into_iter()
                .chain(b"line1\r\nline2\r\n".iter().copied())
                .collect::<Vec<_>>(),
        )
        .await
        .expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &base.join("src/lib.rs")).await;
        let tool = EditFileLocalTool { read_state };
        tool.invoke(
            tool_invocation(serde_json::json!({
                "path": "src/lib.rs",
                "edits": [{
                    "old_string": "line2\n",
                    "new_string": "changed\n"
                }]
            })),
            &tool_context(&base),
        )
        .await
        .expect("edit");

        let updated = fs::read(base.join("src/lib.rs")).await.expect("read");
        assert!(updated.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert_eq!(
            String::from_utf8(updated[3..].to_vec()).expect("utf8"),
            "line1\r\nchanged\r\n"
        );
    }

    #[tokio::test]
    async fn edit_file_rejects_stale_read_witness() {
        let base = test_workspace("edit_file_rejects_stale_read_witness");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        let path = base.join("src/lib.rs");
        fs::write(&path, "before\n").await.expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &path).await;
        fs::write(&path, "external change\n")
            .await
            .expect("mutate after read");

        let tool = EditFileLocalTool { read_state };
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "edits": [{
                        "old_string": "before\n",
                        "new_string": "changed\n"
                    }]
                })),
                &tool_context(&base),
            )
            .await
            .expect_err("stale witness should fail");

        assert!(
            err.to_string()
                .contains("file changed since it was last read")
        );
    }

    #[tokio::test]
    async fn edit_file_rejects_partial_read_witness() {
        let base = test_workspace("edit_file_rejects_partial_read_witness");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        let path = base.join("src/lib.rs");
        fs::write(&path, "line1\nline2\nline3\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        let bytes = fs::read(&path).await.expect("read file for token");
        let token = version_token_for_bytes(&bytes);
        read_state
            .record_snapshot("test", &path, Some(token), true)
            .await;

        let tool = EditFileLocalTool { read_state };
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "edits": [{
                        "old_string": "line2\n",
                        "new_string": "changed\n"
                    }]
                })),
                &tool_context(&base),
            )
            .await
            .expect_err("partial read witness should fail");

        assert!(err.to_string().contains("was partial"));
    }

    #[tokio::test]
    async fn edit_file_accepts_full_read_witness_when_version_matches() {
        let base = test_workspace("edit_file_accepts_full_read_witness_when_version_matches");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        let path = base.join("src/lib.rs");
        fs::write(&path, "line1\nline2\nline3\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        let bytes = fs::read(&path).await.expect("read file for token");
        let token = version_token_for_bytes(&bytes);
        read_state
            .record_snapshot("test", &path, Some(token), false)
            .await;

        let tool = EditFileLocalTool { read_state };
        tool.invoke(
            tool_invocation(serde_json::json!({
                "path": "src/lib.rs",
                "edits": [{
                    "old_string": "line2\n",
                    "new_string": "changed\n"
                }]
            })),
            &tool_context(&base),
        )
        .await
        .expect("edit file");

        let updated = fs::read_to_string(&path).await.expect("read file");
        assert_eq!(updated, "line1\nchanged\nline3\n");
    }

    #[tokio::test]
    async fn edit_file_rejects_line_number_prefixed_old_string() {
        let base = test_workspace("edit_file_rejects_line_number_prefixed_old_string");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "before\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &base.join("src/lib.rs")).await;
        let tool = EditFileLocalTool { read_state };
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "edits": [{
                        "old_string": "     1  before\n",
                        "new_string": "after\n"
                    }]
                })),
                &tool_context(&base),
            )
            .await
            .expect_err("should reject line-number-prefixed old_string");

        assert!(err.to_string().contains("leading line numbers"));
    }

    #[tokio::test]
    async fn edit_file_applies_multiple_edits_in_one_call() {
        let base = test_workspace("edit_file_applies_multiple_edits_in_one_call");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "alpha\nbeta\ngamma\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &base.join("src/lib.rs")).await;
        let tool = EditFileLocalTool { read_state };
        tool.invoke(
            tool_invocation(serde_json::json!({
                "path": "src/lib.rs",
                "edits": [
                    {
                        "old_string": "alpha\n",
                        "new_string": "ALPHA\n"
                    },
                    {
                        "old_string": "gamma\n",
                        "new_string": "GAMMA\n"
                    }
                ]
            })),
            &tool_context(&base),
        )
        .await
        .expect("edit file");

        let updated = fs::read_to_string(base.join("src/lib.rs"))
            .await
            .expect("read file");
        assert_eq!(updated, "ALPHA\nbeta\nGAMMA\n");
    }

    #[tokio::test]
    async fn edit_file_rejects_sequential_edits_that_retarget_inserted_text() {
        let base = test_workspace("edit_file_rejects_sequential_edits_that_retarget_inserted_text");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "before\n")
            .await
            .expect("write");

        let read_state = FileReadStateStore::new();
        seed_full_read(&read_state, &base.join("src/lib.rs")).await;
        let tool = EditFileLocalTool { read_state };
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "edits": [
                        {
                            "old_string": "before\n",
                            "new_string": "middle value\n"
                        },
                        {
                            "old_string": "value",
                            "new_string": "final"
                        }
                    ]
                })),
                &tool_context(&base),
            )
            .await
            .expect_err("should reject sequential overlapping edits");

        assert!(
            err.to_string()
                .contains("substring of text introduced by an earlier edit")
        );
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
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    async fn seed_full_read(read_state: &FileReadStateStore, path: &std::path::Path) {
        let bytes = fs::read(path).await.expect("read file for token");
        let token = version_token_for_bytes(&bytes);
        read_state.record_full("test", path, token).await;
    }

    fn tool_invocation(arguments: serde_json::Value) -> LocalToolInvocation {
        LocalToolInvocation {
            identity: agent_core::ToolIdentity::built_in("edit_file"),
            source: LocalToolSource::BuiltIn,
            payload: LocalToolPayload::Function { arguments },
        }
    }
}
