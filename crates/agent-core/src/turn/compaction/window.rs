use crate::model::ModelUsage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutoCompactWindowSnapshot {
    pub ordinal: u64,
    pub prefill_input_tokens: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutoCompactWindow {
    ordinal: u64,
    prefill_input_tokens: Option<usize>,
    server_observed: bool,
}

impl AutoCompactWindow {
    pub fn new() -> Self {
        Self {
            ordinal: 1,
            prefill_input_tokens: None,
            server_observed: false,
        }
    }

    pub fn from_snapshot(snapshot: AutoCompactWindowSnapshot) -> Self {
        Self {
            ordinal: snapshot.ordinal.max(1),
            prefill_input_tokens: snapshot.prefill_input_tokens,
            server_observed: false,
        }
    }

    pub fn snapshot(&self) -> AutoCompactWindowSnapshot {
        AutoCompactWindowSnapshot {
            ordinal: self.ordinal.max(1),
            prefill_input_tokens: self.prefill_input_tokens,
        }
    }

    pub fn set_estimated_prefill(&mut self, input_tokens: usize) {
        if !self.server_observed {
            self.prefill_input_tokens = Some(input_tokens);
        }
    }

    pub fn ensure_server_observed_prefill_from_usage(&mut self, usage: &ModelUsage) {
        if !self.server_observed {
            self.prefill_input_tokens = Some(usage.input_tokens as usize);
            self.server_observed = true;
        }
    }

    pub fn start_next(&mut self) {
        self.ordinal = self.ordinal.saturating_add(1).max(1);
        self.prefill_input_tokens = None;
        self.server_observed = false;
    }
}
