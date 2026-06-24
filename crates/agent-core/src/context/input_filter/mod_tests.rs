use super::*;
use crate::text_input_items;
use crate::tool::StructuredToolResult;

fn run_filter(command: &str, output: Option<&str>) -> String {
    filter_command_execution_output(command, output)
}

#[test]
fn filter_disabled_keeps_original_message() {
    let svc = ContextInputFilterService::new();
    let input = vec![ResponseItem::User {
        content: text_input_items("hello"),
    }];
    let out = svc.filter_for_model(input.clone(), FilterPolicy { enabled: false });
    assert_eq!(out.len(), 1);
    match &out[0] {
        ResponseItem::User { content } => assert_eq!(content, &text_input_items("hello")),
        _ => panic!("expected user message"),
    }
}

#[test]
fn git_diff_is_summarized() {
    let structured = StructuredToolResult::CommandExecution {
        command: "git diff".to_string(),
        current_directory: "D:\\repo".to_string(),
        session_id: None,
        status: crate::tool::CommandExecutionStatus::Completed,
        exit_code: Some(0),
        success: Some(true),
        output: Some("+++ a\n--- b\n+line\n-line\n".to_string()),
        duration_ms: Some(1),
        original_token_count: Some(10),
        max_output_tokens: Some(10_000),
    };
    let svc = ContextInputFilterService::new();
    let out = svc.filter_for_model(
        vec![ResponseItem::Tool {
            tool_call_id: "1".to_string(),
            name: "exec_command".to_string(),
            content: "raw".to_string(),
            structured: Some(structured),
        }],
        FilterPolicy { enabled: true },
    );
    match &out[0] {
        ResponseItem::Tool { content, .. } => {
            assert!(content.contains("Git diff summary"));
            assert!(content.contains("files changed"));
        }
        _ => panic!("expected tool message"),
    }
}

#[test]
fn git_diff_truncates_to_key_hunks() {
    let raw = "diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n-old1\n-old2\n+new1\n+new2\n@@ -10,3 +10,3 @@\n-old3\n+new3\n@@ -20,3 +20,3 @@\n-old4\n+new4\ndiff --git a/src/b.rs b/src/b.rs\nindex 333..444 100644\n--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1,3 +1,3 @@\n-old5\n+new5";
    let out = run_filter("git diff", Some(raw));
    assert!(out.contains("Git diff summary"));
    assert!(out.contains("changed files:"));
    assert!(out.contains("src/a.rs"));
    assert!(out.contains("src/b.rs"));
}

#[test]
fn git_log_is_summarized() {
    let out = run_filter("git log --oneline", Some("commit abc123\ncommit def456"));
    assert!(out.contains("Git log summary"));
    assert!(out.contains("2 commits"));
}

#[test]
fn git_log_keeps_commit_subject_and_body_line() {
    let raw = "commit abc123\nAuthor: A <a@example.com>\nDate:   Tue Jun 24 21:00:00 2026 +0800\n\n    feat: add cache\n    preserve commit body first line\n\ncommit def456\nAuthor: B <b@example.com>\nDate:   Tue Jun 24 21:10:00 2026 +0800\n\n    fix: adjust diff summary";
    let out = run_filter("git log --format=full", Some(raw));
    assert!(out.contains("Git log summary"));
    assert!(out.contains("abc123"));
    assert!(out.contains("feat: add cache"));
    assert!(out.contains("preserve commit body first line"));
}

#[test]
fn cargo_test_is_summarized() {
    let out = run_filter("cargo test", Some("error: failed\nwarning: unused\nFAILED test_x"));
    assert!(out.contains("Cargo test summary"));
    assert!(out.contains("failures"));
}

#[test]
fn cargo_test_prefers_failure_block_details() {
    let raw = "running 3 tests\n---- tests::it_works stdout ----\nthread 'tests::it_works' panicked at 'boom'\nnote: run with `RUST_BACKTRACE=1`\nfailures:\n    tests::it_works\n\ntest result: FAILED. 2 passed; 1 failed; 0 ignored";
    let out = run_filter("cargo test", Some(raw));
    assert!(out.contains("Cargo test summary"));
    assert!(out.contains("panicked"));
    assert!(out.contains("failures:"));
}

#[test]
fn cargo_build_is_summarized() {
    let out = run_filter("cargo build", Some("error: failed\nwarning: unused"));
    assert!(out.contains("Cargo build summary"));
    assert!(out.contains("warnings"));
}

#[test]
fn cargo_build_keeps_error_and_warning_lines() {
    let out = run_filter("cargo build", Some("error: failed\nwarning: unused\nnote: help"));
    assert!(out.contains("Cargo build summary"));
    assert!(out.contains("error: failed"));
    assert!(out.contains("warning: unused"));
}

#[test]
fn cargo_test_uses_test_summary_header() {
    let out = run_filter("cargo test", Some("FAILED test_x"));
    assert!(out.starts_with("[rtk:cargo]"));
    assert!(out.contains("Cargo test summary"));
}

#[test]
fn cargo_build_uses_build_summary_header() {
    let out = run_filter("cargo build", Some("warning: unused"));
    assert!(out.starts_with("[rtk:cargo]"));
    assert!(out.contains("Cargo build summary"));
}

#[test]
fn cargo_clippy_uses_clippy_summary() {
    let out = run_filter("cargo clippy", Some("warning: clippy::"));
    assert!(out.contains("Cargo clippy summary"));
}

#[test]
fn cargo_fmt_uses_fmt_summary() {
    let out = run_filter("cargo fmt", Some("Formatted src/lib.rs"));
    assert!(out.contains("Cargo fmt summary"));
}

#[test]
fn cargo_fmt_keeps_useful_lines() {
    let raw = "Running rustfmt\nFormatted src/lib.rs\nwarning: foo\nerror: bar";
    let out = run_filter("cargo fmt", Some(raw));
    assert!(out.contains("Cargo fmt summary"));
    assert!(out.contains("Formatted src/lib.rs"));
    assert!(out.contains("warning: foo"));
}

#[test]
fn install_output_is_compacted() {
    let text = "Downloading x\nadded 10 packages\naudited 10 packages\n";
    let compacted = filter_install_output(text);
    assert!(compacted.contains("Install summary"));
    assert!(compacted.contains("added 10 packages"));
}

#[test]
fn passthrough_verbose_command() {
    let out = run_filter("cargo test -- --nocapture", Some("full output"));
    assert_eq!(out, "full output");
}

#[test]
fn generic_output_has_stable_header() {
    let out = run_filter("echo hi", Some("line1\nline2"));
    assert!(out.starts_with("[rtk:generic]"));
}

#[test]
fn python_module_pytest_uses_test_header() {
    let raw = "PASSED t1\nFAILED t2";
    let out = run_filter("python -m pytest -q", Some(raw));
    assert!(out.starts_with("[rtk:test]"));
    assert!(out.contains("Test summary"));
}

#[test]
fn cargo_install_uses_install_header() {
    let out = run_filter("cargo install ripgrep", Some("installed ripgrep"));
    assert!(out.starts_with("[rtk:install]"));
}

#[test]
fn cargo_install_summarizes_install_output() {
    let out = run_filter("cargo install ripgrep", Some("finished\ninstalled ripgrep\nsuccess"));
    assert!(out.contains("Install summary"));
}

#[test]
fn git_status_uses_git_header() {
    let raw = "modified: a.rs\nnew file: b.rs";
    let out = run_filter("git status", Some(raw));
    assert!(out.starts_with("[rtk:git]"));
    assert!(out.contains("changed files"));
}

#[test]
fn test_runner_uses_test_header() {
    let raw = "PASSED t1\nFAILED t2";
    let out = run_filter("pytest -q", Some(raw));
    assert!(out.starts_with("[rtk:test]"));
    assert!(out.contains("Test summary"));
}

#[test]
fn python_pytest_is_compressed() {
    let raw = "=========================== test session starts ===========================\nPASSED t1\nFAILED t2\nSKIPPED t3\nERROR t4";
    let out = run_filter("python -m pytest -q", Some(raw));
    assert!(out.starts_with("[rtk:test]"));
    assert!(out.contains("Test summary"));
    assert!(out.contains("passed"));
    assert!(out.contains("failed"));
}

#[test]
fn python_pip_install_is_compressed() {
    let raw = "Collecting foo\nDownloading foo\nSuccessfully installed foo-1.0.0\nWARNING: running pip as root";
    let out = run_filter("pip install foo", Some(raw));
    assert!(out.starts_with("[rtk:python]"));
    assert!(out.contains("Python pip install summary"));
    assert!(out.contains("Successfully installed"));
}

#[test]
fn older_duplicate_read_file_is_compressed_but_latest_stays_raw() {
    let svc = ContextInputFilterService::new();
    let messages = vec![
        ResponseItem::Tool {
            tool_call_id: "1".to_string(),
            name: "read_file".to_string(),
            content: "old file body".to_string(),
            structured: Some(StructuredToolResult::ReadFile {
                path: "src/app.rs".to_string(),
                start_line: None,
                max_lines: None,
                total_chars: 120,
                read: crate::tool::ReadFileEntry {
                    path: "src/app.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    next_start_line: None,
                    returned_line_count: 0,
                    total_line_count: None,
                    returned_char_count: 0,
                    truncated: false,
                    char_count: 120,
                    status: crate::tool::ReadFileStatus::Ok,
                    version_token: None,
                },
            }),
        },
        ResponseItem::Tool {
            tool_call_id: "2".to_string(),
            name: "read_file".to_string(),
            content: "new file body".to_string(),
            structured: Some(StructuredToolResult::ReadFile {
                path: "src/app.rs".to_string(),
                start_line: None,
                max_lines: None,
                total_chars: 160,
                read: crate::tool::ReadFileEntry {
                    path: "src/app.rs".to_string(),
                    start_line: None,
                    end_line: None,
                    next_start_line: None,
                    returned_line_count: 0,
                    total_line_count: None,
                    returned_char_count: 0,
                    truncated: false,
                    char_count: 160,
                    status: crate::tool::ReadFileStatus::Ok,
                    version_token: None,
                },
            }),
        },
    ];

    let out = svc.filter_for_model(messages, FilterPolicy { enabled: true });
    match &out[0] {
        ResponseItem::Tool { content, .. } => {
            assert!(content.starts_with("[rtk:read_file]"));
            assert!(content.contains("superseded by a newer read"));
        }
        _ => panic!("expected tool message"),
    }
    match &out[1] {
        ResponseItem::Tool { content, .. } => assert_eq!(content, "new file body"),
        _ => panic!("expected tool message"),
    }
}

#[test]
fn older_duplicate_find_files_is_compressed_only_for_same_query() {
    let svc = ContextInputFilterService::new();
    let messages = vec![
        ResponseItem::Tool {
            tool_call_id: "1".to_string(),
            name: "search_workspace".to_string(),
            content: "Top 2 matches:\nsrc/app.rs\nsrc/lib.rs".to_string(),
            structured: Some(StructuredToolResult::SearchWorkspace {
                session_id: "search:test:1".to_string(),
                operation: crate::tool::SearchWorkspaceOperation::Search,
                mode: crate::tool::SearchWorkspaceMode::Files,
                status: crate::tool::SearchWorkspaceStatus::Active,
                query: "app".to_string(),
                path_scope: Some("src".to_string()),
                case_sensitive: false,
                context_lines: 0,
                offset: 0,
                max_results: 20,
                file_count: 2,
                match_count: 0,
                truncated: false,
                next_offset: None,
                hits: Vec::new(),
            }),
        },
        ResponseItem::Tool {
            tool_call_id: "2".to_string(),
            name: "search_workspace".to_string(),
            content: "Top 3 matches:\nsrc/app.rs\nsrc/lib.rs\nsrc/main.rs".to_string(),
            structured: Some(StructuredToolResult::SearchWorkspace {
                session_id: "search:test:1".to_string(),
                operation: crate::tool::SearchWorkspaceOperation::Search,
                mode: crate::tool::SearchWorkspaceMode::Files,
                status: crate::tool::SearchWorkspaceStatus::Active,
                query: "app".to_string(),
                path_scope: Some("src".to_string()),
                case_sensitive: false,
                context_lines: 0,
                offset: 0,
                max_results: 20,
                file_count: 3,
                match_count: 0,
                truncated: false,
                next_offset: None,
                hits: Vec::new(),
            }),
        },
        ResponseItem::Tool {
            tool_call_id: "3".to_string(),
            name: "search_workspace".to_string(),
            content: "Top 1 matches:\nREADME.md".to_string(),
            structured: Some(StructuredToolResult::SearchWorkspace {
                session_id: "search:test:2".to_string(),
                operation: crate::tool::SearchWorkspaceOperation::Search,
                mode: crate::tool::SearchWorkspaceMode::Files,
                status: crate::tool::SearchWorkspaceStatus::Active,
                query: "readme".to_string(),
                path_scope: None,
                case_sensitive: false,
                context_lines: 0,
                offset: 0,
                max_results: 20,
                file_count: 1,
                match_count: 0,
                truncated: false,
                next_offset: None,
                hits: Vec::new(),
            }),
        },
    ];

    let out = svc.filter_for_model(messages, FilterPolicy { enabled: true });
    match &out[0] {
        ResponseItem::Tool { content, .. } => {
            assert!(content.starts_with("[rtk:search_workspace]"));
            assert!(content.contains("query: app"));
            assert!(content.contains("superseded by newer search results"));
        }
        _ => panic!("expected tool message"),
    }
    match &out[1] {
        ResponseItem::Tool { content, .. } => assert!(content.starts_with("Top 3 matches:")),
        _ => panic!("expected tool message"),
    }
    match &out[2] {
        ResponseItem::Tool { content, .. } => assert!(content.starts_with("Top 1 matches:")),
        _ => panic!("expected tool message"),
    }
}
