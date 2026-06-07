
use super::{render_markdown, render_plaintext};
use ratatui::style::Color;
use ratatui::style::Modifier;

fn joined(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tables_keep_column_separators() {
    let input = "\
| 风险 | 根因 | 优先级 |\n\
| --- | --- | --- |\n\
| final budget exceeded | 只 log 不 block | 高 |\n\
| token estimator | len vs chars | 中 |";

    let rendered = render_markdown(input, 120);
    let text = joined(&rendered);

    assert!(text.contains("风险"));
    assert!(text.contains(" | "));
    assert!(text.contains("只 log 不 block"));
}

#[test]
fn plaintext_preserves_line_breaks() {
    let rendered = render_plaintext("first line\nsecond line", 40);
    let text = joined(&rendered);

    assert_eq!(text, "first line\nsecond line");
}

#[test]
fn tables_wrap_long_cells_inside_columns() {
    let input = "\
| section | detail |\n\
| --- | --- |\n\
| command | this is a very long detail cell that should wrap inside the table column |";

    let rendered = render_markdown(input, 44);
    let text = joined(&rendered);

    assert!(text.contains("section"));
    assert!(text.contains("detail"));
    assert!(text.contains("command"));
    assert!(text.contains(" | "));
    assert!(text.lines().count() > 3);
}

#[test]
fn heading_markers_use_heading_style() {
    let rendered = render_markdown("## Summary\n\nBody", 80);
    let heading = rendered.first().expect("heading line");

    assert_eq!(heading.spans[0].content.as_ref(), "## ");
    assert!(heading.spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(heading.spans[1].content.as_ref(), "Summary");
    assert!(heading.spans[1].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn fenced_code_blocks_do_not_fill_the_line_background() {
    let rendered = render_markdown("```text\nlet value = 1;\n```", 80);
    let code = rendered.first().expect("code line");
    let text = code
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text, "let value = 1;");
    assert_eq!(code.style.bg, None);
    assert!(
        code.spans
            .iter()
            .all(|span| span.style.bg != Some(Color::Rgb(25, 28, 35)))
    );
}

#[test]
fn fenced_code_blocks_without_language_are_not_indented_as_indented_code() {
    let rendered = render_markdown("```\nplain\n```", 80);
    let code = rendered.first().expect("code line");
    let text = code
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text, "plain");
}

#[test]
fn code_blocks_keep_content_on_single_line() {
    let rendered = render_markdown("Intro\n\n    abcdefghijklmnopqrstuvwxyz", 12);
    let text = joined(&rendered);

    assert_eq!(text, "Intro\n\n    abcdefghijklmnopqrstuvwxyz");
}

#[test]
fn paragraphs_render_with_single_blank_separator_row() {
    let rendered = render_markdown("First paragraph.\n\nSecond paragraph.", 80);
    let text = joined(&rendered);

    assert_eq!(text, "First paragraph.\n\nSecond paragraph.");
}

#[test]
fn source_blank_lines_before_lists_are_preserved() {
    let rendered = render_markdown("Intro\n\n- one\n- two\n\nNext", 80);
    let text = joined(&rendered);

    assert_eq!(text, "Intro\n\n- one\n- two\n\nNext");
}

#[test]
fn fenced_code_blocks_keep_full_line_content() {
    let rendered = render_markdown("```text\nabcdefghijklmnop\n```", 10);
    let text = joined(&rendered);

    assert_eq!(text, "abcdefghijklmnop");
}

#[test]
fn renders_codex_style_common_markdown_blocks() {
    let input = concat!(
        "Intro with `code`, [docs](https://example.com), and escaped \\*literal\\*.\n",
        "\n",
        "> quoted\n",
        "> - item\n",
        "\n",
        "- outer\n",
        "    - inner\n",
        "- [x] done\n",
        "- [ ] todo\n",
        "\n",
        "---\n",
        "\n",
        "1. one\n",
        "2. two",
    );
    let rendered = render_markdown(input, 80);
    let text = joined(&rendered);

    assert!(text.contains("Intro with code, docs (https://example.com), and escaped *literal*."));
    assert!(text.contains("> quoted"));
    assert!(text.contains("> - item"));
    assert!(text.contains("- outer"));
    assert!(text.contains("    - inner"), "{text}");
    assert!(text.contains("- [x] done"));
    assert!(text.contains("- [ ] todo"));
    assert!(text.contains("———"));
    assert!(text.contains("1. one"));
    assert!(text.contains("2. two"));
}

#[test]
fn blockquote_in_ordered_list_on_next_line_is_inline() {
    let rendered = render_markdown("1.\n   > quoted\n", 80);
    let text = joined(&rendered);

    assert_eq!(text, "1. > quoted");
}

#[test]
fn blockquote_inside_nested_list_keeps_nested_prefixes() {
    let rendered = render_markdown("1. A\n    - B\n      > inner\n", 80);
    let text = joined(&rendered);

    assert_eq!(text, "1. A\n    - B\n      > inner");
}

#[test]
fn list_item_text_then_blockquote_stays_on_new_line() {
    let rendered = render_markdown("1. before\n   > quoted\n", 80);
    let text = joined(&rendered);

    assert_eq!(text, "1. before\n   > quoted");
}

#[test]
fn blockquote_with_heading_and_paragraph_preserves_blank_line() {
    let rendered = render_markdown("> # Heading\n> paragraph text\n", 80);
    let text = joined(&rendered);

    assert_eq!(text, "> # Heading\n> \n> paragraph text");
}

#[test]
fn inline_code_uses_text_only_without_background_padding() {
    let rendered = render_markdown("Use `cargo test` now.", 80);
    let line = rendered.first().expect("line");
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text, "Use cargo test now.");
    assert!(
        line.spans
            .iter()
            .any(|span| span.style.fg == Some(Color::Cyan)),
        "{line:?}"
    );
    assert!(line.spans.iter().all(|span| span.style.bg.is_none()));
}

#[test]
fn source_list_indent_repair_preserves_nested_marker_columns() {
    let repaired = super::repair_source_list_indents(
        vec![ratatui::text::Line::from("- inner")],
        "    - inner",
    );

    assert_eq!(joined(&repaired), "    - inner");
}

#[test]
fn nested_list_source_indent_survives_render_markdown() {
    let rendered = render_markdown("- outer\n    - inner", 80);
    assert_eq!(joined(&rendered), "- outer\n    - inner");
}

#[test]
fn nested_list_source_indent_repair_works_with_surrounding_markdown() {
    let source = "Intro\n\n- outer\n    - inner\n\n---";
    let repaired =
        super::repair_source_list_indents(vec![ratatui::text::Line::from("- inner")], source);
    assert_eq!(joined(&repaired), "    - inner");
}
