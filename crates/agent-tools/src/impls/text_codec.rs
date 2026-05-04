#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16LeBom,
    Utf16BeBom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedTextFile {
    pub(crate) text: String,
    pub(crate) encoding: TextEncoding,
    pub(crate) line_ending: LineEnding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TextDecodeFailure {
    UnsupportedEncoding,
    InvalidUtf8,
    InvalidUtf16,
}

impl TextDecodeFailure {
    pub(crate) fn render(&self) -> &'static str {
        match self {
            Self::UnsupportedEncoding => {
                "[file omitted: unsupported text encoding; safe UTF-8 editing is unavailable]"
            }
            Self::InvalidUtf8 => "[file omitted: invalid UTF-8 text; safe editing is unavailable]",
            Self::InvalidUtf16 => {
                "[file omitted: invalid UTF-16 text; safe editing is unavailable]"
            }
        }
    }
}

pub(crate) fn decode_text_file(bytes: &[u8]) -> Result<DecodedTextFile, TextDecodeFailure> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let text = std::str::from_utf8(rest)
            .map_err(|_| TextDecodeFailure::InvalidUtf8)?
            .to_string();
        return Ok(DecodedTextFile {
            line_ending: detect_line_ending(&text),
            text,
            encoding: TextEncoding::Utf8Bom,
        });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let text = decode_utf16_bytes(rest, true)?;
        return Ok(DecodedTextFile {
            line_ending: detect_line_ending(&text),
            text,
            encoding: TextEncoding::Utf16LeBom,
        });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let text = decode_utf16_bytes(rest, false)?;
        return Ok(DecodedTextFile {
            line_ending: detect_line_ending(&text),
            text,
            encoding: TextEncoding::Utf16BeBom,
        });
    }

    let text = std::str::from_utf8(bytes)
        .map_err(|err| {
            if err.valid_up_to() == 0 {
                TextDecodeFailure::UnsupportedEncoding
            } else {
                TextDecodeFailure::InvalidUtf8
            }
        })?
        .to_string();
    Ok(DecodedTextFile {
        line_ending: detect_line_ending(&text),
        text,
        encoding: TextEncoding::Utf8,
    })
}

pub(crate) fn encode_text_file(decoded: &DecodedTextFile, text: &str) -> Vec<u8> {
    match decoded.encoding {
        TextEncoding::Utf8 => text.as_bytes().to_vec(),
        TextEncoding::Utf8Bom => {
            let mut out = vec![0xEF, 0xBB, 0xBF];
            out.extend_from_slice(text.as_bytes());
            out
        }
        TextEncoding::Utf16LeBom => encode_utf16_with_bom(text, true),
        TextEncoding::Utf16BeBom => encode_utf16_with_bom(text, false),
    }
}

fn detect_line_ending(text: &str) -> LineEnding {
    if text.contains("\r\n") {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

fn decode_utf16_bytes(bytes: &[u8], little_endian: bool) -> Result<String, TextDecodeFailure> {
    if bytes.len() % 2 != 0 {
        return Err(TextDecodeFailure::InvalidUtf16);
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| TextDecodeFailure::InvalidUtf16)
}

fn encode_utf16_with_bom(text: &str, little_endian: bool) -> Vec<u8> {
    let mut out = if little_endian {
        vec![0xFF, 0xFE]
    } else {
        vec![0xFE, 0xFF]
    };
    for unit in text.encode_utf16() {
        let bytes = if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        };
        out.extend_from_slice(&bytes);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_and_preserves_metadata() {
        let decoded =
            decode_text_file(&[0xEF, 0xBB, 0xBF, b'a', b'\r', b'\n']).expect("decode utf8 bom");
        assert_eq!(decoded.text, "a\r\n");
        assert_eq!(decoded.encoding, TextEncoding::Utf8Bom);
        assert_eq!(decoded.line_ending, LineEnding::CrLf);
        assert_eq!(
            encode_text_file(&decoded, &decoded.text),
            vec![0xEF, 0xBB, 0xBF, b'a', b'\r', b'\n']
        );
    }

    #[test]
    fn decodes_utf16le_bom_and_reencodes() {
        let bytes = vec![0xFF, 0xFE, b'a', 0x00, b'\n', 0x00];
        let decoded = decode_text_file(&bytes).expect("decode utf16le");
        assert_eq!(decoded.text, "a\n");
        assert_eq!(decoded.encoding, TextEncoding::Utf16LeBom);
        assert_eq!(encode_text_file(&decoded, &decoded.text), bytes);
    }

    #[test]
    fn rejects_invalid_utf8_without_lossy_fallback() {
        let err = decode_text_file(&[0xD6, 0xD0, 0xCE, 0xC4]).expect_err("should reject");
        assert_eq!(err, TextDecodeFailure::UnsupportedEncoding);
    }
}
