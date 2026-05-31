#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandAccess {
    ReadOnly,
    Network,
    Dangerous,
    MutatingOrUnknown,
}

impl CommandAccess {
    pub(crate) fn is_read_only(self) -> bool {
        matches!(self, Self::ReadOnly)
    }

    pub(crate) fn is_network(self) -> bool {
        matches!(self, Self::Network)
    }

    pub(crate) fn is_dangerous(self) -> bool {
        matches!(self, Self::Dangerous)
    }

    pub(crate) fn summary(self, command: &str) -> &'static str {
        if !self.is_read_only() {
            return "action";
        }

        let normalized = normalize_command(command);
        let Some(program) = normalized.split_whitespace().next() else {
            return "unknown";
        };

        match program {
            "rg" | "grep" | "findstr" | "select-string" => "search",
            "git" if normalized.starts_with("git ls-files") => "list files",
            "git" if normalized.starts_with("git grep") => "search",
            "git" => "inspect",
            "fd" => "find files",
            _ => "inspect",
        }
    }
}

pub(crate) fn classify_command(command: &str) -> CommandAccess {
    let normalized = normalize_command(command);
    if normalized.is_empty() {
        return CommandAccess::MutatingOrUnknown;
    }
    if is_dangerous_command(&normalized) {
        return CommandAccess::Dangerous;
    }
    if contains_network_indicator(&normalized) {
        return CommandAccess::Network;
    }
    if contains_write_operator(&normalized) {
        return CommandAccess::MutatingOrUnknown;
    }
    if is_safe_readonly_chain(&normalized) {
        CommandAccess::ReadOnly
    } else {
        CommandAccess::MutatingOrUnknown
    }
}

fn normalize_command(command: &str) -> String {
    command.trim().to_ascii_lowercase()
}

fn is_safe_readonly_chain(command: &str) -> bool {
    if command.contains("&&") {
        return command
            .split("&&")
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .all(is_safe_readonly_segment);
    }

    if command.contains(';') {
        return command
            .split(';')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .all(is_safe_readonly_segment);
    }

    is_safe_readonly_segment(command)
}

fn is_safe_readonly_segment(segment: &str) -> bool {
    let normalized = segment.trim();
    if normalized.is_empty() {
        return false;
    }

    if contains_write_operator(normalized) || contains_network_indicator(normalized) {
        return false;
    }

    let Some(program) = normalized.split_whitespace().next() else {
        return false;
    };

    match program {
        "cd" | "set-location" | "pushd" => {
            let location = normalized
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            !location.is_empty()
                && !location.starts_with("..")
                && !location.contains("/../")
                && !location.contains("\\..\\")
        }
        "pwd" | "ls" | "dir" | "cat" | "type" | "head" | "tail" | "find" | "tree" | "rg"
        | "grep" | "fd" | "findstr" | "select-string" | "get-childitem" | "get-content"
        | "measure-object" | "where-object" | "sort-object" | "select-object" => true,
        "git" => is_safe_git_command(normalized),
        _ => false,
    }
}

fn contains_write_operator(command: &str) -> bool {
    let write_markers = [
        " >",
        ">>",
        " out-file",
        " set-content",
        " add-content",
        " tee-object",
        " remove-item",
        " move-item",
        " copy-item",
        " rename-item",
        " new-item",
        " set-item",
        " rm ",
        " del ",
        " mv ",
        " cp ",
        " chmod ",
        " chown ",
        " mkdir ",
        " rmdir ",
        " sed -i",
    ];
    write_markers.iter().any(|marker| command.contains(marker))
}

fn contains_network_indicator(command: &str) -> bool {
    let network_markers = [
        "curl ",
        "wget ",
        "invoke-webrequest",
        "invoke-restmethod",
        "http://",
        "https://",
        " ping ",
        "ssh ",
        "scp ",
        "ftp ",
        "npm install",
        "pnpm install",
        "yarn add",
        "cargo install",
        "go get",
        "pip install",
    ];
    network_markers
        .iter()
        .any(|marker| command.contains(marker))
}

fn is_safe_git_command(command: &str) -> bool {
    [
        "git status",
        "git diff",
        "git show",
        "git log",
        "git branch",
        "git rev-parse",
        "git cat-file",
        "git ls-files",
        "git grep",
    ]
    .iter()
    .any(|prefix| command.starts_with(prefix))
}

fn is_dangerous_command(command: &str) -> bool {
    let dangerous_markers = [
        "rm -rf /",
        "rm -rf *",
        "del /s",
        "format ",
        "mkfs",
        "diskpart",
        "shutdown ",
        "reboot ",
        "init 0",
    ];
    if dangerous_markers
        .iter()
        .any(|marker| command.contains(marker))
    {
        return true;
    }

    is_recursive_delete_command(command)
}

fn is_recursive_delete_command(command: &str) -> bool {
    (command.contains("remove-item") && command.contains("-recurse"))
        || (command.contains("rm ") && (command.contains(" -rf") || command.contains(" -r")))
        || (command.contains("rmdir ") && command.contains("/s"))
        || (command.contains("rd ") && command.contains("/s"))
}

#[cfg(test)]
mod tests {
    use super::{CommandAccess, classify_command};

    #[test]
    fn classifies_readonly_commands() {
        assert_eq!(classify_command("rg -n TODO src"), CommandAccess::ReadOnly);
        assert_eq!(classify_command("git status"), CommandAccess::ReadOnly);
        assert_eq!(
            classify_command("Get-ChildItem -Force"),
            CommandAccess::ReadOnly
        );
    }

    #[test]
    fn classifies_mutating_network_and_dangerous_commands() {
        assert_eq!(
            classify_command("Set-Content out.txt hi"),
            CommandAccess::MutatingOrUnknown
        );
        assert_eq!(
            classify_command("curl https://example.com"),
            CommandAccess::Network
        );
        assert_eq!(
            classify_command("Remove-Item -Recurse -Force target"),
            CommandAccess::Dangerous
        );
    }
}
