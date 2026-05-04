use infra_http::HttpStreamError;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRequestError {
    Http {
        status: u16,
        body: String,
    },
    Transport {
        message: String,
    },
    Protocol {
        message: String,
    },
    Provider {
        code: Option<String>,
        message: String,
        retry_after_ms: Option<u64>,
    },
}

impl fmt::Display for ProviderRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { status, body } => {
                write!(f, "provider request http error {status}: {body}")
            }
            Self::Transport { message } => write!(f, "provider request transport error: {message}"),
            Self::Protocol { message } => write!(f, "provider request protocol error: {message}"),
            Self::Provider { code, message, .. } => {
                if let Some(code) = code {
                    write!(f, "provider request error {code}: {message}")
                } else {
                    write!(f, "provider request error: {message}")
                }
            }
        }
    }
}

impl std::error::Error for ProviderRequestError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStreamError {
    IdleTimeout,
    ClosedBeforeCompletion,
    Http {
        status: u16,
        body: String,
    },
    Transport {
        message: String,
    },
    Protocol {
        message: String,
    },
    Provider {
        code: Option<String>,
        message: String,
        retry_after_ms: Option<u64>,
    },
}

impl fmt::Display for ProviderStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdleTimeout => f.write_str("provider stream idle timeout"),
            Self::ClosedBeforeCompletion => f.write_str("provider stream closed before completion"),
            Self::Http { status, body } => {
                write!(f, "provider stream http error {status}: {body}")
            }
            Self::Transport { message } => write!(f, "provider transport error: {message}"),
            Self::Protocol { message } => write!(f, "provider protocol error: {message}"),
            Self::Provider { code, message, .. } => {
                if let Some(code) = code {
                    write!(f, "provider error {code}: {message}")
                } else {
                    write!(f, "provider error: {message}")
                }
            }
        }
    }
}

impl std::error::Error for ProviderStreamError {}

impl From<HttpStreamError> for ProviderStreamError {
    fn from(value: HttpStreamError) -> Self {
        match value {
            HttpStreamError::IdleTimeout => Self::IdleTimeout,
            HttpStreamError::ClosedBeforeCompletion => Self::ClosedBeforeCompletion,
            HttpStreamError::Transport(message) => Self::Transport { message },
        }
    }
}
