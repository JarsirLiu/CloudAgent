use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpStreamError {
    FirstFrameTimeout,
    IdleTimeout,
    ClosedBeforeCompletion,
    Transport(String),
}

impl fmt::Display for HttpStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FirstFrameTimeout => f.write_str("timed out waiting for first stream frame"),
            Self::IdleTimeout => f.write_str("idle timeout waiting for stream data"),
            Self::ClosedBeforeCompletion => f.write_str("stream closed before completion"),
            Self::Transport(message) => write!(f, "stream transport error: {message}"),
        }
    }
}

impl std::error::Error for HttpStreamError {}
