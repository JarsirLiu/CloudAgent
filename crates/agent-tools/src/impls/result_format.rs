pub(crate) fn push_fact(lines: &mut Vec<String>, label: &str, value: impl Into<String>) {
    lines.push(format!("{label}: {}", value.into()));
}

pub(crate) fn push_section(lines: &mut Vec<String>, title: &str, body: impl Into<String>) {
    let body = body.into();
    if body.trim().is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("{title}:"));
    lines.push(body);
}

#[cfg(test)]
pub(crate) fn push_list_section(lines: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("{title}:"));
    for item in items {
        lines.push(format!("- {item}"));
    }
}

pub(crate) fn finalize(
    summary: impl Into<String>,
    mut lines: Vec<String>,
    next_step: Option<&str>,
) -> String {
    let mut rendered = vec![format!("Summary: {}", summary.into())];
    if !lines.is_empty() {
        rendered.push(String::new());
        rendered.append(&mut lines);
    }
    if let Some(next_step) = next_step.filter(|value| !value.trim().is_empty()) {
        rendered.push(String::new());
        rendered.push(format!("Next step: {next_step}"));
    }
    rendered.join("\n")
}
