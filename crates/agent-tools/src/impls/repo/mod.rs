mod common;
mod list_directory;
mod read_files;
mod search_workspace;
mod text_read;

pub(crate) use list_directory::ListDirectoryLocalTool;
pub use list_directory::ListDirectoryTool;
pub(crate) use read_files::ReadFilesLocalTool;
pub use read_files::ReadFilesTool;
pub(crate) use search_workspace::SearchWorkspaceLocalTool;
pub use search_workspace::SearchWorkspaceTool;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::LocalTool;
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
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            output_tx: None,
        };
        let output = tool
            .invoke(
                serde_json::json!({
                    "mode": "files",
                    "pattern": "service.rs",
                    "max_results": 10
                }),
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
            Some(agent_protocol::StructuredToolResult::SearchWorkspace {
                session_id,
                mode: agent_protocol::SearchWorkspaceMode::Files,
                query,
                ..
            }) if session_id == "search:test:1" && query == "service.rs"
        ));
        assert!(lines.iter().any(|line| line == "Top 2 matches (showing 2 of 2):"));
        assert!(lines.iter().any(|line| line == "src/service.rs"));
    }

    #[tokio::test]
    async fn read_files_supports_single_path_reads() {
        let base = test_workspace("read_files_supports_single_path_reads");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "one\ntwo\nthree\nfour\n")
            .await
            .expect("write file");

        let tool = ReadFilesLocalTool {
            max_read_chars: 10_000,
        };
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                serde_json::json!({
                    "path": "src/lib.rs",
                    "start_line": 2,
                    "max_lines": 2
                }),
                &ctx,
            )
            .await
            .expect("read_files works");

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
                serde_json::json!({
                    "mode": "text",
                    "query": "render_",
                    "path_scope": "src",
                    "max_results": 10
                }),
                &ctx,
            )
            .await
            .expect("search_workspace works");

        assert!(output.content.contains("Found 2 matches in 1 files"));
        assert!(output.content.contains("src/lib.rs:1: fn render_active_cell() {}"));
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
                serde_json::json!({
                    "mode": "text",
                    "query": "render_active",
                    "path_scope": "src",
                    "max_results": 10
                }),
                &ctx,
            )
            .await
            .expect("start search session");
        let session_id = match first.structured.as_ref() {
            Some(agent_protocol::StructuredToolResult::SearchWorkspace {
                session_id,
                ..
            }) => session_id.clone(),
            other => panic!("expected search session id, got {other:?}"),
        };

        let refined = tool
            .invoke(
                serde_json::json!({
                    "session_id": session_id,
                    "query": "render_live_status"
                }),
                &ctx,
            )
            .await
            .expect("refine search session");
        assert!(refined.content.contains("render_live_status_line"));

        let closed = tool
            .invoke(
                serde_json::json!({
                    "session_id": match refined.structured.as_ref() {
                        Some(agent_protocol::StructuredToolResult::SearchWorkspace {
                            session_id,
                            ..
                        }) => session_id.clone(),
                        other => panic!("expected search session id, got {other:?}"),
                    },
                    "operation": "close"
                }),
                &ctx,
            )
            .await
            .expect("close search session");
        assert!(closed.content.contains("Closed search session"));
    }

    #[tokio::test]
    async fn read_files_batches_multiple_paths() {
        let base = test_workspace("read_files_batches_multiple_paths");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/a.rs"), "alpha\n")
            .await
            .expect("write a");
        fs::write(base.join("src/b.rs"), "beta\n")
            .await
            .expect("write b");

        let tool = ReadFilesLocalTool {
            max_read_chars: 10_000,
        };
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                serde_json::json!({
                    "paths": ["src/a.rs", "src/b.rs"]
                }),
                &ctx,
            )
            .await
            .expect("read_files works");

        assert!(output.content.contains("a.rs"));
        assert!(output.content.contains("b.rs"));
        assert!(output.content.contains("alpha"));
        assert!(output.content.contains("beta"));
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
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            output_tx: None,
        }
    }
}
