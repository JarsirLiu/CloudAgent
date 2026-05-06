use crate::app::runtime::scrollback_browse::ScrollbackBrowseState;

pub(crate) enum RenderGateIntent {
    AppInput,
    TerminalScrollbackBrowse,
    Resize,
}

#[derive(Default)]
pub(crate) struct RenderGate {
    scrollback_browse: ScrollbackBrowseState,
}

impl RenderGate {
    pub(crate) fn apply_intent(&mut self, intent: RenderGateIntent) {
        match intent {
            RenderGateIntent::AppInput | RenderGateIntent::Resize => {
                self.scrollback_browse.leave();
            }
            RenderGateIntent::TerminalScrollbackBrowse => {
                self.scrollback_browse.enter();
            }
        }
    }

    pub(crate) fn should_draw(&self, needs_redraw: bool) -> bool {
        needs_redraw && !self.scrollback_browse.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderGate, RenderGateIntent};

    #[test]
    fn scrollback_input_temporarily_blocks_redraw() {
        let mut gate = RenderGate::default();

        assert!(gate.should_draw(true));
        gate.apply_intent(RenderGateIntent::TerminalScrollbackBrowse);
        assert!(!gate.should_draw(true));
    }

    #[test]
    fn app_input_releases_scrollback_suspend() {
        let mut gate = RenderGate::default();

        gate.apply_intent(RenderGateIntent::TerminalScrollbackBrowse);
        assert!(!gate.should_draw(true));

        gate.apply_intent(RenderGateIntent::AppInput);
        assert!(gate.should_draw(true));
    }

    #[test]
    fn resize_releases_scrollback_suspend() {
        let mut gate = RenderGate::default();

        gate.apply_intent(RenderGateIntent::TerminalScrollbackBrowse);
        gate.apply_intent(RenderGateIntent::Resize);

        assert!(gate.should_draw(true));
    }
}
