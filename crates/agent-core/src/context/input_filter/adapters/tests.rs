use super::super::pipeline::filter_failure_tail;

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
    let tail = filter_failure_tail(raw);
    if !tail.is_empty() {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}
