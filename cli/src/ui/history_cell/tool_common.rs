use crate::tool_identity::WEB_SEARCH_TOOL_NAME;
use agent_core::RuntimeItem;

pub(super) fn humanize_tool_label(tool_name: &str) -> String {
    match tool_name {
        "exec_command" | "tool" => "Run command".to_string(),
        "apply_patch" | "edit_file" => "Edit file".to_string(),
        WEB_SEARCH_TOOL_NAME => "Web search".to_string(),
        "read_file" => "Read file".to_string(),
        "read_directory" => "Read directory".to_string(),
        "search_workspace" => "Search workspace".to_string(),
        "tool_search" => "Search tools".to_string(),
        "get_metadata" => "File info".to_string(),
        "create_directory" => "Create directory".to_string(),
        "write_file" => "Write file".to_string(),
        "copy_path" => "Copy path".to_string(),
        "remove_path" => "Remove path".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn compact_inline(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            return out;
        }
        out.push(if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        });
    }
    out
}

pub(super) fn compact_path(path: &str, max_chars: usize) -> String {
    let path = path.replace('\\', "/");
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_chars {
        return path;
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let tail_len = max_chars.saturating_sub(1);
    let tail: String = chars[chars.len().saturating_sub(tail_len)..]
        .iter()
        .collect();
    format!("…{tail}")
}

pub(super) fn format_line_range(start_line: Option<usize>, end_line: Option<usize>) -> String {
    match (start_line, end_line) {
        (Some(start), Some(end)) if end >= start => format!(":{start}-{end}"),
        (Some(start), _) => format!(":{start}"),
        _ => String::new(),
    }
}

pub(super) fn runtime_summary(item: &RuntimeItem) -> Option<String> {
    item.progress
        .as_ref()
        .and_then(|progress| progress.message.clone())
        .or_else(|| item.summary.clone())
        .filter(|summary| !summary.trim().is_empty())
}
