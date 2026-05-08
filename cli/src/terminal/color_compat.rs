use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorDepth {
    TrueColor,
    Ansi256,
    Ansi16,
}

impl ColorDepth {
    pub(crate) fn detect() -> Self {
        if let Some(value) = std::env::var_os("CLOUDAGENT_COLOR_DEPTH") {
            if let Some(depth) = parse_color_depth_override(value.to_string_lossy().trim()) {
                return depth;
            }
        }

        if matches_colorterm("truecolor") || matches_colorterm("24bit") {
            return Self::TrueColor;
        }
        if std::env::var_os("WT_SESSION").is_some() {
            return Self::TrueColor;
        }
        if let Some(term_program) = env_lowercase("TERM_PROGRAM")
            && ["iterm", "wezterm", "vscode", "warp", "hyper", "kitty"]
                .iter()
                .any(|needle| term_program.contains(needle))
        {
            return Self::TrueColor;
        }
        if let Some(term) = env_lowercase("TERM") {
            if term == "dumb" {
                return Self::Ansi16;
            }
            if term.contains("truecolor") || term.contains("24bit") {
                return Self::TrueColor;
            }
            if term.contains("256") {
                return Self::Ansi256;
            }
            return Self::Ansi256;
        }
        Self::Ansi16
    }
}

pub(crate) fn adapt_color(color: Color, depth: ColorDepth) -> Color {
    match (color, depth) {
        (Color::Rgb(r, g, b), ColorDepth::Ansi256) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => nearest_ansi16(r, g, b),
        _ => color,
    }
}

pub(crate) fn adapt_bg(color: Color, depth: ColorDepth) -> Color {
    adapt_color(color, depth)
}

fn env_lowercase(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| value.to_ascii_lowercase())
}

fn matches_colorterm(needle: &str) -> bool {
    env_lowercase("COLORTERM")
        .map(|value| value.contains(needle))
        .unwrap_or(false)
}

fn parse_color_depth_override(value: &str) -> Option<ColorDepth> {
    match value.to_ascii_lowercase().as_str() {
        "truecolor" | "24bit" | "24" => Some(ColorDepth::TrueColor),
        "256" | "ansi256" => Some(ColorDepth::Ansi256),
        "16" | "ansi16" => Some(ColorDepth::Ansi16),
        "auto" => None,
        _ => None,
    }
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
    use super::{ColorDepth, adapt_color};
    use ratatui::style::Color;

    #[test]
    fn ansi256_adapts_rgb_to_indexed_color() {
        assert!(matches!(
            adapt_color(Color::Rgb(120, 170, 255), ColorDepth::Ansi256),
            Color::Indexed(_)
        ));
    }

    #[test]
    fn ansi16_adapts_rgb_to_named_color() {
        assert_eq!(
            adapt_color(Color::Rgb(255, 32, 32), ColorDepth::Ansi16),
            Color::Indexed(9)
        );
    }
}
