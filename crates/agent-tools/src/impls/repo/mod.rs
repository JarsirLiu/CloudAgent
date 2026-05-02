mod common;
mod find_files;
mod read_file;
mod read_files;
mod search_text;
mod text_read;

pub(crate) use find_files::FindFilesLocalTool;
pub use find_files::{FindFilesArgs, FindFilesTool};
pub(crate) use read_file::ReadFileLocalTool;
pub use read_file::ReadFileTool;
pub(crate) use read_files::ReadFilesLocalTool;
pub use read_files::{ReadFilesArgs, ReadFilesTool};
pub(crate) use search_text::SearchTextLocalTool;
pub use search_text::{SearchTextArgs, SearchTextOutput, SearchTextTool, run_search_text};

#[cfg(test)]
mod tests {
    use super::common::rank_file_match;
    use super::*;
    use crate::registry::shared::LocalTool;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn search_text_skips_ignored_dirs() {
        let base = test_workspace("search_text_skips_ignored_dirs");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::create_dir_all(base.join("node_modules"))
            .await
            .expect("create node_modules");
        fs::write(base.join("src/main.rs"), "let token = 1;\n")
            .await
            .expect("write src");
        fs::write(base.join("node_modules/bad.js"), "token token token\n")
            .await
            .expect("write ignored");

        let output = run_search_text(
            &base,
            SearchTextArgs {
                query: "token".to_string(),
                path_scope: None,
                max_results: Some(10),
                case_sensitive: None,
            },
        )
        .await
        .expect("search works");

        assert_eq!(output.match_count, 1);
        assert_eq!(output.file_count, 1);
        assert!(output.results[0].path.contains("src/main.rs"));
    }

    #[tokio::test]
    async fn find_files_prefers_exact_file_name_matches() {
        let base = test_workspace("find_files_prefers_exact_file_name_matches");
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

        let tool = FindFilesLocalTool;
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
            .expect("find files works");

        let lines = output
            .content
            .lines()
            .map(str::trim)
            .map(ToOwned::to_owned)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(lines.first().map(String::as_str), Some("src/service.rs"));
    }

    #[test]
    fn fuzzy_rank_prefers_name_subsequence_over_distant_path_noise() {
        let direct = rank_file_match("src/service.rs", "service.rs", "servrs");
        let noisy = rank_file_match("src/server_controller.rs", "server_controller.rs", "servrs");

        assert!(direct.is_some());
        assert!(noisy.is_none() || direct > noisy);
    }

    #[tokio::test]
    async fn read_file_renders_line_numbers_and_truncation_notice() {
        let base = test_workspace("read_file_renders_line_numbers");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/lib.rs"), "one\ntwo\nthree\nfour\n")
            .await
            .expect("write file");

        let tool = ReadFileLocalTool {
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
            .expect("read file works");

        assert_eq!(output.content, "     2  two\n     3  three\n[truncated]");
    }

    #[tokio::test]
    async fn read_files_handles_binary_files_without_failing_batch() {
        let base = test_workspace("read_files_handles_binary_files");
        fs::create_dir_all(base.join("src"))
            .await
            .expect("create src");
        fs::write(base.join("src/main.rs"), "fn main() {}\n")
            .await
            .expect("write text file");
        fs::write(base.join("src/logo.bin"), [0_u8, 1_u8, 2_u8, 3_u8])
            .await
            .expect("write binary file");

        let tool = ReadFilesLocalTool {
            max_read_chars: 10_000,
        };
        let ctx = tool_context(&base);
        let output = tool
            .invoke(
                serde_json::json!({
                    "paths": ["src/main.rs", "src/logo.bin"]
                }),
                &ctx,
            )
            .await
            .expect("read files works");

        assert!(output.content.contains("== src/main.rs =="));
        assert!(output.content.contains("     1  fn main() {}"));
        assert!(output.content.contains("== src/logo.bin =="));
        assert!(output.content.contains("[binary file omitted]"));
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
