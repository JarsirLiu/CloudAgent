use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TurnInterruptedError;

impl Display for TurnInterruptedError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("turn interrupted by client")
    }
}

impl std::error::Error for TurnInterruptedError {}
