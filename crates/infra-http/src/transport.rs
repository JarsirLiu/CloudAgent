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
    utf8_remainder: Vec<u8>,
}

impl SseFrameDecoder {
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Vec<String> {
        append_utf8_safe(&mut self.buffer, &mut self.utf8_remainder, chunk);
        let mut frames = Vec::new();
        while let Some((pos, delimiter_len)) = find_sse_delimiter(&self.buffer) {
            let block = self.buffer[..pos].trim().to_string();
            self.buffer.drain(..pos + delimiter_len);
            if block.is_empty() {
                continue;
            }
            frames.push(block);
        }
        frames
    }
}

fn find_sse_delimiter(buffer: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;

    for (delimiter, len) in [("\r\n\r\n", 4usize), ("\n\n", 2usize)] {
        if let Some(pos) = buffer.find(delimiter) {
            if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                best = Some((pos, len));
            }
        }
    }

    best
}

fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, new_bytes: &[u8]) {
    let (owned, bytes): (Option<Vec<u8>>, &[u8]) = if remainder.is_empty() {
        (None, new_bytes)
    } else if remainder.len() > 3 {
        buffer.push_str(&String::from_utf8_lossy(remainder));
        remainder.clear();
        (None, new_bytes)
    } else {
        let mut combined = std::mem::take(remainder);
        combined.extend_from_slice(new_bytes);
        (Some(combined), &[])
    };
    let input = owned.as_deref().unwrap_or(bytes);

    let mut pos = 0;
    loop {
        match std::str::from_utf8(&input[pos..]) {
            Ok(s) => {
                buffer.push_str(s);
                return;
            }
            Err(err) => {
                let valid_up_to = pos + err.valid_up_to();
                let valid_slice = &input[pos..valid_up_to];
                match std::str::from_utf8(valid_slice) {
                    Ok(valid) => buffer.push_str(valid),
                    Err(_) => buffer.push_str(&String::from_utf8_lossy(valid_slice)),
                }

                if let Some(invalid_len) = err.error_len() {
                    buffer.push('\u{FFFD}');
                    pos = valid_up_to + invalid_len;
                } else {
                    *remainder = input[valid_up_to..].to_vec();
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SseFrameDecoder;

    #[test]
    fn decoder_extracts_sse_data_lines() {
        let mut decoder = SseFrameDecoder::default();
        let frames = decoder.push_chunk(b"data: one\n\ndata: two\n\n");
        assert_eq!(
            frames,
            vec!["data: one".to_string(), "data: two".to_string()]
        );
    }

    #[test]
    fn decoder_keeps_partial_lines_until_next_chunk() {
        let mut decoder = SseFrameDecoder::default();
        assert!(decoder.push_chunk(b"data: par").is_empty());
        let frames = decoder.push_chunk(b"tial\n\n");
        assert_eq!(frames, vec!["data: partial".to_string()]);
    }

    #[test]
    fn decoder_preserves_named_event_blocks() {
        let mut decoder = SseFrameDecoder::default();
        let frames = decoder.push_chunk(
            b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
        );
        assert_eq!(
            frames,
            vec![
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}"
                    .to_string()
            ]
        );
    }

    #[test]
    fn decoder_supports_crlf_delimiters() {
        let mut decoder = SseFrameDecoder::default();
        let frames = decoder.push_chunk(b"data: one\r\n\r\ndata: two\r\n\r\n");
        assert_eq!(
            frames,
            vec!["data: one".to_string(), "data: two".to_string()]
        );
    }

    #[test]
    fn decoder_preserves_utf8_across_chunk_boundaries() {
        let mut decoder = SseFrameDecoder::default();
        assert!(decoder.push_chunk(&"data: 你".as_bytes()[..8]).is_empty());
        let frames = decoder.push_chunk(&"data: 你\n\n".as_bytes()[8..]);
        assert_eq!(frames, vec!["data: 你".to_string()]);
    }
}
