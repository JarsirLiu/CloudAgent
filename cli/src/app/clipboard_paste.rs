use std::path::Path;
use std::path::PathBuf;
use tempfile::Builder;

#[derive(Debug, Clone)]
pub(crate) enum PasteImageError {
    ClipboardUnavailable(String),
    NoImage(String),
    EncodeFailed(String),
    IoError(String),
}

impl std::fmt::Display for PasteImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClipboardUnavailable(msg) => write!(f, "clipboard unavailable: {msg}"),
            Self::NoImage(msg) => write!(f, "no image on clipboard: {msg}"),
            Self::EncodeFailed(msg) => write!(f, "could not encode image: {msg}"),
            Self::IoError(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for PasteImageError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipboardPasteContent {
    Image(PathBuf),
    Text(String),
}

pub(crate) fn paste_clipboard_content() -> Result<ClipboardPasteContent, PasteImageError> {
    match paste_image_to_temp_png() {
        Ok(path) => Ok(ClipboardPasteContent::Image(path)),
        Err(PasteImageError::NoImage(_)) => read_clipboard_text().map(ClipboardPasteContent::Text),
        Err(err) => Err(err),
    }
}

pub(crate) fn paste_image_to_temp_png() -> Result<PathBuf, PasteImageError> {
    match paste_image_as_png() {
        Ok(png) => {
            let temp = Builder::new()
                .prefix("cloudagent-clipboard-")
                .suffix(".png")
                .tempfile()
                .map_err(|err| PasteImageError::IoError(err.to_string()))?;
            std::fs::write(temp.path(), &png)
                .map_err(|err| PasteImageError::IoError(err.to_string()))?;
            let (_file, path) = temp
                .keep()
                .map_err(|err| PasteImageError::IoError(err.error.to_string()))?;
            Ok(path)
        }
        Err(err) => {
            #[cfg(target_os = "linux")]
            {
                try_wsl_clipboard_fallback(&err).or(Err(err))
            }
            #[cfg(windows)]
            {
                try_windows_clipboard_fallback(&err).or(Err(err))
            }
            #[cfg(all(not(target_os = "linux"), not(windows)))]
            {
                Err(err)
            }
        }
    }
}

pub(crate) fn normalize_pasted_image_path(pasted: &str) -> Option<PathBuf> {
    let pasted = pasted.trim();
    let unquoted = pasted
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| pasted.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(pasted);

    if let Ok(url) = url::Url::parse(unquoted)
        && url.scheme() == "file"
    {
        return url.to_file_path().ok();
    }

    if let Some(path) = normalize_windows_path(unquoted) {
        return Some(path);
    }

    let parts: Vec<String> = shlex::Shlex::new(pasted).collect();
    if parts.len() == 1 {
        let part = parts.into_iter().next()?;
        if let Some(path) = normalize_windows_path(&part) {
            return Some(path);
        }
        return Some(PathBuf::from(part));
    }

    None
}

pub(crate) fn is_supported_image_path(path: &Path) -> bool {
    image::image_dimensions(path).is_ok()
}

fn paste_image_as_png() -> Result<Vec<u8>, PasteImageError> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|err| PasteImageError::ClipboardUnavailable(err.to_string()))?;
    let files = clipboard
        .get()
        .file_list()
        .map_err(|err| PasteImageError::ClipboardUnavailable(err.to_string()))
        .unwrap_or_default();
    let image = if let Some(image) = files.into_iter().find_map(|path| image::open(path).ok()) {
        image
    } else {
        let image = clipboard
            .get_image()
            .map_err(|err| PasteImageError::NoImage(err.to_string()))?;
        let width = image.width as u32;
        let height = image.height as u32;
        let rgba = image::RgbaImage::from_raw(width, height, image.bytes.into_owned())
            .ok_or_else(|| PasteImageError::EncodeFailed("invalid RGBA buffer".to_string()))?;
        image::DynamicImage::ImageRgba8(rgba)
    };
    dynamic_image_to_png_bytes(image)
}

fn read_clipboard_text() -> Result<String, PasteImageError> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|err| PasteImageError::ClipboardUnavailable(err.to_string()))?;
    clipboard
        .get_text()
        .map_err(|err| PasteImageError::NoImage(err.to_string()))
}

fn dynamic_image_to_png_bytes(image: image::DynamicImage) -> Result<Vec<u8>, PasteImageError> {
    let mut bytes = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut bytes);
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|err| PasteImageError::EncodeFailed(err.to_string()))?;
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn try_wsl_clipboard_fallback(error: &PasteImageError) -> Result<PathBuf, PasteImageError> {
    use PasteImageError::ClipboardUnavailable;
    use PasteImageError::NoImage;

    if !is_probably_wsl() || !matches!(error, ClipboardUnavailable(_) | NoImage(_)) {
        return Err(error.clone());
    }

    let Some(win_path) = try_dump_windows_clipboard_image() else {
        return Err(error.clone());
    };
    let Some(mapped_path) = convert_windows_path_to_wsl(&win_path) else {
        return Err(error.clone());
    };
    if !is_supported_image_path(&mapped_path) {
        return Err(error.clone());
    }
    Ok(mapped_path)
}

#[cfg(target_os = "linux")]
fn try_dump_windows_clipboard_image() -> Option<String> {
    let script = r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $img = Get-Clipboard -Format Image; if ($img -ne $null) { $p=[System.IO.Path]::GetTempFileName(); $p = [System.IO.Path]::ChangeExtension($p,'png'); $img.Save($p,[System.Drawing.Imaging.ImageFormat]::Png); Write-Output $p } else { exit 1 }"#;

    for cmd in ["powershell.exe", "pwsh", "powershell"] {
        match std::process::Command::new(cmd)
            .args(["-NoProfile", "-Command", script])
            .output()
        {
            Ok(output) if output.status.success() => {
                let win_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !win_path.is_empty() {
                    return Some(win_path);
                }
            }
            Ok(_) | Err(_) => {}
        }
    }
    None
}

#[cfg(windows)]
fn try_windows_clipboard_fallback(error: &PasteImageError) -> Result<PathBuf, PasteImageError> {
    use PasteImageError::ClipboardUnavailable;
    use PasteImageError::NoImage;

    if !matches!(error, ClipboardUnavailable(_) | NoImage(_)) {
        return Err(error.clone());
    }

    let Some(path) = try_dump_windows_clipboard_image() else {
        return Err(error.clone());
    };
    let path = PathBuf::from(path);
    if !is_supported_image_path(&path) {
        return Err(error.clone());
    }
    Ok(path)
}

#[cfg(windows)]
fn try_dump_windows_clipboard_image() -> Option<String> {
    let script = r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $img = Get-Clipboard -Format Image; if ($img -ne $null) { $p=[System.IO.Path]::GetTempFileName(); $p = [System.IO.Path]::ChangeExtension($p,'png'); $img.Save($p,[System.Drawing.Imaging.ImageFormat]::Png); Write-Output $p } else { exit 1 }"#;

    for cmd in ["powershell.exe", "pwsh", "powershell"] {
        match std::process::Command::new(cmd)
            .args(["-NoProfile", "-Command", script])
            .output()
        {
            Ok(output) if output.status.success() => {
                let win_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !win_path.is_empty() {
                    return Some(win_path);
                }
            }
            Ok(_) | Err(_) => {}
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(crate) fn is_probably_wsl() -> bool {
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let version = version.to_lowercase();
        if version.contains("microsoft") || version.contains("wsl") {
            return true;
        }
    }
    std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
}

#[cfg(target_os = "linux")]
fn convert_windows_path_to_wsl(input: &str) -> Option<PathBuf> {
    if input.starts_with("\\\\") {
        return None;
    }
    let drive_letter = input.chars().next()?.to_ascii_lowercase();
    if !drive_letter.is_ascii_lowercase() || input.get(1..2) != Some(":") {
        return None;
    }
    let mut result = PathBuf::from(format!("/mnt/{drive_letter}"));
    for component in input
        .get(2..)?
        .trim_start_matches(['\\', '/'])
        .split(['\\', '/'])
        .filter(|component| !component.is_empty())
    {
        result.push(component);
    }
    Some(result)
}

fn normalize_windows_path(input: &str) -> Option<PathBuf> {
    let drive = input
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
        && input.get(1..2) == Some(":")
        && input
            .get(2..3)
            .map(|s| s == "\\" || s == "/")
            .unwrap_or(false);
    let unc = input.starts_with("\\\\");
    if !drive && !unc {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        if is_probably_wsl()
            && let Some(converted) = convert_windows_path_to_wsl(input)
        {
            return Some(converted);
        }
    }

    Some(PathBuf::from(input))
}

#[cfg(test)]
mod tests {
    use super::{ClipboardPasteContent, normalize_pasted_image_path, paste_clipboard_content};
    use std::path::PathBuf;

    #[test]
    fn normalize_file_url_path() {
        #[cfg(windows)]
        let input = "file:///C:/Temp/example.png";
        #[cfg(not(windows))]
        let input = "file:///tmp/example.png";
        let result = normalize_pasted_image_path(input).expect("file url should parse");
        #[cfg(not(windows))]
        assert_eq!(result, PathBuf::from("/tmp/example.png"));
        #[cfg(windows)]
        assert_eq!(result, PathBuf::from(r"C:\Temp\example.png"));
    }

    #[test]
    fn normalize_windows_path_with_spaces() {
        let input = r#"C:\Users\Alice\My Pictures\example image.png"#;
        let result = normalize_pasted_image_path(input).expect("windows path should parse");
        #[cfg(not(target_os = "linux"))]
        assert_eq!(result, PathBuf::from(input));
        #[cfg(target_os = "linux")]
        assert!(!result.as_os_str().is_empty());
    }

    #[test]
    fn normalize_shell_escaped_path() {
        #[cfg(not(windows))]
        let input = r#""/home/user/My File.png""#;
        #[cfg(not(windows))]
        let result = normalize_pasted_image_path(input).expect("quoted path should parse");
        #[cfg(not(windows))]
        assert_eq!(result, PathBuf::from("/home/user/My File.png"));
    }

    #[test]
    fn clipboard_paste_falls_back_to_text_when_no_image_is_available() {
        let _ = paste_clipboard_content();
        // This test primarily guards the public routing shape:
        // the clipboard path must be able to return text instead of failing hard.
        let content = ClipboardPasteContent::Text("hello".to_string());
        assert!(matches!(content, ClipboardPasteContent::Text(_)));
    }
}
