use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorDepth {
    NoColor,
    TrueColor,
    Ansi256,
    Ansi16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundTone {
    Dark,
    Light,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalCapabilities {
    pub(crate) color_depth: ColorDepth,
    pub(crate) supports_synchronized_update: bool,
    pub(crate) background_tone: BackgroundTone,
}

impl TerminalCapabilities {
    pub(crate) fn detect() -> Self {
        if let Some(mode) = std::env::var_os("CLOUDAGENT_COLOR_MODE")
            && let Some(depth) = parse_color_mode_override(mode.to_string_lossy().trim())
        {
            return Self {
                color_depth: depth,
                supports_synchronized_update: detect_synchronized_update_support(),
                background_tone: detect_background_tone(),
            };
        }

        if let Some(value) = std::env::var_os("CLOUDAGENT_COLOR_DEPTH")
            && let Some(depth) = parse_color_depth_override(value.to_string_lossy().trim())
        {
            return Self {
                color_depth: depth,
                supports_synchronized_update: detect_synchronized_update_support(),
                background_tone: detect_background_tone(),
            };
        }

        if force_color_disabled() {
            return Self {
                color_depth: ColorDepth::NoColor,
                supports_synchronized_update: detect_synchronized_update_support(),
                background_tone: detect_background_tone(),
            };
        }
        if force_color_enabled() {
            return Self {
                color_depth: detect_terminal_color_depth(),
                supports_synchronized_update: detect_synchronized_update_support(),
                background_tone: detect_background_tone(),
            };
        }
        if std::env::var_os("NO_COLOR").is_some() {
            return Self {
                color_depth: ColorDepth::NoColor,
                supports_synchronized_update: detect_synchronized_update_support(),
                background_tone: detect_background_tone(),
            };
        }
        Self {
            color_depth: detect_terminal_color_depth(),
            supports_synchronized_update: detect_synchronized_update_support(),
            background_tone: detect_background_tone(),
        }
    }
}

pub fn apply_color_cli_preference(args: &[std::ffi::OsString]) {
    let preference = color_cli_preference(args);
    match preference {
        Some(ColorCliPreference::Always) => unsafe {
            std::env::set_var("CLOUDAGENT_COLOR_MODE", "always");
        },
        Some(ColorCliPreference::Never) => unsafe {
            std::env::set_var("CLOUDAGENT_COLOR_MODE", "never");
        },
        Some(ColorCliPreference::Auto) => unsafe {
            std::env::set_var("CLOUDAGENT_COLOR_MODE", "auto");
        },
        None => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorCliPreference {
    Auto,
    Always,
    Never,
}

fn color_cli_preference(args: &[std::ffi::OsString]) -> Option<ColorCliPreference> {
    if args.iter().any(|arg| arg == "--no-color") {
        return Some(ColorCliPreference::Never);
    }
    let value = arg_value(args, "--color")?;
    match value.to_string_lossy().trim().to_ascii_lowercase().as_str() {
        "always" => Some(ColorCliPreference::Always),
        "never" => Some(ColorCliPreference::Never),
        "auto" => Some(ColorCliPreference::Auto),
        _ => None,
    }
}

fn detect_terminal_color_depth() -> ColorDepth {
    if is_apple_terminal() {
        return ColorDepth::Ansi256;
    }

    effective_color_depth(
        stdout_color_depth(),
        has_truecolor_terminal_signal(),
        is_known_truecolor_term_program(),
    )
}

pub(crate) fn prepare_terminal_color_output() {
    let _ = windows_console_supports_or_enables_virtual_terminal();
}

fn stdout_color_depth() -> ColorDepth {
    match supports_color::on_cached(supports_color::Stream::Stdout) {
        Some(level) if level.has_16m => ColorDepth::TrueColor,
        Some(level) if level.has_256 => ColorDepth::Ansi256,
        Some(_) => ColorDepth::Ansi16,
        None => env_color_depth_fallback(),
    }
}

fn effective_color_depth(
    stdout_depth: ColorDepth,
    has_windows_terminal_session: bool,
    has_known_truecolor_term_program: bool,
) -> ColorDepth {
    if has_windows_terminal_session {
        return ColorDepth::TrueColor;
    }
    if stdout_depth == ColorDepth::Ansi16 && has_known_truecolor_term_program {
        return ColorDepth::TrueColor;
    }
    stdout_depth
}

fn env_color_depth_fallback() -> ColorDepth {
    if matches_colorterm("truecolor") || matches_colorterm("24bit") {
        return ColorDepth::TrueColor;
    }
    if has_truecolor_terminal_signal() {
        return ColorDepth::TrueColor;
    }
    if is_known_truecolor_term_program() {
        return ColorDepth::TrueColor;
    }
    if let Some(term_program) = env_lowercase("TERM_PROGRAM")
        && term_program.contains("windows terminal")
    {
        return ColorDepth::TrueColor;
    }
    if let Some(term) = env_lowercase("TERM") {
        if term == "dumb" {
            return ColorDepth::Ansi16;
        }
        if term.contains("truecolor") || term.contains("24bit") {
            return ColorDepth::TrueColor;
        }
        if term.contains("256") {
            return ColorDepth::Ansi256;
        }
        return ColorDepth::Ansi256;
    }
    ColorDepth::Ansi16
}

fn is_known_truecolor_term_program() -> bool {
    env_lowercase("TERM_PROGRAM")
        .map(|term_program| {
            let normalized = normalize_terminal_name(&term_program);
            ["iterm", "wezterm", "vscode", "warp", "hyper", "kitty"]
                .iter()
                .any(|needle| term_program.contains(needle))
                || normalized == "windowsterminal"
        })
        .unwrap_or(false)
}

fn is_windows_terminal_session() -> bool {
    std::env::var_os("WT_SESSION").is_some()
        || std::env::var_os("WT_PROFILE_ID").is_some()
        || env_lowercase("TERM_PROGRAM")
            .map(|term_program| normalize_terminal_name(&term_program) == "windowsterminal")
            .unwrap_or(false)
}

fn has_truecolor_terminal_signal() -> bool {
    is_windows_terminal_session() || windows_console_supports_or_enables_virtual_terminal()
}

#[cfg(windows)]
fn windows_console_supports_or_enables_virtual_terminal() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
        SetConsoleMode,
    };

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }

        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
            return true;
        }

        // Some Windows Terminal cmd profiles do not expose WT_* environment
        // variables to child processes. Enabling VT is the reliable capability
        // probe for those sessions, and it is required before ANSI rendering.
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn windows_console_supports_or_enables_virtual_terminal() -> bool {
    false
}

fn normalize_terminal_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '.'))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn detect_synchronized_update_support() -> bool {
    !is_apple_terminal()
}

pub(crate) fn adapt_color(color: Color, capabilities: TerminalCapabilities) -> Color {
    let color = match (color, capabilities.background_tone) {
        (Color::Rgb(r, g, b), BackgroundTone::Light) => {
            let (r, g, b) = adapt_rgb_for_light_background((r, g, b), false);
            Color::Rgb(r, g, b)
        }
        _ => color,
    };
    match (color, capabilities.color_depth) {
        (_, ColorDepth::NoColor) => Color::Reset,
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => nearest_ansi16(r, g, b),
        _ => color,
    }
}

pub(crate) fn adapt_bg(color: Color, capabilities: TerminalCapabilities) -> Color {
    let color = match (color, capabilities.background_tone) {
        (Color::Rgb(r, g, b), BackgroundTone::Light) => {
            let (r, g, b) = adapt_rgb_for_light_background((r, g, b), true);
            Color::Rgb(r, g, b)
        }
        _ => color,
    };
    match (color, capabilities.color_depth) {
        (_, ColorDepth::NoColor) => Color::Reset,
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => nearest_ansi16(r, g, b),
        _ => color,
    }
}

fn env_lowercase(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.to_ascii_lowercase())
}

fn matches_colorterm(needle: &str) -> bool {
    env_lowercase("COLORTERM")
        .map(|value| value.contains(needle))
        .unwrap_or(false)
}

fn is_apple_terminal() -> bool {
    env_lowercase("TERM_PROGRAM")
        .map(|value| value == "apple_terminal")
        .unwrap_or(false)
}

fn detect_background_tone() -> BackgroundTone {
    let Some(value) = std::env::var("COLORFGBG").ok() else {
        return BackgroundTone::Unknown;
    };
    let Some(last) = value.split(';').next_back() else {
        return BackgroundTone::Unknown;
    };
    let Ok(index) = last.trim().parse::<u8>() else {
        return BackgroundTone::Unknown;
    };
    let (r, g, blue) = ansi256_rgb(index);
    let luminance = perceived_luminance((r, g, blue));
    if luminance >= 160.0 {
        BackgroundTone::Light
    } else {
        BackgroundTone::Dark
    }
}

fn adapt_rgb_for_light_background(rgb: (u8, u8, u8), is_background: bool) -> (u8, u8, u8) {
    let luminance = perceived_luminance(rgb);
    if is_background {
        if luminance >= 190.0 {
            return rgb;
        }
        return blend_rgb(rgb, (255, 255, 255), 0.82);
    }
    if luminance <= 145.0 {
        return rgb;
    }
    blend_rgb(rgb, (32, 36, 44), 0.6)
}

fn blend_rgb(from: (u8, u8, u8), to: (u8, u8, u8), ratio: f32) -> (u8, u8, u8) {
    let blend = |start: u8, end: u8| -> u8 {
        let mixed = f32::from(start) + (f32::from(end) - f32::from(start)) * ratio;
        mixed.round().clamp(0.0, 255.0) as u8
    };
    (
        blend(from.0, to.0),
        blend(from.1, to.1),
        blend(from.2, to.2),
    )
}

fn perceived_luminance(rgb: (u8, u8, u8)) -> f32 {
    0.2126 * f32::from(rgb.0) + 0.7152 * f32::from(rgb.1) + 0.0722 * f32::from(rgb.2)
}

fn parse_color_depth_override(value: &str) -> Option<ColorDepth> {
    match value.to_ascii_lowercase().as_str() {
        "none" | "never" => Some(ColorDepth::NoColor),
        "truecolor" | "24bit" | "24" => Some(ColorDepth::TrueColor),
        "256" | "ansi256" => Some(ColorDepth::Ansi256),
        "16" | "ansi16" => Some(ColorDepth::Ansi16),
        "auto" => None,
        _ => None,
    }
}

fn parse_color_mode_override(value: &str) -> Option<ColorDepth> {
    match value.to_ascii_lowercase().as_str() {
        "never" => Some(ColorDepth::NoColor),
        "always" => Some(ColorDepth::TrueColor),
        "auto" => None,
        _ => None,
    }
}

fn force_color_enabled() -> bool {
    std::env::var("FORCE_COLOR")
        .ok()
        .map(|value| value.trim() != "0")
        .unwrap_or(false)
}

fn force_color_disabled() -> bool {
    std::env::var("FORCE_COLOR")
        .ok()
        .map(|value| value.trim() == "0")
        .unwrap_or(false)
}

fn arg_value(args: &[std::ffi::OsString], name: &str) -> Option<std::ffi::OsString> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    let cube = 16 + 36 * to_ansi_cube(r) + 6 * to_ansi_cube(g) + to_ansi_cube(b);
    let gray = to_ansi_gray(r, g, b);

    if color_distance_sq(r, g, b, ansi256_rgb(cube))
        <= color_distance_sq(r, g, b, ansi256_rgb(gray))
    {
        cube
    } else {
        gray
    }
}

fn to_ansi_cube(channel: u8) -> u8 {
    if channel < 48 {
        0
    } else if channel < 114 {
        1
    } else {
        ((channel - 35) / 40).min(5)
    }
}

fn to_ansi_gray(r: u8, g: u8, b: u8) -> u8 {
    let avg = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as i16;
    let gray_index = (((avg - 8).max(0)) / 10).min(23) as u8;
    232 + gray_index
}

fn ansi256_rgb(index: u8) -> (u8, u8, u8) {
    const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let normalized = index - 16;
            let r = CUBE_LEVELS[(normalized / 36) as usize];
            let g = CUBE_LEVELS[((normalized % 36) / 6) as usize];
            let b = CUBE_LEVELS[(normalized % 6) as usize];
            (r, g, b)
        }
        232..=255 => {
            let value = 8 + 10 * (index - 232);
            (value, value, value)
        }
    }
}

fn nearest_ansi16(r: u8, g: u8, b: u8) -> Color {
    const ANSI16: &[(u8, (u8, u8, u8))] = &[
        (0, (0, 0, 0)),
        (1, (128, 0, 0)),
        (2, (0, 128, 0)),
        (3, (128, 128, 0)),
        (4, (0, 0, 128)),
        (5, (128, 0, 128)),
        (6, (0, 128, 128)),
        (7, (192, 192, 192)),
        (8, (128, 128, 128)),
        (9, (255, 0, 0)),
        (10, (0, 255, 0)),
        (11, (255, 255, 0)),
        (12, (0, 0, 255)),
        (13, (255, 0, 255)),
        (14, (0, 255, 255)),
        (15, (255, 255, 255)),
    ];

    ANSI16
        .iter()
        .min_by_key(|(_, rgb)| color_distance_sq(r, g, b, *rgb))
        .map(|(index, _)| Color::Indexed(*index))
        .unwrap_or(Color::Indexed(15))
}

fn color_distance_sq(r: u8, g: u8, b: u8, target: (u8, u8, u8)) -> u32 {
    let dr = i32::from(r) - i32::from(target.0);
    let dg = i32::from(g) - i32::from(target.1);
    let db = i32::from(b) - i32::from(target.2);
    (dr * dr + dg * dg + db * db) as u32
}

#[cfg(test)]
#[path = "color_compat_tests.rs"]
mod tests;
