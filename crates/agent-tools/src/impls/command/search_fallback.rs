use std::env;

const DEFAULT_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "dist",
    "build",
    "target",
    "target-verify",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    "coverage",
    ".venv",
    "venv",
    "__pycache__",
];

pub(super) fn translate_search_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if !normalized.starts_with("rg") {
        return None;
    }

    if command_exists("rg") || command_exists("rg.exe") {
        return None;
    }

    translate_rg_command(trimmed)
}

pub(super) fn preferred_windows_shell() -> String {
    find_windows_shell().unwrap_or_else(|| "powershell".to_string())
}

pub(super) fn windows_utf8_command(command: &str) -> String {
    format!(
        concat!(
            "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); ",
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); ",
            "$OutputEncoding = [System.Text.UTF8Encoding]::new($false); ",
            "chcp 65001 > $null; ",
            "{command}"
        ),
        command = command
    )
}

fn find_windows_shell() -> Option<String> {
    for candidate in ["pwsh.exe", "pwsh", "powershell.exe", "powershell"] {
        if command_exists(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn command_exists(candidate: &str) -> bool {
    if candidate.contains('\\') || candidate.contains('/') {
        return std::path::Path::new(candidate).exists();
    }
    let path_value = env::var_os("PATH");
    let Some(path_value) = path_value else {
        return false;
    };
    env::split_paths(&path_value).any(|dir| {
        let direct = dir.join(candidate);
        if direct.exists() {
            return true;
        }
        if direct.extension().is_none() {
            return dir.join(format!("{candidate}.exe")).exists();
        }
        false
    })
}

fn translate_rg_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if !normalized.starts_with("rg") {
        return None;
    }

    if normalized.starts_with("rg --files") || normalized.starts_with("rg.exe --files") {
        let path_scope = extract_rg_path_scope(trimmed).unwrap_or_else(|| ".".to_string());
        return Some(if cfg!(windows) {
            format!(
                "{} | Select-Object -ExpandProperty FullName",
                windows_bounded_file_listing_command(&path_scope)
            )
        } else {
            unix_bounded_file_listing_command(&path_scope)
        });
    }

    let (pattern, path_scope, case_sensitive) = extract_rg_search_args(trimmed)?;
    Some(if cfg!(windows) {
        let mut command = format!(
            "{} | Select-String -Pattern {}",
            windows_bounded_file_listing_command(&path_scope),
            powershell_quote(&pattern)
        );
        if !case_sensitive {
            command.push_str(" -CaseSensitive:$false");
        }
        command
    } else {
        let mut command = format!(
            "{} | xargs -0 grep -n -I",
            unix_bounded_file_listing_command(&path_scope)
        );
        if !case_sensitive {
            command.push_str(" -i");
        }
        command.push_str(" -e ");
        command.push_str(&shell_quote(&pattern));
        command
    })
}

fn windows_bounded_file_listing_command(path_scope: &str) -> String {
    let mut command = format!(
        "Get-ChildItem -Recurse -File {}",
        powershell_quote(path_scope)
    );
    for ignored in DEFAULT_IGNORED_DIRS {
        command.push_str(&format!(
            " | Where-Object {{ $_.FullName -notmatch '[\\\\/]{ignored}([\\\\/]|$)' }}"
        ));
    }
    command
}

fn unix_bounded_file_listing_command(path_scope: &str) -> String {
    let mut command = format!("find {}", shell_quote(path_scope));
    for ignored in DEFAULT_IGNORED_DIRS {
        command.push_str(&format!(
            " -path {} -prune -o",
            shell_quote(&format!("*/{ignored}"))
        ));
    }
    command.push_str(" -type f -print0");
    command
}

fn extract_rg_search_args(command: &str) -> Option<(String, String, bool)> {
    let mut rest = command.trim();
    let program = take_shell_token(&mut rest)?.to_ascii_lowercase();
    if program != "rg" && program != "rg.exe" {
        return None;
    }

    let mut case_sensitive = true;
    let mut pattern = None;
    let mut path_scope = ".".to_string();
    while !rest.trim().is_empty() {
        let token = take_shell_token(&mut rest)?;
        match token.as_str() {
            "-i" | "--ignore-case" => case_sensitive = false,
            "-n" | "-H" | "--with-filename" | "--no-heading" | "--line-number" | "-S" => {}
            "--files" => return None,
            t if t.starts_with('-') => {}
            t if pattern.is_none() => pattern = Some(t.to_string()),
            t => path_scope = t.to_string(),
        }
    }

    Some((pattern?, path_scope, case_sensitive))
}

fn extract_rg_path_scope(command: &str) -> Option<String> {
    let mut rest = command.trim();
    let program = take_shell_token(&mut rest)?.to_ascii_lowercase();
    if program != "rg" && program != "rg.exe" {
        return None;
    }

    let mut path_scope = ".".to_string();
    while !rest.trim().is_empty() {
        let token = take_shell_token(&mut rest)?;
        if token == "--files" {
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        path_scope = token;
        break;
    }

    Some(path_scope)
}

fn take_shell_token(input: &mut &str) -> Option<String> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        *input = "";
        return None;
    }

    let mut chars = trimmed.chars().peekable();
    let mut token = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut consumed = 0usize;

    for ch in chars.by_ref() {
        consumed += ch.len_utf8();
        match ch {
            '\'' | '"' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            ch if in_quotes && ch == quote_char => {
                in_quotes = false;
            }
            ch if !in_quotes && ch.is_whitespace() => {
                break;
            }
            _ => token.push(ch),
        }
    }

    *input = trimmed[consumed..].trim_start();
    if token.is_empty() { None } else { Some(token) }
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let escaped = value.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

#[cfg(test)]
mod tests {
    use super::{command_exists, take_shell_token, translate_rg_command};

    #[test]
    fn command_exists_rejects_missing_binary() {
        assert!(!command_exists("cloudagent-definitely-missing-command"));
    }

    #[test]
    fn rg_search_commands_fall_back_when_rg_is_missing() {
        let translated = translate_rg_command(r#"rg -n "context|token" crates/agent-core"#)
            .expect("search command should translate");
        if cfg!(windows) {
            assert!(translated.contains("Get-ChildItem -Recurse -File"));
            assert!(translated.contains("Select-String -Pattern"));
            assert!(translated.contains("crates/agent-core"));
            assert!(translated.contains("node_modules"));
        } else {
            assert!(translated.starts_with("find 'crates/agent-core'"));
            assert!(translated.contains("-prune"));
            assert!(translated.contains("| xargs -0 grep -n -I"));
        }
    }

    #[test]
    fn rg_files_search_commands_fall_back_when_rg_is_missing() {
        let translated =
            translate_rg_command("rg --files crates").expect("files search should translate");
        if cfg!(windows) {
            assert!(translated.contains("Get-ChildItem -Recurse -File"));
            assert!(translated.contains("-ExpandProperty FullName"));
            assert!(translated.contains("target"));
        } else {
            assert!(translated.starts_with("find 'crates'"));
            assert!(translated.contains("-print0"));
        }
    }

    #[test]
    fn shell_token_parser_handles_quoted_patterns() {
        let mut input = r#"-n "context|token" crates/agent-core"#;
        assert_eq!(take_shell_token(&mut input), Some("-n".to_string()));
        assert_eq!(
            take_shell_token(&mut input),
            Some("context|token".to_string())
        );
        assert_eq!(
            take_shell_token(&mut input),
            Some("crates/agent-core".to_string())
        );
    }
}
