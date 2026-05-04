use crate::impls::text_codec::{decode_text_file, TextDecodeFailure};
use anyhow::{Context, Result};
use std::path::Path;

const BINARY_SNIFF_BYTES: usize = 8192;
const DEFAULT_MAX_FILE_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct TextReadOptions {
    pub(crate) start_line: usize,
    pub(crate) max_lines: usize,
    pub(crate) max_chars: usize,
    pub(crate) max_file_bytes: usize,
    pub(crate) include_line_numbers: bool,
}

impl TextReadOptions {
    pub(crate) fn for_single_file(
        max_chars: usize,
        start_line: Option<usize>,
        max_lines: Option<usize>,
    ) -> Self {
        Self {
            start_line: start_line.unwrap_or(1).max(1),
            max_lines: max_lines.unwrap_or(200).clamp(1, 5_000),
            max_chars: max_chars.max(128),
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            include_line_numbers: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextReadResult {
    pub(crate) rendered: String,
    pub(crate) source_char_count: usize,
    pub(crate) end_line: Option<usize>,
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum TextReadFailure {
    Binary,
    TooLarge { bytes: usize, limit: usize },
    UnsupportedEncoding(TextDecodeFailure),
}

impl TextReadFailure {
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Binary => "[binary file omitted]".to_string(),
            Self::TooLarge { bytes, limit } => {
                format!("[file omitted: {bytes} bytes exceeds {limit} byte read limit]")
            }
            Self::UnsupportedEncoding(reason) => reason.render().to_string(),
        }
    }
}

pub(crate) async fn read_text_snippet(
    path: &Path,
    options: &TextReadOptions,
) -> Result<Result<TextReadResult, TextReadFailure>> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read file {}", path.display()))?;

    if looks_binary(&bytes) {
        return Ok(Err(TextReadFailure::Binary));
    }
    if bytes.len() > options.max_file_bytes {
        return Ok(Err(TextReadFailure::TooLarge {
            bytes: bytes.len(),
            limit: options.max_file_bytes,
        }));
    }

    let decoded = match decode_text_file(&bytes) {
        Ok(decoded) => decoded,
        Err(err) => return Ok(Err(TextReadFailure::UnsupportedEncoding(err))),
    };
    let text = decoded.text;
    let source_char_count = text.chars().count();
    let mut truncated = false;
    let mut rendered_lines = Vec::new();
    let mut end_line = None;

    for (idx, line) in text
        .lines()
        .enumerate()
        .skip(options.start_line.saturating_sub(1))
    {
        if rendered_lines.len() >= options.max_lines {
            truncated = true;
            break;
        }
        let line_number = idx + 1;
        rendered_lines.push(render_line(line_number, line, options.include_line_numbers));
        end_line = Some(line_number);
    }

    if rendered_lines.is_empty() && options.start_line > 1 {
        rendered_lines.push(format!(
            "[no content at or after line {}]",
            options.start_line
        ));
    }

    let mut rendered = rendered_lines.join("\n");
    let rendered_char_count = rendered.chars().count();
    if rendered_char_count > options.max_chars {
        rendered = rendered.chars().take(options.max_chars).collect::<String>();
        truncated = true;
    }
    if truncated {
        if !rendered.is_empty() {
            rendered.push_str("\n");
        }
        rendered.push_str("[truncated]");
    }

    Ok(Ok(TextReadResult {
        rendered,
        source_char_count,
        end_line,
        truncated,
    }))
}

fn render_line(line_number: usize, line: &str, include_line_numbers: bool) -> String {
    if include_line_numbers {
        format!("{line_number:>6}  {line}")
    } else {
        line.to_string()
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
    bytes[..sniff_len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn text_read_adds_line_numbers_and_truncation_marker() {
        let path = temp_file_path("text_read_line_numbers.txt");
        tokio::fs::write(&path, "alpha\nbeta\ngamma\n")
            .await
            .expect("write");

        let result = read_text_snippet(
            &path,
            &TextReadOptions {
                start_line: 2,
                max_lines: 1,
                max_chars: 200,
                max_file_bytes: DEFAULT_MAX_FILE_BYTES,
                include_line_numbers: true,
            },
        )
        .await
        .expect("read ok")
        .expect("text file");

        assert_eq!(result.rendered, "     2  beta\n[truncated]");
        assert!(result.truncated);
    }

    #[tokio::test]
    async fn text_read_marks_binary_files() {
        let path = temp_file_path("text_read_binary.bin");
        tokio::fs::write(&path, [0_u8, 159, 146, 150])
            .await
            .expect("write");

        let result = read_text_snippet(&path, &TextReadOptions::for_single_file(200, None, None))
            .await
            .expect("read ok");

        assert!(matches!(result, Err(TextReadFailure::Binary)));
    }

    #[tokio::test]
    async fn text_read_rejects_unsupported_encoding_without_lossy_decode() {
        let path = temp_file_path("text_read_unsupported_encoding.txt");
        tokio::fs::write(&path, [0xD6_u8, 0xD0, 0xCE, 0xC4])
            .await
            .expect("write");

        let result = read_text_snippet(&path, &TextReadOptions::for_single_file(200, None, None))
            .await
            .expect("read ok");

        assert!(matches!(
            result,
            Err(TextReadFailure::UnsupportedEncoding(_))
        ));
    }

    fn temp_file_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        path.push(format!("cloudagent_{name}_{stamp}"));
        path
    }
}
