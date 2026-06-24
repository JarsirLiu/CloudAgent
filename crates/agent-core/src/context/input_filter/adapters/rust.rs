use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_cargo_test_output(raw: &str) -> String {
    let (errors, warnings, failures) = summarize_cargo_lines(raw);
    let mut out = format!("Cargo test summary: {errors} errors, {warnings} warnings, {failures} failures");
    let block = cargo_test_failure_block(raw);
    if !block.is_empty() {
        out.push('\n');
        out.push_str(&block);
    }
    out
}

pub(crate) fn filter_cargo_build_output(raw: &str) -> String {
    let (errors, warnings, _) = summarize_cargo_lines(raw);
    let mut out = format!("Cargo build summary: {errors} errors, {warnings} warnings");
    let tail = cargo_failure_block(raw);
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}

pub(crate) fn filter_cargo_clippy_output(raw: &str) -> String {
    let (errors, warnings, failures) = summarize_cargo_lines(raw);
    let mut out = format!("Cargo clippy summary: {errors} errors, {warnings} warnings, {failures} failures");
    let tail = cargo_failure_block(raw);
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}

pub(crate) fn filter_cargo_fmt_output(raw: &str) -> String {
    let stats = cargo_fmt_stats(raw);
    let mut out = format!(
        "Cargo fmt summary: {} files, {} formatted, {} reformatted, {} skipped, {} errors",
        stats.files, stats.formatted, stats.reformatted, stats.skipped, stats.errors
    );
    if let Some(mode) = stats.mode {
        out.push_str(" | mode=");
        out.push_str(mode);
    }
    let tail = cargo_fmt_block(raw);
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}

pub(crate) fn filter_cargo_install_output(raw: &str) -> String {
    let mut out = String::from("Install summary:");
    let tail = filter_tool_output(raw);
    if tail != "(no significant output)" {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}

fn summarize_cargo_lines(raw: &str) -> (usize, usize, usize) {
    let errors = raw.lines().filter(|l| l.contains("error")).count();
    let warns = raw.lines().filter(|l| l.contains("warning")).count();
    let failed = raw
        .lines()
        .filter(|l| l.contains("FAILED") || l.contains("failed") || l.contains("panicked"))
        .count();
    (errors, warns, failed)
}

fn cargo_failure_block(raw: &str) -> String {
    let lines: Vec<_> = raw
        .lines()
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            lower.contains("error")
                || lower.contains("failed")
                || lower.contains("panic")
                || lower.contains("note")
                || lower.contains("warning")
        })
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    if lines.len() > 80 {
        let mut compact = Vec::with_capacity(41);
        compact.extend(lines.iter().take(30).cloned());
        compact.push("... (cargo output truncated, kept head)".to_string());
        let mut tail = lines.iter().rev().take(10).cloned().collect::<Vec<_>>();
        tail.reverse();
        compact.extend(tail);
        return compact.join("\n");
    }
    lines.join("\n")
}

fn cargo_test_failure_block(raw: &str) -> String {
    let mut selected = Vec::new();
    let mut capture = false;

    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("test result:") || lower.contains("failures:") || lower.contains("----") {
            capture = true;
        }
        if capture {
            if lower.contains("failures:")
                || lower.contains("error:")
                || lower.contains("failed")
                || lower.contains("panic")
                || lower.contains("note:")
                || lower.contains("stdout")
                || lower.contains("stderr")
                || line.starts_with("---- ")
            {
                selected.push(line.to_string());
            }
        }
    }

    if selected.is_empty() {
        return cargo_failure_block(raw);
    }

    if selected.len() > 80 {
        let mut compact = Vec::with_capacity(41);
        compact.extend(selected.iter().take(30).cloned());
        compact.push("... (cargo test output truncated, kept head)".to_string());
        let mut tail = selected.iter().rev().take(10).cloned().collect::<Vec<_>>();
        tail.reverse();
        compact.extend(tail);
        return compact.join("\n");
    }

    selected.join("\n")
}

fn cargo_fmt_block(raw: &str) -> String {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("formatted")
            || lower.contains("reformatted")
            || lower.contains("skipped")
            || lower.contains("error")
            || lower.contains("warning")
            || lower.contains("diff")
            || lower.contains("would reformat")
        {
            lines.push(line.to_string());
        }
    }

    if lines.is_empty() {
        return String::new();
    }

    let mut compacted = Vec::new();
    let mut seen_diff_header = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("diff ") {
            if seen_diff_header {
                continue;
            }
            seen_diff_header = true;
        }
        compacted.push(line);
    }

    if compacted.len() > 24 {
        let mut compact = Vec::with_capacity(21);
        compact.extend(compacted.iter().take(12).cloned());
        compact.push("... (cargo fmt output truncated, kept head)".to_string());
        let mut tail = compacted.iter().rev().take(8).cloned().collect::<Vec<_>>();
        tail.reverse();
        compact.extend(tail);
        return compact.join("\n");
    }
    compacted.join("\n")
}

#[derive(Default)]
struct CargoFmtStats {
    files: usize,
    formatted: usize,
    reformatted: usize,
    skipped: usize,
    errors: usize,
    mode: Option<&'static str>,
}

fn cargo_fmt_stats(raw: &str) -> CargoFmtStats {
    let mut stats = CargoFmtStats::default();
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("would reformat") {
            stats.reformatted += 1;
            stats.files += 1;
        } else if lower.contains("formatted") {
            stats.formatted += 1;
            stats.files += 1;
        } else if lower.contains("skipped") {
            stats.skipped += 1;
        }
        if lower.contains("error") {
            stats.errors += 1;
        }
        if lower.contains("--check") {
            stats.mode = Some("check");
        } else if lower.contains("--emit") {
            stats.mode = Some("emit");
        }
    }
    stats
}
