use crate::app::TuiApp;
use crate::app::clipboard_paste::paste_image_to_temp_png;
use crate::state::NoticeLevel;

pub(crate) fn handle_local_image_paste(app: &mut TuiApp) {
    match paste_image_to_temp_png() {
        Ok(path) => {
            let _ = app.bottom_pane.attach_image(path);
        }
        Err(err) => {
            app.bottom_pane
                .show_transient_notice(NoticeLevel::Warn, format!("Failed to paste image: {err}"));
        }
    }
}
