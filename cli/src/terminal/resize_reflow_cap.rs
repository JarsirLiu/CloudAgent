use config::TerminalResizeReflowMaxRows;

const FALLBACK_RESIZE_REFLOW_MAX_ROWS: usize = 1_000;
const VSCODE_RESIZE_REFLOW_MAX_ROWS: usize = 1_000;
const WINDOWS_TERMINAL_RESIZE_REFLOW_MAX_ROWS: usize = 9_001;
const WEZTERM_RESIZE_REFLOW_MAX_ROWS: usize = 3_500;
const ALACRITTY_RESIZE_REFLOW_MAX_ROWS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalName {
    VsCode,
    WindowsTerminal,
    WezTerm,
    Alacritty,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSignals {
    name: TerminalName,
    running_in_vscode: bool,
}

pub(crate) fn resize_reflow_max_rows(config: TerminalResizeReflowMaxRows) -> Option<usize> {
    resize_reflow_max_rows_for(config, detect_terminal_signals())
}

fn resize_reflow_max_rows_for(
    config: TerminalResizeReflowMaxRows,
    terminal: TerminalSignals,
) -> Option<usize> {
    match config {
        TerminalResizeReflowMaxRows::Auto => Some(auto_resize_reflow_max_rows(terminal)),
        TerminalResizeReflowMaxRows::Disabled => None,
        TerminalResizeReflowMaxRows::Limit(rows) => Some(rows),
    }
}

fn auto_resize_reflow_max_rows(terminal: TerminalSignals) -> usize {
    if terminal.running_in_vscode {
        return VSCODE_RESIZE_REFLOW_MAX_ROWS;
    }

    match terminal.name {
        TerminalName::VsCode => VSCODE_RESIZE_REFLOW_MAX_ROWS,
        TerminalName::WindowsTerminal => WINDOWS_TERMINAL_RESIZE_REFLOW_MAX_ROWS,
        TerminalName::WezTerm => WEZTERM_RESIZE_REFLOW_MAX_ROWS,
        TerminalName::Alacritty => ALACRITTY_RESIZE_REFLOW_MAX_ROWS,
        TerminalName::Unknown => FALLBACK_RESIZE_REFLOW_MAX_ROWS,
    }
}

fn detect_terminal_signals() -> TerminalSignals {
    let term_program = env_lowercase("TERM_PROGRAM");
    let term = env_lowercase("TERM");
    let name = if std::env::var_os("WT_SESSION").is_some() {
        TerminalName::WindowsTerminal
    } else if term_program
        .as_deref()
        .is_some_and(|value| value.contains("vscode"))
    {
        TerminalName::VsCode
    } else if term_program
        .as_deref()
        .is_some_and(|value| value.contains("wezterm"))
        || term
            .as_deref()
            .is_some_and(|value| value.contains("wezterm"))
    {
        TerminalName::WezTerm
    } else if term_program
        .as_deref()
        .is_some_and(|value| value.contains("alacritty"))
        || term
            .as_deref()
            .is_some_and(|value| value.contains("alacritty"))
    {
        TerminalName::Alacritty
    } else {
        TerminalName::Unknown
    };

    TerminalSignals {
        name,
        running_in_vscode: std::env::var_os("VSCODE_INJECTION").is_some()
            || term_program
                .as_deref()
                .is_some_and(|value| value.contains("vscode")),
    }
}

fn env_lowercase(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_resize_reflow_max_rows_uses_terminal_defaults() {
        let cases = [
            (TerminalName::VsCode, VSCODE_RESIZE_REFLOW_MAX_ROWS),
            (
                TerminalName::WindowsTerminal,
                WINDOWS_TERMINAL_RESIZE_REFLOW_MAX_ROWS,
            ),
            (TerminalName::WezTerm, WEZTERM_RESIZE_REFLOW_MAX_ROWS),
            (TerminalName::Alacritty, ALACRITTY_RESIZE_REFLOW_MAX_ROWS),
            (TerminalName::Unknown, FALLBACK_RESIZE_REFLOW_MAX_ROWS),
        ];

        for (name, expected) in cases {
            assert_eq!(
                auto_resize_reflow_max_rows(TerminalSignals {
                    name,
                    running_in_vscode: false,
                }),
                expected
            );
        }
    }

    #[test]
    fn auto_resize_reflow_max_rows_prefers_vscode_probe() {
        assert_eq!(
            auto_resize_reflow_max_rows(TerminalSignals {
                name: TerminalName::WindowsTerminal,
                running_in_vscode: true,
            }),
            VSCODE_RESIZE_REFLOW_MAX_ROWS
        );
    }

    #[test]
    fn configured_resize_reflow_max_rows_overrides_auto_detection() {
        assert_eq!(
            resize_reflow_max_rows_for(
                TerminalResizeReflowMaxRows::Limit(42),
                TerminalSignals {
                    name: TerminalName::WindowsTerminal,
                    running_in_vscode: false,
                },
            ),
            Some(42)
        );
    }

    #[test]
    fn disabled_resize_reflow_max_rows_keeps_all_rows() {
        assert_eq!(
            resize_reflow_max_rows_for(
                TerminalResizeReflowMaxRows::Disabled,
                TerminalSignals {
                    name: TerminalName::WindowsTerminal,
                    running_in_vscode: false,
                },
            ),
            None
        );
    }
}
