use std::time::{Duration, Instant};

const PASTE_ECHO_SUPPRESS_WINDOW: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(crate) struct PasteEchoGuard {
    pending: Option<PendingPasteEcho>,
}

struct PendingPasteEcho {
    expected: Vec<char>,
    matched: usize,
    expires_at: Instant,
}

impl PasteEchoGuard {
    pub(crate) fn arm(&mut self, text: &str) {
        let expected: Vec<char> = text.chars().collect();
        if expected.is_empty() {
            self.pending = None;
            return;
        }
        self.pending = Some(PendingPasteEcho {
            expected,
            matched: 0,
            expires_at: Instant::now() + PASTE_ECHO_SUPPRESS_WINDOW,
        });
    }

    pub(crate) fn should_ignore_char(&mut self, ch: char) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        if Instant::now() > pending.expires_at {
            self.pending = None;
            return false;
        }

        if pending
            .expected
            .get(pending.matched)
            .is_some_and(|expected| *expected == ch)
        {
            pending.matched += 1;
            if pending.matched >= pending.expected.len() {
                self.pending = None;
            }
            return true;
        }

        self.pending = None;
        false
    }

    pub(crate) fn clear(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::PasteEchoGuard;

    #[test]
    fn ignores_matching_echo_sequence_once() {
        let mut guard = PasteEchoGuard::default();
        guard.arm("abc");
        assert!(guard.should_ignore_char('a'));
        assert!(guard.should_ignore_char('b'));
        assert!(guard.should_ignore_char('c'));
        assert!(!guard.should_ignore_char('c'));
    }

    #[test]
    fn clears_on_non_matching_char() {
        let mut guard = PasteEchoGuard::default();
        guard.arm("abc");
        assert!(!guard.should_ignore_char('x'));
        assert!(!guard.should_ignore_char('a'));
    }
}
