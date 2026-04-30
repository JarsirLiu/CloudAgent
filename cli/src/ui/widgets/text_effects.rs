use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::time::{Duration, Instant};

static SHIMMER_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn elapsed() -> Duration {
    SHIMMER_START.get_or_init(Instant::now).elapsed()
}

pub fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![];
    }
    let padding = 8usize;
    let period = chars.len() + padding * 2;
    let sweep = 2.0f32;
    let pos = ((elapsed().as_secs_f32() % sweep) / sweep * period as f32) as usize;

    let base = Color::Rgb(100, 100, 110);
    let bright = Color::Rgb(200, 200, 220);

    chars
        .into_iter()
        .enumerate()
        .map(|(i, ch)| {
            let dist = ((i + padding) as isize - pos as isize).unsigned_abs();
            let t = if dist < 6 {
                let x = std::f32::consts::PI * (dist as f32 / 6.0);
                0.5 * (1.0 + x.cos())
            } else {
                0.0
            };
            let color = blend_color(base, bright, t * 0.85);
            Span::styled(
                ch.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )
        })
        .collect()
}

fn blend_color(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = unpack(a);
    let (br, bg, bb) = unpack(b);
    let r = (ar as f32 + (br as f32 - ar as f32) * t) as u8;
    let g = (ag as f32 + (bg as f32 - ag as f32) * t) as u8;
    let b2 = (ab as f32 + (bb as f32 - ab as f32) * t) as u8;
    Color::Rgb(r, g, b2)
}

fn unpack(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}
