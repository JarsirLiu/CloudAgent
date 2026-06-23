use crate::app::TuiApp;
use crate::ui::chat_surface::ChatSurface;
use agent_protocol::FrontendMode;
use ratatui::layout::Rect;

#[derive(Default)]
pub(crate) struct ViewportHeightPolicy {
    running_viewport_area: Option<Rect>,
    running_viewport_height: Option<u16>,
}

impl ViewportHeightPolicy {
    pub(crate) fn reset(&mut self) {
        self.running_viewport_area = None;
        self.running_viewport_height = None;
    }

    pub(crate) fn resolve(&mut self, app: &mut TuiApp, area: Rect) -> u16 {
        let desired_height = ChatSurface::desired_viewport_height(app, area);
        let should_lock_running_height = app.current_mode() == FrontendMode::Running
            && !app.transcript_owner.live_is_empty()
            && app.bottom_pane.no_modal_or_popup_active();

        if !should_lock_running_height {
            self.reset();
            return desired_height;
        }

        // Keep the inline viewport stable while a live stream is active so transcript growth
        // does not repeatedly clear and replay the visible frame.
        if self.running_viewport_area != Some(area) {
            self.running_viewport_area = Some(area);
            self.running_viewport_height = Some(desired_height);
        }

        self.running_viewport_height.unwrap_or(desired_height)
    }
}
