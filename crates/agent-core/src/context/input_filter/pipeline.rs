pub(crate) fn filter_tool_output(content: &str) -> String {
    let mut lines = content
        .lines()
        .map(strip_ansi)
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !looks_like_progress(line))
        .map(|line| truncate_line(&line, 220))
        .collect::<Vec<_>>();

    if lines.len() > 120 {
        let tail = lines.split_off(lines.len() - 40);
        let mut compacted = lines.into_iter().take(80).collect::<Vec<_>>();
        compacted.push("... (output truncated, kept head/tail)".to_string());
        compacted.extend(tail);
        lines = compacted;
    }

    if lines.is_empty() {
        return "(no significant output)".to_string();
    }
    lines.join("\n")
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in line.chars().take(max_chars) {
        out.push(ch);
    }
    if line.chars().count() > max_chars {
        out.push_str(" ...");
    }
    out
}

fn looks_like_progress(line: &str) -> bool {
    let s = line.trim();
    (s.contains('%') && (s.contains("Downloading") || s.contains("download") || s.contains("ETA")))
        || s.starts_with('[') && s.ends_with(']') && s.chars().any(|c| c == '=' || c == '#')
}

pub(crate) fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() {
                let b = bytes[i];
                i += 1;
                if (b as char).is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
