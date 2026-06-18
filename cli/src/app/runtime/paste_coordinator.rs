use crate::input::keymap::matches_image_paste_shortcut;
use crossterm::event::{KeyEvent, KeyEventKind};
use std::time::{Duration, Instant};

const PASTE_DEDUP_WINDOW: Duration = Duration::from_millis(200);

#[derive(Default)]
pub(super) struct PasteCoordinator {
    pending_shortcut_paste: Option<PendingShortcutPaste>,
}

pub(super) enum TerminalPasteDecision {
    ForwardNoPending,
    ForwardExpiredPending,
    ForwardDifferentText,
    SuppressMatchingShortcutPaste,
}

struct PendingShortcutPaste {
    text: String,
    expires_at: Instant,
}

impl PasteCoordinator {
    pub(super) fn should_handle_text_shortcut(
        &self,
        key: KeyEvent,
        supports_text_paste_shortcut: bool,
    ) -> bool {
        supports_text_paste_shortcut
            && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && matches_image_paste_shortcut(key)
    }

    pub(super) fn record_shortcut_text(&mut self, text: &str) {
        self.pending_shortcut_paste = Some(PendingShortcutPaste {
            text: text.to_string(),
            expires_at: Instant::now() + PASTE_DEDUP_WINDOW,
        });
    }

    pub(super) fn clear(&mut self) {
        self.pending_shortcut_paste = None;
    }

    pub(super) fn decide_terminal_paste(&mut self, text: &str) -> TerminalPasteDecision {
        let Some(pending) = self.pending_shortcut_paste.as_ref() else {
            return TerminalPasteDecision::ForwardNoPending;
        };
        if Instant::now() > pending.expires_at {
            self.pending_shortcut_paste = None;
            return TerminalPasteDecision::ForwardExpiredPending;
        }
        if pending.text == text {
            self.pending_shortcut_paste = None;
            return TerminalPasteDecision::SuppressMatchingShortcutPaste;
        }
        TerminalPasteDecision::ForwardDifferentText
    }
}

impl TerminalPasteDecision {
    pub(super) fn should_forward(&self) -> bool {
        matches!(
            self,
            Self::ForwardNoPending | Self::ForwardExpiredPending | Self::ForwardDifferentText
        )
    }
}
