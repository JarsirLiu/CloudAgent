use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_install_output(raw: &str) -> String {
    let kept = raw
        .lines()
        .filter(|line| {
            let l = line.to_ascii_lowercase();
            l.contains("error")
                || l.contains("warning")
                || l.contains("added")
                || l.contains("installed")
                || l.contains("audited")
                || l.contains("finished")
                || l.contains("compiling")
        })
        .collect::<Vec<_>>();

    if kept.is_empty() {
        return filter_tool_output(raw);
    }

    let mut compact = String::from("Install summary:\n");
    compact.push_str(&kept.join("\n"));
    compact
}
