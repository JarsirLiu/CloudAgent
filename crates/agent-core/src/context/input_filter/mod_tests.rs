use super::*;
use crate::text_input_items;
use crate::tool::StructuredToolResult;

fn run_filter(command: &str, output: Option<&str>, success: Option<bool>) -> String {
    filter_command_execution_output(command, output, success)
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
        ResponseItem::Tool { content, .. } => assert!(content.contains("Git diff summary")),
        _ => panic!("expected tool message"),
    }
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
    let out = run_filter("cargo test -- --nocapture", Some("full output"), None);
    assert_eq!(out, "full output");
}

#[test]
fn generic_output_has_stable_header() {
    let out = run_filter("echo hi", Some("line1\nline2"), None);
    assert!(out.starts_with("[rtk:generic]"));
}

#[test]
fn failed_command_keeps_tail_with_header() {
    let out = run_filter("unknown", Some("a\nb\nc"), Some(false));
    assert!(out.starts_with("[rtk:fallback]"));
    assert!(out.contains("c"));
}

#[test]
fn git_status_uses_git_header() {
    let raw = "modified: a.rs\nnew file: b.rs";
    let out = run_filter("git status", Some(raw), None);
    assert!(out.starts_with("[rtk:git]"));
    assert!(out.contains("changed files"));
}

#[test]
fn test_runner_uses_test_header() {
    let raw = "PASSED t1\nFAILED t2";
    let out = run_filter("pytest -q", Some(raw), None);
    assert!(out.starts_with("[rtk:test]"));
    assert!(out.contains("Test summary"));
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
