use crate::app::TuiApp;
use crate::terminal::Frame;
use crate::ui::chat_surface::ChatSurface;

impl TuiApp {
    pub(crate) fn render(&mut self, frame: &mut Frame) {
        ChatSurface::render(self, frame);
    }
}
