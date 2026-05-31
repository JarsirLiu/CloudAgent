use crate::impls::command::search_fallback::{preferred_windows_shell, windows_utf8_command};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub(super) fn build_command_process(
    command_text: &str,
    workdir: &Path,
    pipe_stdin: bool,
) -> Command {
    let mut command = if cfg!(windows) {
        let mut cmd = Command::new(preferred_windows_shell());
        cmd.arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(windows_utf8_command(command_text));
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-lc").arg(command_text);
        cmd
    };
    command
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if pipe_stdin {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    command.env("NO_COLOR", "1");
    command.env("TERM", "dumb");
    command.env("COLORTERM", "");
    command.env("PAGER", "cat");
    command.env("GIT_PAGER", "cat");
    command.env("GH_PAGER", "cat");
    command.env("MANPAGER", "cat");
    command.env("EDITOR", "true");
    command.env("VISUAL", "true");
    command.env("CI", "1");
    command.env("CLOUDAGENT_CI", "1");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.kill_on_drop(true);
    configure_captured_command_process(&mut command);
    command
}

fn configure_captured_command_process(_command: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        _command.creation_flags(CREATE_NO_WINDOW);
    }
}
