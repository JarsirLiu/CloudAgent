use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_git_output(cmd: &str, raw: &str) -> String {
    if cmd.starts_with("git status") {
        let files = raw
            .lines()
            .filter(|l| {
                let trimmed = l.trim_start();
                trimmed.starts_with("modified:")
                    || trimmed.starts_with("new file:")
                    || trimmed.starts_with("deleted:")
                    || trimmed.starts_with("renamed:")
            })
            .count();
        if files > 0 {
            return format!(
                "Git status: {files} changed files\n{}",
                filter_tool_output(raw)
            );
        }
    }
    if cmd.starts_with("git diff") {
        return summarize_git_diff(raw);
    }
    if cmd.starts_with("git log") {
        return summarize_git_log(raw);
    }
    filter_tool_output(raw)
}

fn summarize_git_diff(raw: &str) -> String {
    let stats = git_diff_stats(raw);
    let mut out = format!(
        "Git diff summary: {} files changed, +{} / -{}",
        stats.file_count, stats.add_count, stats.delete_count
    );

    if stats.rename_count > 0 {
        out.push_str(&format!(", {} renames", stats.rename_count));
    }
    if stats.binary_count > 0 {
        out.push_str(&format!(", {} binary files", stats.binary_count));
    }

    if !stats.changed_files.is_empty() {
        out.push('\n');
        out.push_str("changed files:");
        for path in stats.changed_files.iter().take(8) {
            out.push('\n');
            out.push_str("- ");
            out.push_str(path);
        }
        if stats.changed_files.len() > 8 {
            out.push('\n');
            out.push_str(&format!(
                "... ({0} more files omitted)",
                stats.changed_files.len() - 8
            ));
        }
    }

    let tail = compact_git_diff_body(raw);
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}

fn summarize_git_log(raw: &str) -> String {
    let mut commits = 0usize;
    let mut entries = Vec::new();
    let mut current: Option<GitLogEntry> = None;

    for line in raw.lines() {
        if let Some(hash) = line.strip_prefix("commit ") {
            commits += 1;
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(GitLogEntry {
                hash: hash.trim().to_string(),
                subject: String::new(),
                body_first_line: None,
            });
            continue;
        }

        if let Some(entry) = current.as_mut() {
            if line.starts_with("Author:") || line.starts_with("Date:") || line.trim().is_empty() {
                continue;
            }
            if entry.subject.is_empty() {
                entry.subject = line.trim().to_string();
                continue;
            }
            if entry.body_first_line.is_none() && !line.trim().is_empty() {
                entry.body_first_line = Some(line.trim().to_string());
            }
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }

    let mut out = format!("Git log summary: {commits} commits");
    if !entries.is_empty() {
        out.push('\n');
        for entry in entries.iter().take(8) {
            out.push_str(&entry.hash);
            if !entry.subject.is_empty() {
                out.push(' ');
                out.push_str(&entry.subject);
            }
            if let Some(body) = &entry.body_first_line {
                out.push_str(" | ");
                out.push_str(body);
            }
            out.push('\n');
        }
        if entries.len() > 8 {
            out.push_str(&format!("... ({} more commits omitted)", entries.len() - 8));
        } else {
            out.pop();
        }
    }
    out
}

#[derive(Default)]
struct GitLogEntry {
    hash: String,
    subject: String,
    body_first_line: Option<String>,
}

#[derive(Default)]
struct GitDiffStats {
    file_count: usize,
    add_count: usize,
    delete_count: usize,
    rename_count: usize,
    binary_count: usize,
    changed_files: Vec<String>,
}

fn git_diff_stats(raw: &str) -> GitDiffStats {
    let mut stats = GitDiffStats::default();
    let mut current_file: Option<String> = None;

    for line in raw.lines() {
        if let Some(path) = parse_diff_file_path(line) {
            stats.file_count += 1;
            current_file = Some(path.clone());
            if stats.changed_files.len() < 64 && !stats.changed_files.contains(&path) {
                stats.changed_files.push(path);
            }
            continue;
        }
        if line.starts_with("rename from ") || line.starts_with("rename to ") {
            stats.rename_count += 1;
        }
        if line.contains("Binary files") || line.contains("GIT binary patch") {
            stats.binary_count += 1;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            stats.add_count += 1;
            let _ = &current_file;
        } else if line.starts_with('-') && !line.starts_with("---") {
            stats.delete_count += 1;
            let _ = &current_file;
        }
    }

    stats
}

fn parse_diff_file_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("diff --git a/")?;
    let (_, b_path) = rest.split_once(" b/")?;
    let path = b_path.trim();
    (!path.is_empty()).then(|| path.to_string())
}

fn compact_git_diff_body(raw: &str) -> String {
    let mut out = Vec::new();
    let mut current_file: Option<String> = None;
    let mut per_file_hunks = 0usize;
    let mut in_binary_patch = false;

    for line in raw.lines() {
        if line.starts_with("GIT binary patch") {
            in_binary_patch = true;
            out.push("GIT binary patch (omitted)".to_string());
            continue;
        }
        if in_binary_patch {
            if line.starts_with("diff --git ") {
                in_binary_patch = false;
            } else {
                continue;
            }
        }

        if let Some(path) = parse_diff_file_path(line) {
            current_file = Some(path.clone());
            per_file_hunks = 0;
            out.push(line.to_string());
            continue;
        }

        if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            out.push(line.to_string());
            continue;
        }

        if line.starts_with("@@") {
            if per_file_hunks >= 2 {
                continue;
            }
            per_file_hunks += 1;
            out.push(line.to_string());
            continue;
        }

        if line.starts_with('+') || line.starts_with('-') || line.starts_with("Binary files ") {
            if let Some(file) = &current_file {
                let _ = file;
            }
            out.push(line.to_string());
        }
    }

    if out.is_empty() {
        return filter_tool_output(raw);
    }

    if out.len() > 120 {
        let mut compact = Vec::with_capacity(81);
        compact.extend(out.iter().take(40).cloned());
        compact.push("... (diff body truncated, kept head)".to_string());
        let mut tail = out.iter().rev().take(40).cloned().collect::<Vec<_>>();
        tail.reverse();
        compact.extend(tail);
        return compact.join("\n");
    }

    out.join("\n")
}
