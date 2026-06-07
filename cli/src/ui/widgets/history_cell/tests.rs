use super::{
    ExplorationAggregate, HistoryCell, HistoryFormat, HistoryTone, line_display_width,
    tool_aggregation, user_message_style,
};

fn joined(cell: &HistoryCell, width: usize) -> String {
    cell.to_lines_with_mode(width)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn joined_lines(lines: Vec<ratatui::text::Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn agent_cells_render_markdown_tables() {
    let cell = HistoryCell::agent(
        "cloudagent",
        "| 风险 | 根因 |\n| --- | --- |\n| budget | only log |",
        HistoryFormat::Markdown,
    );

    let rendered = joined(&cell, 100);
    assert!(rendered.contains("风险"));
    assert!(rendered.contains("根因"));
    assert!(rendered.contains(" | "));
    assert!(rendered.contains("budget"));
}

#[test]
fn agent_cells_render_without_shell_bullet_prefix() {
    let cell = HistoryCell::agent(
        "cloudagent",
        "### 也就是说\n\n逻辑已经改对了。\n\n1. 查锁\n2. 重跑",
        HistoryFormat::Markdown,
    );

    let rendered = joined(&cell, 100);
    let transcript = joined_lines(cell.to_transcript_lines(100));

    assert!(rendered.starts_with("  ### 也就是说"));
    assert!(transcript.starts_with("  ### 也就是说"));
    assert!(!rendered.contains("●"));
    assert!(!rendered.contains("• ###"));
    assert!(!rendered.contains("• 逻辑"));
    assert!(!transcript.contains("• ###"));
    assert!(!transcript.contains("• 逻辑"));
}

#[test]
fn agent_cells_keep_codex_style_left_padding_without_bullet() {
    let cell = HistoryCell::agent("cloudagent", "正文\n第二行", HistoryFormat::Markdown);

    let plain = cell
        .to_lines_with_mode(80)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(plain, vec!["  正文", "  第二行"]);
}

#[test]
fn plaintext_cells_do_not_get_markdown_table_rendering() {
    let cell = HistoryCell::agent("tool", "| raw | text |", HistoryFormat::PlainText);

    let rendered = joined(&cell, 100);
    assert!(rendered.contains("| raw | text |"));
}

#[test]
fn reasoning_cells_wrap_without_terminal_hard_break_artifacts() {
    let cell = HistoryCell::reasoning(
        "reasoning",
        "Now let me look at the collect_repo_entries function to understand how it handles paths, and also check if there's any path validation that rejects relative paths.",
    );

    let rendered = joined(&cell, 80);
    assert!(!rendered.contains("pat\n    │ h"));
    assert!(rendered.contains("path"));
}

#[test]
fn reasoning_multiline_paragraphs_keep_a_single_header() {
    let cell = HistoryCell::reasoning(
        "reasoning",
        "Now I have a clear picture.\n1. resolve_read_path allows absolute paths.\n2. resolve_full_access_path allows absolute paths.",
    );

    let rendered = joined(&cell, 100);
    assert_eq!(rendered.matches("≈ reasoning").count(), 1);
    assert!(rendered.contains("Now I have a clear picture."));
    assert!(rendered.contains("1. resolve_read_path"));
    assert!(rendered.contains("2."));
    assert!(rendered.contains("resolve_full_access_path"));
    assert!(!rendered.contains("\n\n"));
}

#[test]
fn reasoning_single_newlines_collapse_into_compact_paragraphs() {
    let cell = HistoryCell::reasoning(
        "reasoning",
        "方案：只修改 exec_command 的 workdir 处理逻辑。\n但 resolve_read_path 允许绝对路径。\n所以需要评估权限边界。",
    );

    let rendered = joined(&cell, 120);
    assert_eq!(rendered.matches("≈ reasoning").count(), 1);
    assert!(!rendered.contains("\n\n"));
    assert!(rendered.contains("方案：只修改 exec_command 的 workdir 处理逻辑。"));
    assert!(rendered.contains("但 resolve_read_path 允许绝对路径。"));
}

#[test]
fn user_cells_wrap_fully_without_intrinsic_truncation() {
    let cell = HistoryCell::user(
        "one two three four five six seven eight nine ten eleven twelve thirteen fourteen",
    );

    let rendered = joined(&cell, 14);
    assert!(rendered.contains("› one two"));
    assert!(rendered.contains("three four"));
    assert!(rendered.contains("thirteen"));
    assert!(!rendered.contains("... +"));
    assert!(!rendered.contains("… +"));
}

#[test]
fn user_cells_only_prefix_first_multiline_row() {
    let cell = HistoryCell::user("first line\nsecond line\nthird line");

    let rendered = cell.to_lines_with_mode(80);
    let plain = rendered
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();

    assert_eq!(plain, vec!["› first line", "  second line", "  third line"]);
}

#[test]
fn user_cells_apply_full_width_background() {
    let cell = HistoryCell::user("hello");

    let rendered = cell.to_lines_with_mode(24);
    let line = rendered.first().expect("user cell should render a line");

    assert_eq!(line.style.bg, user_message_style().bg);
    assert_eq!(line_display_width(line), 24);
    assert!(
        line.spans
            .iter()
            .all(|span| span.style.bg == user_message_style().bg)
    );
}

#[test]
fn exploration_cells_render_summary_with_nested_details() {
    let mut aggregate = ExplorationAggregate::new("file search `cli`".to_string());
    aggregate.searches = 8;
    aggregate.read_files = 10;
    aggregate.push_detail("text search `clipboard`".to_string());
    aggregate.push_detail("Read input_mapping.rs".to_string());
    aggregate.push_detail("Read textarea.rs".to_string());
    let cell = HistoryCell::exploration(
        "Explored workspace",
        "searched 8 times, read 10 files",
        aggregate,
        HistoryTone::Control,
    );

    let rendered = joined(&cell, 120);
    assert!(rendered.contains("Explored workspace"));
    assert!(rendered.contains("searched 8 times, read 10 files"));
    assert!(rendered.contains("└ file search `cli`"));
    assert!(rendered.contains("text search `clipboard`"));
}

#[test]
fn transcript_merges_adjacent_agent_stream_continuations() {
    let mut first = HistoryCell::agent("", "hello", HistoryFormat::Markdown);
    let second =
        HistoryCell::agent("", " world", HistoryFormat::Markdown).with_stream_continuation(true);

    assert!(tool_aggregation::coalesce_agent_stream(&mut first, &second));
    assert_eq!(first.body(), "hello world");
}

#[test]
fn transcript_does_not_merge_agent_cells_across_non_agent_boundaries() {
    let first = HistoryCell::agent("", "hello", HistoryFormat::Markdown);
    let barrier = HistoryCell::reasoning("Reasoning", "thinking");
    let second =
        HistoryCell::agent("", " world", HistoryFormat::Markdown).with_stream_continuation(true);

    let mut cells = Vec::new();
    for cell in [first, barrier, second] {
        if let Some(last) = cells.last_mut()
            && tool_aggregation::coalesce_agent_stream(last, &cell)
        {
            continue;
        }
        if let Some(last) = cells.last_mut()
            && tool_aggregation::coalesce_tool_like(last, &cell, true)
        {
            continue;
        }
        cells.push(cell);
    }

    assert_eq!(cells.len(), 3);
    assert_eq!(cells[0].body(), "hello");
    assert_eq!(cells[1].body(), "thinking");
    assert_eq!(cells[2].body(), " world");
}
