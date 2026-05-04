mod error;
mod stream;
mod transport;

pub use error::HttpStreamError;
pub use stream::{SseFrameStream, spawn_sse_frame_stream};
pub use transport::{SseFrameDecoder, build_http_client};

pub fn crate_name() -> &'static str {
    "infra-http"
}
