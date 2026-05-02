mod common;
mod find_files;
mod read_file;
mod read_files;
mod search_text;

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
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;

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
}
