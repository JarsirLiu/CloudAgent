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
    if matches_colorterm("truecolor") || matches_colorterm("24bit") {
        return ColorDepth::TrueColor;
    }
    if std::env::var_os("WT_SESSION").is_some() {
        return ColorDepth::TrueColor;
    }
    if let Some(term_program) = env_lowercase("TERM_PROGRAM")
        && ["iterm", "wezterm", "vscode", "warp", "hyper", "kitty"]
            .iter()
            .any(|needle| term_program.contains(needle))
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
        "always" | "auto" => None,
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
mod tests {
    use super::{
        BackgroundTone, ColorCliPreference, ColorDepth, TerminalCapabilities, adapt_bg,
        adapt_color, color_cli_preference,
    };
    use ratatui::style::Color;
    use std::ffi::OsString;

    #[test]
    fn ansi256_adapts_rgb_to_indexed_color() {
        assert!(matches!(
            adapt_color(
                Color::Rgb(120, 170, 255),
                TerminalCapabilities {
                    color_depth: ColorDepth::Ansi256,
                    supports_synchronized_update: true,
                    background_tone: BackgroundTone::Dark,
                }
            ),
            Color::Indexed(_)
        ));
    }

    #[test]
    fn ansi16_adapts_rgb_to_named_color() {
        assert_eq!(
            adapt_color(
                Color::Rgb(255, 32, 32),
                TerminalCapabilities {
                    color_depth: ColorDepth::Ansi16,
                    supports_synchronized_update: true,
                    background_tone: BackgroundTone::Dark,
                }
            ),
            Color::Indexed(9)
        );
    }

    #[test]
    fn no_color_resets_rgb_output() {
        assert_eq!(
            adapt_color(
                Color::Rgb(120, 170, 255),
                TerminalCapabilities {
                    color_depth: ColorDepth::NoColor,
                    supports_synchronized_update: true,
                    background_tone: BackgroundTone::Dark,
                }
            ),
            Color::Reset
        );
    }

    #[test]
    fn cli_color_flags_prefer_explicit_values() {
        assert_eq!(
            color_cli_preference(&[OsString::from("--color"), OsString::from("always"),]),
            Some(ColorCliPreference::Always)
        );
        assert_eq!(
            color_cli_preference(&[OsString::from("--no-color")]),
            Some(ColorCliPreference::Never)
        );
    }

    #[test]
    fn explicit_depth_override_disables_truecolor_without_touching_capabilities_shape() {
        unsafe {
            std::env::set_var("CLOUDAGENT_COLOR_DEPTH", "256");
        }
        let capabilities = TerminalCapabilities::detect();
        unsafe {
            std::env::remove_var("CLOUDAGENT_COLOR_DEPTH");
        }
        assert_eq!(capabilities.color_depth, ColorDepth::Ansi256);
    }

    #[test]
    fn apple_terminal_disables_synchronized_update_and_truecolor() {
        unsafe {
            std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        }
        let capabilities = TerminalCapabilities::detect();
        unsafe {
            std::env::remove_var("TERM_PROGRAM");
        }
        assert_eq!(capabilities.color_depth, ColorDepth::Ansi256);
        assert!(!capabilities.supports_synchronized_update);
    }

    #[test]
    fn light_background_darkens_bright_foregrounds_and_softens_dark_backgrounds() {
        let capabilities = TerminalCapabilities {
            color_depth: ColorDepth::TrueColor,
            supports_synchronized_update: true,
            background_tone: BackgroundTone::Light,
        };
        let fg = adapt_color(Color::Rgb(210, 215, 225), capabilities);
        let bg = adapt_bg(Color::Rgb(26, 34, 50), capabilities);
        assert!(matches!(fg, Color::Rgb(_, _, _)));
        assert!(matches!(bg, Color::Rgb(_, _, _)));
        let Color::Rgb(_, _, fg_b) = fg else {
            unreachable!()
        };
        let Color::Rgb(_, _, bg_b) = bg else {
            unreachable!()
        };
        assert!(fg_b < 225);
        assert!(bg_b > 50);
    }
}
