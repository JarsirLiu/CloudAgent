use crate::state::NoticeLevel;
use crate::state::notification::ToastNotification;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub(crate) struct NotificationStore {
    active_toast: Option<ToastNotification>,
}

impl NotificationStore {
    pub(crate) fn push_toast(&mut self, level: NoticeLevel, message: String) {
        self.active_toast = Some(ToastNotification::new(level, message));
    }

    pub(crate) fn active_toast(&self) -> Option<&ToastNotification> {
        self.active_toast.as_ref()
    }

    pub(crate) fn handle_tick(&mut self) -> bool {
        if self
            .active_toast
            .as_ref()
            .is_some_and(|toast| Instant::now() >= toast.expires_at)
        {
            self.active_toast = None;
            return true;
        }
        false
    }

    #[cfg(test)]
    pub(crate) fn expire_toast_for_test(&mut self) {
        if let Some(toast) = self.active_toast.as_mut() {
            toast.expires_at = Instant::now();
        }
    }
}
