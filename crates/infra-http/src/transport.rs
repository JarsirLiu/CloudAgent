use anyhow::{Context, Result};
use reqwest::Client;

pub fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent("cloudagent/0.1.0")
        .build()
        .context("failed to build HTTP client")
}

#[derive(Default)]
pub struct SseFrameDecoder {
    buffer: String,
}

impl SseFrameDecoder {
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut frames = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].trim().to_string();
            self.buffer = self.buffer[pos + 1..].to_string();
            if line.is_empty() || !line.starts_with("data:") {
                continue;
            }
            frames.push(line.trim_start_matches("data:").trim().to_string());
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::SseFrameDecoder;

    #[test]
    fn decoder_extracts_sse_data_lines() {
        let mut decoder = SseFrameDecoder::default();
        let frames = decoder.push_chunk(b"data: one\n\ndata: two\n\n");
        assert_eq!(frames, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn decoder_keeps_partial_lines_until_next_chunk() {
        let mut decoder = SseFrameDecoder::default();
        assert!(decoder.push_chunk(b"data: par").is_empty());
        let frames = decoder.push_chunk(b"tial\n\n");
        assert_eq!(frames, vec!["partial".to_string()]);
    }
}
