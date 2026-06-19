mod actions;
mod render;
mod state;

use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use agent_protocol::{PlatformConfigResponse, PlatformControlEntry};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

pub struct GatewayPanel {
    pub(crate) mode: state::GatewayPanelMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinLoginSessionView {
    pub session_id: String,
    pub qr_url: String,
}

impl GatewayPanel {
    pub fn list(entries: Vec<PlatformControlEntry>) -> Self {
        Self {
            mode: state::GatewayPanelMode::List {
                entries,
                selected: 0,
            },
        }
    }

    pub fn edit(
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
        weixin_login: Option<WeixinLoginSessionView>,
    ) -> Self {
        Self {
            mode: state::GatewayPanelMode::Edit {
                platform: entry.platform,
                enabled: entry.enabled,
                configured: config.configured,
                selected: 0,
                fields: config
                    .fields
                    .into_iter()
                    .map(|field| state::EditableField::new(field))
                    .collect(),
                weixin_login,
            },
        }
    }
}

impl BottomPaneView for GatewayPanel {
    fn kind(&self) -> ViewKind {
        render::kind(&self.mode)
    }

    fn should_capture_global_paste_shortcut(&self) -> bool {
        false
    }

    fn supports_text_paste_shortcut(&self) -> bool {
        true
    }

    fn handle_paste(&mut self, text: &str) -> BottomPaneViewAction {
        actions::handle_paste(self, text)
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        actions::handle_key_event(self, key)
    }

    fn render_lines(&self, area_width: u16) -> Vec<ratatui::text::Line<'static>> {
        render::render_lines(self, area_width)
    }

    fn cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        render::cursor_position(self, area)
    }
}

impl Default for GatewayPanel {
    fn default() -> Self {
        Self::list(Vec::new())
    }
}

#[cfg(test)]
#[path = "gateway_panel/tests.rs"]
mod tests;
