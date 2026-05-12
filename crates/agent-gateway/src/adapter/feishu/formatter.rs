use serde_json::{Value, json};

const TEXT_CHUNK_LIMIT: usize = 3800;
const POST_CHUNK_LIMIT: usize = 3200;

pub struct FormattedOutboundChunk {
    pub msg_type: &'static str,
    pub content: Value,
    pub preview_text: String,
}

pub fn format_text_chunks(text: &str, is_group_context: bool) -> Vec<FormattedOutboundChunk> {
    let normalized = text.replace("\r\n", "\n");
    if prefers_text_mode(&normalized, is_group_context) {
        let mut plain_text = if contains_markdown_table(&normalized) {
            strip_markdown_to_plain_text(&convert_markdown_tables_to_plain_text(&normalized))
        } else {
            strip_markdown_to_plain_text(&normalized)
        };
        if is_group_context {
            plain_text = optimize_group_plain_text(&plain_text);
        }
        return split_plain_text(&plain_text)
            .into_iter()
            .map(|chunk| FormattedOutboundChunk {
                msg_type: "text",
                content: json!({ "text": chunk.clone() }),
                preview_text: preview(&chunk, 120),
            })
            .collect();
    }

    if prefers_post_mode(&normalized, is_group_context) {
        let post_markdown = normalize_markdown_for_post(&normalized);
        return split_post_chunks(&post_markdown)
            .into_iter()
            .map(|chunk| FormattedOutboundChunk {
                msg_type: "post",
                content: build_markdown_post_content(&chunk),
                preview_text: preview(&chunk, 120),
            })
            .collect();
    }

    split_plain_text(&normalized)
        .into_iter()
        .map(|chunk| FormattedOutboundChunk {
            msg_type: "text",
            content: json!({ "text": chunk.clone() }),
            preview_text: preview(&chunk, 120),
        })
        .collect()
}

fn prefers_post_mode(text: &str, is_group_context: bool) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || contains_markdown_table(trimmed) {
        return false;
    }

    if is_group_context && prefers_group_text_mode(trimmed) {
        return false;
    }

    trimmed.contains("```")
        || trimmed.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("#")
                || line.starts_with("> ")
                || line.starts_with("- ")
                || line.starts_with("* ")
                || has_ordered_list_prefix(line)
                || line.contains("**")
                || line.contains("`")
        })
        || (trimmed.contains('[') && trimmed.contains("]("))
}

fn prefers_text_mode(text: &str, is_group_context: bool) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty()
        || contains_markdown_table(trimmed)
        || prefers_plain_text_fallback(trimmed)
        || (is_group_context && prefers_group_text_mode(trimmed))
}

fn prefers_group_text_mode(text: &str) -> bool {
    let line_count = text.lines().count();
    let has_numbered_section = text.lines().any(is_numbered_section_line);
    let has_bulleted_summary = text
        .lines()
        .any(|line| split_bulleted_summary_line(line.trim()).is_some());
    let heading_count = text
        .lines()
        .filter(|line| heading_prefix_len(line.trim_start()).is_some())
        .count();
    let list_count = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- ") || line.starts_with("* ") || has_ordered_list_prefix(line)
        })
        .count();
    let code_fence_count = text.matches("```").count();
    let link_count = text.matches("](").count();

    text.chars().count() >= 260
        || line_count >= 8
        || heading_count >= 2
        || list_count >= 4
        || code_fence_count >= 2
        || link_count >= 2
        || (heading_count >= 1 && line_count >= 3)
        || has_numbered_section
        || has_bulleted_summary
}

fn contains_markdown_table(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    for window in lines.windows(2) {
        let first = window[0].trim();
        let second = window[1].trim();
        if first.starts_with('|')
            && first.ends_with('|')
            && second.starts_with('|')
            && second.ends_with('|')
            && second.contains('-')
        {
            return true;
        }
    }
    false
}

fn convert_markdown_tables_to_plain_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        if idx + 1 < lines.len() && is_markdown_table_header(lines[idx], lines[idx + 1]) {
            let headers = parse_table_row(lines[idx]);
            idx += 2;
            while idx < lines.len() {
                let line = lines[idx].trim();
                if !line.starts_with('|') || !line.ends_with('|') {
                    break;
                }
                let cells = parse_table_row(lines[idx]);
                if cells.is_empty() {
                    idx += 1;
                    continue;
                }
                out.push(format_table_row(&headers, &cells));
                idx += 1;
            }
            if idx < lines.len() && !lines[idx].trim().is_empty() {
                out.push(String::new());
            }
            continue;
        }

        out.push(lines[idx].to_string());
        idx += 1;
    }

    out.join("\n").trim().to_string()
}

fn is_markdown_table_header(header: &str, separator: &str) -> bool {
    let header = header.trim();
    let separator = separator.trim();
    header.starts_with('|')
        && header.ends_with('|')
        && separator.starts_with('|')
        && separator.ends_with('|')
        && separator
            .trim_matches('|')
            .split('|')
            .all(|part| part.trim().chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn parse_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().trim_matches('`').to_string())
        .collect()
}

fn format_table_row(headers: &[String], cells: &[String]) -> String {
    if headers.is_empty() {
        return cells.join(" | ");
    }
    let mut parts = Vec::new();
    for (idx, cell) in cells.iter().enumerate() {
        let header = headers.get(idx).map(String::as_str).unwrap_or("");
        if header.is_empty() {
            parts.push(cell.to_string());
        } else {
            parts.push(format!("{header}: {cell}"));
        }
    }
    format!("- {}", parts.join("，"))
}

fn has_ordered_list_prefix(line: &str) -> bool {
    let mut seen_digit = false;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            continue;
        }
        return seen_digit && ch == '.';
    }
    false
}

fn is_heading_like_line(line: &str) -> bool {
    let char_count = line.chars().count();
    char_count <= 20
        && !line.starts_with('•')
        && !has_ordered_list_prefix(line)
        && !line.contains('：')
        && !line.contains(':')
}

fn trim_heading_punctuation(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '：' | ':' | '-' | '—'))
}

fn split_summary_line(line: &str) -> Option<(&str, &str)> {
    if let Some((label, value)) = line.split_once('：')
        && label.chars().count() <= 14
    {
        return Some((label.trim(), value.trim()));
    }
    if let Some((label, value)) = line.split_once(':')
        && label.chars().count() <= 18
    {
        return Some((label.trim(), value.trim()));
    }
    None
}

fn split_bulleted_summary_line(line: &str) -> Option<(String, String)> {
    let (prefix, remainder) = if let Some(rest) = line.strip_prefix("• ") {
        ("• ", rest)
    } else {
        return None;
    };

    let (label, value) = split_summary_line(remainder)?;
    if value.is_empty() {
        return None;
    }

    Some((format!("{prefix}{label}"), value.to_string()))
}

fn is_numbered_section_line(line: &str) -> bool {
    let trimmed = line.trim();
    let char_count = trimmed.chars().count();
    if char_count == 0 || char_count > 80 {
        return false;
    }

    let starts_with_numeric_prefix = trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || trimmed.starts_with("1️⃣")
        || trimmed.starts_with("2️⃣")
        || trimmed.starts_with("3️⃣")
        || trimmed.starts_with("4️⃣")
        || trimmed.starts_with("5️⃣")
        || trimmed.starts_with("6️⃣")
        || trimmed.starts_with("7️⃣")
        || trimmed.starts_with("8️⃣")
        || trimmed.starts_with("9️⃣")
        || trimmed.starts_with("🔟");

    starts_with_numeric_prefix
        && (trimmed.contains('—')
            || trimmed.contains('-')
            || trimmed.contains('：')
            || trimmed.contains(':'))
}

fn split_plain_text(text: &str) -> Vec<String> {
    split_by_limit(text, TEXT_CHUNK_LIMIT)
}

fn optimize_group_plain_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            if !previous_blank {
                lines.push(String::new());
            }
            previous_blank = true;
            continue;
        }

        previous_blank = false;
        let normalized = if is_heading_like_line(trimmed) || is_numbered_section_line(trimmed) {
            format!("【{}】", trim_heading_punctuation(trimmed))
        } else if let Some((label, value)) = split_bulleted_summary_line(trimmed) {
            format!("{label}\n  {value}")
        } else if let Some((label, value)) = split_summary_line(trimmed) {
            if value.is_empty() {
                label.to_string()
            } else {
                format!("{label}\n{value}")
            }
        } else {
            trimmed.to_string()
        };
        lines.push(normalized);
    }

    collapse_blank_lines(lines.join("\n").trim())
}

fn split_post_chunks(text: &str) -> Vec<String> {
    split_markdown_by_limit(text, POST_CHUNK_LIMIT)
}

fn split_by_limit(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if count >= max_chars {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
        current.push(ch);
        count += 1;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_markdown_by_limit(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let candidate = if current.is_empty() {
            line.to_string()
        } else {
            format!("{current}\n{line}")
        };
        if candidate.chars().count() > max_chars && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current.push_str(line);
        } else if candidate.chars().count() > max_chars {
            chunks.extend(split_by_limit(line, max_chars));
            current.clear();
        } else {
            current = candidate;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

fn build_markdown_post_content(text: &str) -> Value {
    let rows = build_markdown_post_rows(text);
    json!({
        "zh_cn": {
            "content": rows
        }
    })
}

fn prefers_plain_text_fallback(text: &str) -> bool {
    let code_block_count = text.matches("```").count();
    let quote_line_count = text
        .lines()
        .filter(|line| line.trim_start().starts_with("> "))
        .count();
    let heading_count = text
        .lines()
        .filter(|line| heading_prefix_len(line.trim_start()).is_some())
        .count();
    let list_line_count = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- ") || line.starts_with("* ") || has_ordered_list_prefix(line)
        })
        .count();
    code_block_count >= 4
        || quote_line_count >= 5
        || heading_count >= 8
        || list_line_count >= 8
        || (heading_count >= 2 && list_line_count >= 4)
        || (text.chars().count() >= 900 && (heading_count >= 2 || list_line_count >= 6))
}

fn strip_markdown_to_plain_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n");
    let linked = replace_markdown_links(&normalized);
    let mut out = Vec::new();
    let mut in_code_block = false;

    for raw_line in linked.lines() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            out.push(raw_line.to_string());
            continue;
        }

        let line = strip_markdown_line(raw_line);
        out.push(line);
    }

    collapse_blank_lines(out.join("\n").trim())
}

fn replace_markdown_links(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] == '['
            && let Some(close_bracket) = chars[idx + 1..].iter().position(|ch| *ch == ']')
        {
            let close_bracket = idx + 1 + close_bracket;
            if close_bracket + 1 < chars.len()
                && chars[close_bracket + 1] == '('
                && let Some(close_paren) =
                    chars[close_bracket + 2..].iter().position(|ch| *ch == ')')
            {
                let close_paren = close_bracket + 2 + close_paren;
                let label: String = chars[idx + 1..close_bracket].iter().collect();
                let url: String = chars[close_bracket + 2..close_paren].iter().collect();
                out.push_str(label.trim());
                if !url.trim().is_empty() {
                    out.push_str(" (");
                    out.push_str(url.trim());
                    out.push(')');
                }
                idx = close_paren + 1;
                continue;
            }
        }

        out.push(chars[idx]);
        idx += 1;
    }

    out
}

fn strip_markdown_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len().saturating_sub(trimmed.len());
    let indent = &line[..indent_len];

    let mut content = trimmed.to_string();
    if let Some(prefix_len) = heading_prefix_len(&content) {
        content = content[prefix_len..].trim_start().to_string();
    }
    if content.starts_with("> ") {
        content = content[2..].to_string();
    }
    if content
        .chars()
        .all(|ch| ch == '-' || ch == ':' || ch.is_whitespace())
    {
        return String::new();
    }

    let normalized = if is_unordered_list(&content) {
        let stripped = strip_list_prefix(&content);
        format!("• {}", strip_inline_markdown(stripped))
    } else if let Some(prefix) = ordered_list_prefix(&content) {
        let stripped = strip_list_prefix(&content);
        format!("{prefix} {}", strip_inline_markdown(stripped))
    } else {
        strip_inline_markdown(&content)
    };
    format!("{indent}{normalized}").trim_end().to_string()
}

fn strip_list_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return rest;
    }

    let mut digits = 0usize;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        if digits > 0 && ch == '.' {
            let remainder = &trimmed[digits + 1..];
            return remainder.trim_start();
        }
        break;
    }

    trimmed
}

fn is_unordered_list(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("- ") || trimmed.starts_with("* ")
}

fn heading_prefix_len(line: &str) -> Option<usize> {
    let mut count = 0usize;
    for ch in line.chars() {
        if ch == '#' {
            count += 1;
            continue;
        }
        break;
    }
    if count == 0 || count > 6 {
        return None;
    }
    Some(count)
}

fn ordered_list_prefix(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let mut digits = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if !digits.is_empty() && ch == '.' {
            return Some(format!("{digits}."));
        }
        break;
    }
    None
}

fn strip_inline_markdown(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace('`', "")
        .replace("<u>", "")
        .replace("</u>", "")
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut previous_blank = false;
    for line in text.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && previous_blank {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line.trim_end());
        previous_blank = is_blank;
    }
    out
}

fn normalize_markdown_for_post(text: &str) -> String {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        out.push(normalize_post_line(raw_line));
    }
    out.join("\n")
}

fn normalize_post_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len().saturating_sub(trimmed.len());
    let indent = &line[..indent_len];

    if let Some(prefix_len) = heading_prefix_len(trimmed) {
        let body = trimmed[prefix_len..].trim_start();
        if body.is_empty() {
            return String::new();
        }
        return format!("{indent}**{}**", body.trim());
    }

    line.to_string()
}

fn build_markdown_post_rows(text: &str) -> Vec<Vec<Value>> {
    if text.is_empty() {
        return vec![vec![json!({"tag": "md", "text": ""})]];
    }
    if !text.contains("```") {
        return vec![vec![json!({"tag": "md", "text": text})]];
    }

    let mut rows = Vec::new();
    let mut current = Vec::new();
    let mut in_code_block = false;

    let flush_current = |rows: &mut Vec<Vec<Value>>, current: &mut Vec<String>| {
        if current.is_empty() {
            return;
        }
        let segment = current.join("\n");
        if !segment.trim().is_empty() {
            rows.push(vec![json!({"tag": "md", "text": segment})]);
        }
        current.clear();
    };

    for raw_line in text.lines() {
        let stripped = raw_line.trim();
        let is_fence = stripped.starts_with("```") && stripped.ends_with("```")
            || stripped == "```"
            || stripped.starts_with("```");
        if is_fence {
            if !in_code_block {
                flush_current(&mut rows, &mut current);
            }
            current.push(raw_line.to_string());
            in_code_block = !in_code_block;
            if !in_code_block {
                flush_current(&mut rows, &mut current);
            }
            continue;
        }
        current.push(raw_line.to_string());
    }
    flush_current(&mut rows, &mut current);
    if rows.is_empty() {
        rows.push(vec![json!({"tag": "md", "text": text})]);
    }
    rows
}

fn preview(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out.replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::format_text_chunks;

    #[test]
    fn plain_text_uses_text_mode() {
        let chunks = format_text_chunks("hello world", false);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].msg_type, "text");
    }

    #[test]
    fn markdown_list_uses_post_mode() {
        let chunks = format_text_chunks("- a\n- b", false);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].msg_type, "post");
    }

    #[test]
    fn markdown_table_falls_back_to_text_mode() {
        let chunks = format_text_chunks("| a |\n| - |\n| b |", false);
        assert_eq!(chunks[0].msg_type, "text");
    }

    #[test]
    fn markdown_table_is_rewritten_for_mobile_readability() {
        let chunks = format_text_chunks(
            "| 提交哈希 | 提交信息 |\n| --- | --- |\n| `abc` | hello |\n| `def` | world |",
            false,
        );
        let rendered = chunks[0]
            .content
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(rendered.contains("提交哈希: abc"));
        assert!(rendered.contains("提交信息: hello"));
    }

    #[test]
    fn text_mode_strips_inline_markdown_noise() {
        let chunks = format_text_chunks(
            "| 项目 | 说明 |\n| --- | --- |\n| [文档](https://example.com) | **重点** |",
            false,
        );
        let rendered = chunks[0]
            .content
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(rendered.contains("文档 (https://example.com)"));
        assert!(rendered.contains("重点"));
        assert!(!rendered.contains("**"));
    }

    #[test]
    fn dense_markdown_prefers_plain_text_fallback() {
        let chunks = format_text_chunks(
            "```rs\nfn a(){}\n```\n```rs\nfn b(){}\n```\n```rs\nfn c(){}\n```\n```rs\nfn d(){}\n```",
            false,
        );
        assert_eq!(chunks[0].msg_type, "text");
    }

    #[test]
    fn long_structured_report_prefers_text_mode() {
        let chunks = format_text_chunks(
            "## 最近 3 条 Git 改动\n\n### 1️⃣ 第一项\n- **方向**：统一平台\n- **说明**：这是一个很长的结构化总结，用来测试飞书手机端阅读体验是否优先回退为纯文本。\n\n### 2️⃣ 第二项\n- **方向**：长连接接入\n- **说明**：继续补充多段内容，确保超过结构化回退阈值。",
            false,
        );
        assert_eq!(chunks[0].msg_type, "text");
        let payload = chunks[0].content.to_string();
        assert!(payload.contains("最近 3 条 Git 改动"));
        assert!(payload.contains("• 方向：统一平台"));
    }

    #[test]
    fn text_mode_strips_heading_prefix_and_keeps_list_readable() {
        let chunks = format_text_chunks("## 标题\n- **重点**：说明", false);
        assert_eq!(chunks[0].msg_type, "post");
        let payload = chunks[0].content.to_string();
        assert!(payload.contains("**标题**"));
        assert!(payload.contains("- **重点**：说明"));
        assert!(!payload.contains("## 标题"));
    }

    #[test]
    fn post_mode_demotes_heading_markers_to_bold_lines() {
        let chunks = format_text_chunks("## 总体方向\n###2️⃣ 第二项", false);
        assert_eq!(chunks[0].msg_type, "post");
        let payload = chunks[0].content.to_string();
        assert!(payload.contains("**总体方向**"));
        assert!(payload.contains("**2️⃣ 第二项**"));
        assert!(!payload.contains("## 总体方向"));
    }

    #[test]
    fn group_context_prefers_text_for_structured_markdown() {
        let chunks = format_text_chunks(
            "## 最近 3 条 Git 改动\n\n### 1️⃣ 第一项\n- **方向**：统一平台\n- **说明**：较长结构化总结。\n\n### 2️⃣ 第二项\n- **方向**：长连接接入\n- **说明**：继续补充多段内容。",
            true,
        );
        assert_eq!(chunks[0].msg_type, "text");
    }

    #[test]
    fn group_text_mode_optimizes_headings_and_summary_lines() {
        let chunks = format_text_chunks(
            "## 总体方向\n方向：统一网关平台\n说明：继续优化群聊阅读体验",
            true,
        );
        let rendered = chunks[0]
            .content
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(rendered.contains("【总体方向】"));
        assert!(rendered.contains("方向\n统一网关平台"));
        assert!(rendered.contains("说明\n继续优化群聊阅读体验"));
    }

    #[test]
    fn group_text_mode_turns_numbered_sections_into_compact_headings() {
        let chunks =
            format_text_chunks("1️⃣ ff39d6a — WIP: unify gateway platform integration", true);
        let rendered = chunks[0]
            .content
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(rendered.contains("【1️⃣ ff39d6a — WIP: unify gateway platform integration】"));
    }

    #[test]
    fn group_text_mode_splits_bulleted_summary_lines() {
        let chunks = format_text_chunks("• 方向：统一网关平台", true);
        let rendered = chunks[0]
            .content
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(rendered.contains("• 方向\n  统一网关平台"));
    }
}
