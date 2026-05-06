pub(crate) fn is_exploration_command(command: &str) -> bool {
    let normalized = command.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.contains("&&")
        || normalized.contains("||")
        || normalized.contains('|')
        || normalized.contains(';')
        || normalized.contains('>')
        || normalized.contains('<')
    {
        return false;
    }

    normalized.starts_with("ls ")
        || normalized == "ls"
        || normalized.starts_with("dir ")
        || normalized == "dir"
        || normalized == "pwd"
        || normalized.starts_with("cat ")
        || normalized.starts_with("type ")
        || normalized.starts_with("rg ")
        || normalized.starts_with("grep ")
        || normalized.starts_with("findstr ")
        || normalized.starts_with("select-string ")
        || normalized.starts_with("git grep ")
}

pub(crate) fn summarize_exploration_command(command: &str) -> String {
    let compact = compact_inline(command.trim(), 72);
    if let Some((_, rhs)) = compact.rsplit_once("&&") {
        compact_inline(rhs.trim(), 56)
    } else {
        compact
    }
}

fn compact_inline(input: &str, max_chars: usize) -> String {
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
