use unicode_width::UnicodeWidthStr;

pub(crate) fn display_width(value: &str) -> usize {
    if !value.contains('\x1B') {
        return UnicodeWidthStr::width(value);
    }

    let mut visible = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1B' && chars.clone().next() == Some(']') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\x07' {
                    break;
                }
            }
            continue;
        }
        visible.push(ch);
    }
    UnicodeWidthStr::width(visible.as_str())
}

#[cfg(test)]
mod tests {
    use super::display_width;

    #[test]
    fn ignores_osc_sequences_when_measuring_width() {
        let linked = "\x1b]8;;https://example.com\x07hello\x1b]8;;\x07";
        assert_eq!(display_width(linked), 5);
    }
}
