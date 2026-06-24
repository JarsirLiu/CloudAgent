use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_rust_build_test_output(raw: &str) -> String {
    let errors = raw.lines().filter(|l| l.contains("error")).count();
    let warns = raw.lines().filter(|l| l.contains("warning")).count();
    let mut out = format!("Cargo summary: {errors} errors, {warns} warnings");
    let tail = filter_tool_output(raw);
    if tail != "(no significant output)" {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}
