mod common;
mod read_file;
mod search_workspace;
mod text_read;

pub(crate) use common::DEFAULT_IGNORED_DIRS;
pub(crate) use read_file::ReadFileLocalTool;
pub use read_file::ReadFileTool;
pub(crate) use search_workspace::SearchWorkspaceLocalTool;
pub use search_workspace::SearchWorkspaceTool;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{
        LocalTool, LocalToolInvocation, LocalToolPayload, LocalToolSource,
    };
    use agent_core::{SearchWorkspaceMode, SearchWorkspaceOperation, StructuredToolResult};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn search_workspace_finds_files() {
        let base = test_workspace("search_workspace_finds_files");
        fs::create_dir_all(base.join("src/nested"))
            .await
            .expect("create nested dir");
        fs::write(base.join("src/service.rs"), "pub fn service() {}\n")
            .await
            .expect("write service file");
        fs::write(
            base.join("src/nested/service_impl.rs"),
            "pub fn impls() {}\n",
        )
        .await
        .expect("write impl file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: base.clone(),
            conversation_store_dir: base.clone(),
            permission_profile: agent_core::PermissionProfile::ReadOnly,
            default_shell_timeout_ms: 5_000,
            max_tool_output_tokens:
                agent_core::ToolExecutionContext::default_max_tool_output_tokens(),
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        };
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "files",
                    "pattern": "service.rs",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        let lines = output
            .content
            .lines()
            .map(str::trim)
            .map(ToOwned::to_owned)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        assert!(matches!(
            output.structured.as_ref(),
            Some(StructuredToolResult::SearchWorkspace {
                session_id,
                mode: SearchWorkspaceMode::Files,
                query,
                ..
            }) if session_id == "search:test:1" && query == "service.rs"
        ));
        assert!(
            lines
                .iter()
                .any(|line| line == "Summary: Found 2 file matches for `service.rs`; showing 2.")
        );
        assert!(lines.iter().any(|line| line == "Top hits:"));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("- 1. src/service.rs [match_kind=file_name"))
        );
        assert!(
            output
                .content
                .contains("Next step: open the strongest 2-3 plausible hits with `read_file` in parallel before editing")
        );
        assert!(matches!(
            output.structured.as_ref(),
            Some(StructuredToolResult::SearchWorkspace { hits, .. })
                if hits.first().and_then(|hit| hit.score).is_some()
                    && hits.first().and_then(|hit| hit.file_score).is_some()
                    && hits.first().and_then(|hit| hit.file_match_count) == Some(1)
                    && hits.first().and_then(|hit| hit.rank) == Some(1)
                    && hits.first().and_then(|hit| hit.match_kind.as_deref()) == Some("file_name")
        ));
    }

    #[tokio::test]
    async fn read_file_supports_single_path_reads() {
        let base = test_workspace("read_file_supports_single_path_reads");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "one\ntwo\nthree\nfour\n")
            .await
            .expect("write file");

        let tool = ReadFileLocalTool {
            max_read_chars: 10_000,
            read_state: crate::impls::file_read_state::FileReadStateStore::new(),
        };
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "start_line": 2,
                    "max_lines": 2
                })),
                &ctx,
            )
            .await
            .expect("read_file works");

        assert!(output.content.contains("==>"));
        assert!(output.content.contains("2  two"));
        assert!(output.content.contains("3  three"));
    }

    #[tokio::test]
    async fn search_workspace_returns_structured_text_matches() {
        let base = test_workspace("search_workspace_returns_structured_text_matches");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            "fn render_active_cell() {}\nfn render_live_status_line() {}\n",
        )
        .await
        .expect("write file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "render_",
                    "path_scope": "src",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        assert!(
            output
                .content
                .contains("Summary: Found 2 text matches in 1 files for `render_`.")
        );
        assert!(output.content.contains("Top files:"));
        assert!(output.content.contains("Matches:"));
        assert!(
            output
                .content
                .contains("src/lib.rs:1: fn render_active_cell() {}")
        );
        assert!(matches!(
            output.structured.as_ref(),
            Some(StructuredToolResult::SearchWorkspace { hits, .. })
                if hits.first().and_then(|hit| hit.score).is_some()
                    && hits.first().and_then(|hit| hit.file_score).is_some()
                    && hits.first().and_then(|hit| hit.file_match_count) == Some(2)
                    && hits.first().and_then(|hit| hit.rank) == Some(1)
        ));
    }

    #[tokio::test]
    async fn search_workspace_text_prefers_definition_like_hits() {
        let base = test_workspace("search_workspace_text_prefers_definition_like_hits");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "call_target();\nfn target() {}\n")
            .await
            .expect("write lib");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "target",
                    "path_scope": "src",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        let hits = match output.structured.as_ref() {
            Some(StructuredToolResult::SearchWorkspace { hits, .. }) => hits,
            other => panic!("expected structured hits, got {other:?}"),
        };
        assert_eq!(hits.first().and_then(|hit| hit.line), Some(2));
        assert_eq!(
            hits.first().and_then(|hit| hit.match_kind.as_deref()),
            Some("definition")
        );
    }

    #[tokio::test]
    async fn search_workspace_text_prefers_phrase_matches_over_single_term_noise() {
        let base =
            test_workspace("search_workspace_text_prefers_phrase_matches_over_single_term_noise");
        fs::create_dir_all(base.join("cli/src/input"))
            .await
            .expect("create input dir");
        fs::create_dir_all(base.join("cli/src/ui/widgets"))
            .await
            .expect("create widgets dir");
        fs::write(
            base.join("cli/src/input/completion.rs"),
            "pub fn show_tab_completion_popup() {}\n",
        )
        .await
        .expect("write completion file");
        fs::write(
            base.join("cli/src/ui/widgets/chat_composer.rs"),
            "fn accept_selected_completion() {}\n",
        )
        .await
        .expect("write composer file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "tab completion",
                    "path_scope": "cli",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        assert!(output.content.contains("Top files:"));
        assert!(output.content.contains("cli/src/input/completion.rs"));
        assert!(
            !output
                .content
                .contains("cli/src/ui/widgets/chat_composer.rs")
        );
    }

    #[tokio::test]
    async fn search_workspace_text_falls_back_to_multi_term_coverage_when_phrase_is_absent() {
        let base = test_workspace(
            "search_workspace_text_falls_back_to_multi_term_coverage_when_phrase_is_absent",
        );
        fs::create_dir_all(base.join("cli/src/input"))
            .await
            .expect("create input dir");
        fs::write(
            base.join("cli/src/input/completion.rs"),
            "fn handle_tab_key_for_completion_menu() {}\n",
        )
        .await
        .expect("write completion file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "tab completion",
                    "path_scope": "cli",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        assert!(output.content.contains("cli/src/input/completion.rs"));
        assert!(matches!(
            output.structured.as_ref(),
            Some(StructuredToolResult::SearchWorkspace { hits, .. })
                if hits.first().and_then(|hit| hit.match_kind.as_deref()) == Some("term_cover")
        ));
    }

    #[tokio::test]
    async fn search_workspace_text_truncation_preserves_file_diversity() {
        let base = test_workspace("search_workspace_text_truncation_preserves_file_diversity");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/a.rs"),
            "fn render_alpha() {}\nfn render_beta() {}\nfn render_gamma() {}\n",
        )
        .await
        .expect("write a");
        fs::write(base.join("src/b.rs"), "fn render_shell() {}\n")
            .await
            .expect("write b");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "render",
                    "path_scope": "src",
                    "max_results": 2
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        let hits = match output.structured.as_ref() {
            Some(StructuredToolResult::SearchWorkspace { hits, .. }) => hits,
            other => panic!("expected structured hits, got {other:?}"),
        };
        assert_eq!(hits.len(), 2);
        assert_ne!(hits[0].path, hits[1].path);
    }

    #[tokio::test]
    async fn search_workspace_text_prefers_semantic_handler_signals_for_tab_completion() {
        let base = test_workspace(
            "search_workspace_text_prefers_semantic_handler_signals_for_tab_completion",
        );
        fs::create_dir_all(base.join("cli/src/input"))
            .await
            .expect("create input dir");
        fs::create_dir_all(base.join("cli/src/ui/widgets"))
            .await
            .expect("create widgets dir");
        fs::write(
            base.join("cli/src/input/completion.rs"),
            "pub fn completion_menu() {}\n",
        )
        .await
        .expect("write completion file");
        fs::write(
            base.join("cli/src/ui/widgets/chat_composer.rs"),
            "fn accept_selected_completion() {}\nmatch key { KeyCode::Tab => accept_selected_completion(), _ => {} }\n",
        )
        .await
        .expect("write composer file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "tab completion",
                    "path_scope": "cli",
                    "max_results": 5
                })),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        let hits = match output.structured.as_ref() {
            Some(StructuredToolResult::SearchWorkspace { hits, .. }) => hits,
            other => panic!("expected structured hits, got {other:?}"),
        };
        assert_eq!(
            hits.first().map(|hit| hit.path.as_str()),
            Some("cli/src/ui/widgets/chat_composer.rs")
        );
        assert!(matches!(
            hits.first().and_then(|hit| hit.match_kind.as_deref()),
            Some("handler" | "entrypoint")
        ));
    }

    #[tokio::test]
    async fn search_workspace_session_refines_and_closes() {
        let base = test_workspace("search_workspace_session_refines_and_closes");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            "fn render_active_cell() {}\nfn render_live_status_line() {}\n",
        )
        .await
        .expect("write file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let first = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "mode": "text",
                    "query": "render_active",
                    "path_scope": "src",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("start search session");
        let session_id = match first.structured.as_ref() {
            Some(StructuredToolResult::SearchWorkspace { session_id, .. }) => session_id.clone(),
            other => panic!("expected search session id, got {other:?}"),
        };

        let refined = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "session_id": session_id,
                    "query": "render_live_status"
                })),
                &ctx,
            )
            .await
            .expect("refine search session");
        assert!(refined.content.contains("render_live_status_line"));

        let closed = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "session_id": match refined.structured.as_ref() {
                        Some(StructuredToolResult::SearchWorkspace {
                            session_id,
                            ..
                        }) => session_id.clone(),
                        other => panic!("expected search session id, got {other:?}"),
                    },
                    "operation": "close"
                })),
                &ctx,
            )
            .await
            .expect("close search session");
        assert!(closed.content.contains("Closed search session"));
    }

    #[tokio::test]
    async fn search_workspace_empty_session_id_starts_fresh_search() {
        let base = test_workspace("search_workspace_empty_session_id_starts_fresh_search");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "fn render_active_cell() {}\n")
            .await
            .expect("write file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "operation": "search",
                    "session_id": "",
                    "mode": "text",
                    "query": "render_active",
                    "path_scope": "src",
                    "max_results": 10
                })),
                &ctx,
            )
            .await
            .expect("empty session id should not refine");

        assert!(matches!(
            output.structured.as_ref(),
            Some(StructuredToolResult::SearchWorkspace {
                session_id,
                operation: SearchWorkspaceOperation::Search,
                query,
                ..
            }) if session_id == "search:test:1" && query == "render_active"
        ));
    }

    #[tokio::test]
    async fn search_workspace_refine_requires_runtime_session_id() {
        let base = test_workspace("search_workspace_refine_requires_runtime_session_id");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "fn render_active_cell() {}\n")
            .await
            .expect("write file");

        let tool = SearchWorkspaceLocalTool::new();
        let ctx = tool_context(&base);
        let err = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "operation": "refine",
                    "session_id": "search:test:99",
                    "query": "render_active"
                })),
                &ctx,
            )
            .await
            .expect_err("refine should require a known runtime session");

        assert!(
            err.to_string()
                .contains("search session `search:test:99` was not found")
        );
    }

    #[tokio::test]
    async fn read_file_reports_when_output_is_truncated() {
        let base = test_workspace("read_file_reports_when_output_is_truncated");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(
            base.join("src/lib.rs"),
            "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\n",
        )
        .await
        .expect("write file");

        let tool = ReadFileLocalTool {
            max_read_chars: 10_000,
            read_state: crate::impls::file_read_state::FileReadStateStore::new(),
        };
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                tool_invocation(serde_json::json!({
                    "path": "src/lib.rs",
                    "max_lines": 1
                })),
                &ctx,
            )
            .await
            .expect("read_file works");

        assert!(output.content.contains("Summary: Read 1 lines from"));
        assert!(output.content.contains("next_start_line"));
        assert!(
            output
                .content
                .contains("Next step: rerun `read_file` with `next_start_line: 2`")
        );
        assert!(matches!(
            output.structured.as_ref(),
            Some(StructuredToolResult::ReadFile {
                read,
                ..
            }) if read.truncated
                && read.next_start_line == Some(2)
                && read.returned_line_count == 1
                && read.total_line_count == Some(6)
        ));
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

    fn tool_context(workspace_root: &std::path::Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::ReadOnly,
            default_shell_timeout_ms: 5_000,
            max_tool_output_tokens:
                agent_core::ToolExecutionContext::default_max_tool_output_tokens(),
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn tool_invocation(arguments: serde_json::Value) -> LocalToolInvocation {
        LocalToolInvocation {
            identity: agent_core::ToolIdentity::built_in("test_tool"),
            source: LocalToolSource::BuiltIn,
            payload: LocalToolPayload::Function { arguments },
        }
    }
}
