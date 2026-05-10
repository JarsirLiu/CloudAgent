use std::time::Duration;

pub fn default_poll_interval() -> Duration {
    Duration::from_millis(100)
}
