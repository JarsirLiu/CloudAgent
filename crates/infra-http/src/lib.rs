mod transport;

pub use transport::{SseFrameDecoder, build_http_client};

pub fn crate_name() -> &'static str {
    "infra-http"
}
