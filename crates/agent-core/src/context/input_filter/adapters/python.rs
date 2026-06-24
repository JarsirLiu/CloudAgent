use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_python_output(command: &str, raw: &str) -> String {
    if command.contains("pytest") {
        return summarize_python_test_output(raw);
    }

    if command.contains("pip install") || command.contains("pip3 install") {
        return summarize_python_install_output(raw);
    }

    if command.contains("python -m pip") || command.contains("python3 -m pip") {
        return summarize_python_pip_output(raw);
    }

    filter_tool_output(raw)
}

fn summarize_python_test_output(raw: &str) -> String {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("passed") {
            passed += 1;
        }
        if lower.contains("failed") {
            failed += 1;
        }
        if lower.contains("skipped") {
            skipped += 1;
        }
        if lower.contains("error") {
            errors += 1;
        }
    }
    format!(
        "Python test summary: {passed} passed, {failed} failed, {skipped} skipped, {errors} errors"
    )
}

fn summarize_python_install_output(raw: &str) -> String {
    let mut out = String::from("Python pip install summary:");
    let tail = filter_tool_output(raw);
    if tail != "(no significant output)" {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}

fn summarize_python_pip_output(raw: &str) -> String {
    let mut out = String::from("Python pip summary:");
    let tail = filter_tool_output(raw);
    if tail != "(no significant output)" {
        out.push('\n');
        out.push_str(&tail);
    }
    out
}
