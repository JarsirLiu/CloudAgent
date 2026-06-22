use crate::state::NoticeLevel;
use std::time::Instant;

pub(crate) const TOAST_TTL_SECS: u64 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToastNotification {
    pub(crate) level: NoticeLevel,
    pub(crate) message: String,
    pub(crate) expires_at: Instant,
}

impl ToastNotification {
    pub(crate) fn new(level: NoticeLevel, message: String) -> Self {
        Self {
            level,
            message,
            expires_at: Instant::now() + std::time::Duration::from_secs(TOAST_TTL_SECS),
        }
    }
}
