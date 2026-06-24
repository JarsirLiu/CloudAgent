use super::super::pipeline::filter_tool_output;

pub(crate) fn filter_git_output(cmd: &str, raw: &str) -> String {
    if cmd.starts_with("git status") {
        let files = raw
            .lines()
            .filter(|l| {
                l.trim_start().starts_with("modified:") || l.trim_start().starts_with("new file:")
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
        let adds = raw
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .count();
        let dels = raw
            .lines()
            .filter(|l| l.starts_with('-') && !l.starts_with("---"))
            .count();
        return format!(
            "Git diff summary: +{adds} / -{dels}\n{}",
            filter_tool_output(raw)
        );
    }
    filter_tool_output(raw)
}
