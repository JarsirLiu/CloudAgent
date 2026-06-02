use crate::app::TuiApp;
use crate::app::clipboard_paste::{ClipboardPasteContent, paste_clipboard_content};
use crate::state::NoticeLevel;

pub(crate) fn handle_clipboard_paste(app: &mut TuiApp) {
    match paste_clipboard_content() {
        Ok(ClipboardPasteContent::Image(path)) => {
            let _ = app.bottom_pane.attach_image(path);
        }
        Ok(ClipboardPasteContent::Text(text)) => {
            let _ = app.bottom_pane.handle_paste(&text);
        }
        Err(err) => {
            app.bottom_pane.show_transient_notice(
                NoticeLevel::Warn,
                format!("Failed to paste clipboard content: {err}"),
            );
        }
    }
}
