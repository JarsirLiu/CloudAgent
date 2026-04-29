use anyhow::Result;

pub(crate) fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| anyhow::anyhow!("clipboard unavailable: {err}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| anyhow::anyhow!("clipboard write failed: {err}"))?;
    Ok(())
}
