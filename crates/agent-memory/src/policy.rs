pub fn sanitize_memory_text(raw: &str, max_chars: usize) -> String {
    let text = raw.trim();
    if text.len() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}
