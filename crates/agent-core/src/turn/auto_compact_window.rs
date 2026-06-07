use crate::model::ModelUsage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoCompactWindowSnapshot {
    pub ordinal: u64,
    pub prefill_input_tokens: Option<usize>,
}

impl Default for AutoCompactWindowSnapshot {
    fn default() -> Self {
        Self {
            ordinal: 1,
            prefill_input_tokens: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AutoCompactWindowPrefill {
    ServerObserved(usize),
    Estimated(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoCompactWindow {
    ordinal: u64,
    prefill_input_tokens: Option<AutoCompactWindowPrefill>,
}

impl AutoCompactWindow {
    pub fn new() -> Self {
        Self {
            ordinal: 1,
            prefill_input_tokens: None,
        }
    }

    pub fn from_snapshot(snapshot: AutoCompactWindowSnapshot) -> Self {
        Self {
            ordinal: snapshot.ordinal.max(1),
            prefill_input_tokens: snapshot
                .prefill_input_tokens
                .map(AutoCompactWindowPrefill::Estimated),
        }
    }

    pub fn start_next(&mut self) {
        self.ordinal = self.ordinal.saturating_add(1);
        self.clear_prefill();
    }

    pub fn clear_prefill(&mut self) {
        self.prefill_input_tokens = None;
    }

    pub fn set_estimated_prefill(&mut self, tokens: usize) {
        if matches!(
            self.prefill_input_tokens,
            Some(AutoCompactWindowPrefill::ServerObserved(_))
        ) {
            return;
        }
        self.prefill_input_tokens = Some(AutoCompactWindowPrefill::Estimated(tokens));
    }

    pub fn ensure_server_observed_prefill_from_usage(&mut self, usage: &ModelUsage) {
        if matches!(
            self.prefill_input_tokens,
            Some(AutoCompactWindowPrefill::ServerObserved(_))
        ) {
            return;
        }
        self.prefill_input_tokens = Some(AutoCompactWindowPrefill::ServerObserved(
            usage.input_tokens as usize,
        ));
    }

    pub fn snapshot(&self) -> AutoCompactWindowSnapshot {
        AutoCompactWindowSnapshot {
            ordinal: self.ordinal,
            prefill_input_tokens: match self.prefill_input_tokens {
                Some(AutoCompactWindowPrefill::ServerObserved(tokens))
                | Some(AutoCompactWindowPrefill::Estimated(tokens)) => Some(tokens),
                None => None,
            },
        }
    }
}

impl Default for AutoCompactWindow {
    fn default() -> Self {
        Self::new()
    }
}
