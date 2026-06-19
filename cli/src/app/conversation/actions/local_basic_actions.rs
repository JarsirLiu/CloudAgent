use crate::app::conversation::actions::show_local_notice;
use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use crate::app::conversation::image_paste::handle_clipboard_paste;
use crate::app::effects::copy_text_to_clipboard;
use anyhow::Result;

pub(crate) async fn handle_basic_input(
    app: &mut TuiApp,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalCopy => {
            let Some(text) = app.transcript_owner.last_copyable_output() else {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Warn,
                    "`/copy` unavailable before first assistant output",
                );
                return Ok(false);
            };
            match copy_text_to_clipboard(text) {
                Ok(()) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Info,
                        "Copied latest assistant output",
                    );
                }
                Err(err) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Error,
                        format!("failed to copy: {err}"),
                    );
                }
            }
        }
        ParsedInput::LocalCopyText(text) => match copy_text_to_clipboard(&text) {
            Ok(()) => {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Info,
                    "Copied selected input text",
                );
            }
            Err(err) => {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Error,
                    format!("failed to copy: {err}"),
                );
            }
        },
        ParsedInput::LocalImagePaste => {
            handle_clipboard_paste(app);
        }
        ParsedInput::LocalHelp => {
            app.bottom_pane.set_help_view();
            show_local_notice(
                app,
                crate::state::NoticeLevel::Info,
                "Command help opened. Esc to close.",
            );
        }
        ParsedInput::LocalInputError(message) => {
            show_local_notice(app, crate::state::NoticeLevel::Warn, message);
        }
        _ => unreachable!("basic input dispatcher received non-basic input"),
    }
    Ok(false)
}
