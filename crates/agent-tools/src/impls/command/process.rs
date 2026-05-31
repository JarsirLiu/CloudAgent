use crate::impls::command::search_fallback::{preferred_windows_shell, windows_utf8_command};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub(super) fn build_command_process(command_text: &str, workdir: &Path) -> Command {
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
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
