use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_test_output(raw: &str) -> String {
    let failed = raw
        .lines()
        .filter(|l| l.contains("FAILED") || l.contains("failed"))
        .count();
    let passed = raw
        .lines()
        .filter(|l| l.contains("PASSED") || l.contains("passed"))
        .count();
    let mut out = format!("Test summary: {passed} passed, {failed} failed");
    let tail = filter_tool_output(raw);
    if tail != "(no significant output)" {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}
