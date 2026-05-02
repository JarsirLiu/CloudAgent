mod common;
mod fs_read_file;
mod fuzzy_file_search;
mod text_read;

pub(crate) use fs_read_file::FsReadFileLocalTool;
pub use fs_read_file::FsReadFileTool;
pub(crate) use fuzzy_file_search::FuzzyFileSearchLocalTool;
pub use fuzzy_file_search::{FuzzyFileSearchArgs, FuzzyFileSearchTool};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::LocalTool;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn fuzzy_file_search_prefers_exact_file_name_matches() {
        let base = test_workspace("fuzzy_file_search_prefers_exact_file_name_matches");
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

        let tool = FuzzyFileSearchLocalTool;
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
                    "pattern": "service.rs",
                    "max_results": 10
                }),
                &ctx,
            )
            .await
            .expect("fuzzy file search works");

        let lines = output
            .content
            .lines()
            .map(str::trim)
            .map(ToOwned::to_owned)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(lines.get(0).map(String::as_str), Some("Top 2 matches:"));
        assert_eq!(lines.get(1).map(String::as_str), Some("src/service.rs"));
    }

    #[tokio::test]
    async fn fs_read_file_renders_line_numbers_and_truncation_notice() {
        let base = test_workspace("fs_read_file_renders_line_numbers");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "one\ntwo\nthree\nfour\n")
            .await
            .expect("write file");

        let tool = FsReadFileLocalTool {
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
            .expect("fs_read_file works");

        assert_eq!(output.content, "     2  two\n     3  three\n[truncated]");
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
