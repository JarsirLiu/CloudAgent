use super::{
    BackgroundTone, ColorCliPreference, ColorDepth, TerminalCapabilities, adapt_bg, adapt_color,
    color_cli_preference, effective_color_depth,
};
use ratatui::style::Color;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().expect("env lock")
}

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
        color_cli_preference(&[OsString::from("--color"), OsString::from("always")]),
        Some(ColorCliPreference::Always)
    );
    assert_eq!(
        color_cli_preference(&[OsString::from("--no-color")]),
        Some(ColorCliPreference::Never)
    );
}

#[test]
fn explicit_depth_override_disables_truecolor_without_touching_capabilities_shape() {
    let _lock = env_lock();
    let previous_depth = std::env::var_os("CLOUDAGENT_COLOR_DEPTH");
    unsafe {
        std::env::set_var("CLOUDAGENT_COLOR_DEPTH", "256");
    }
    let capabilities = TerminalCapabilities::detect();
    unsafe {
        match previous_depth {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_DEPTH", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_DEPTH"),
        }
    }
    assert_eq!(capabilities.color_depth, ColorDepth::Ansi256);
}

#[test]
fn color_always_forces_truecolor() {
    let _lock = env_lock();
    let previous_mode = std::env::var_os("CLOUDAGENT_COLOR_MODE");
    let previous_depth = std::env::var_os("CLOUDAGENT_COLOR_DEPTH");
    let previous_no_color = std::env::var_os("NO_COLOR");
    unsafe {
        std::env::set_var("CLOUDAGENT_COLOR_MODE", "always");
        std::env::remove_var("CLOUDAGENT_COLOR_DEPTH");
        std::env::remove_var("NO_COLOR");
    }
    let capabilities = TerminalCapabilities::detect();
    unsafe {
        match previous_mode {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_MODE", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_MODE"),
        }
        match previous_depth {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_DEPTH", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_DEPTH"),
        }
        match previous_no_color {
            Some(value) => std::env::set_var("NO_COLOR", value),
            None => std::env::remove_var("NO_COLOR"),
        }
    }
    assert_eq!(capabilities.color_depth, ColorDepth::TrueColor);
}

#[test]
fn wt_session_promotes_to_truecolor() {
    let _lock = env_lock();
    let previous_wt_session = std::env::var_os("WT_SESSION");
    let previous_mode = std::env::var_os("CLOUDAGENT_COLOR_MODE");
    let previous_depth = std::env::var_os("CLOUDAGENT_COLOR_DEPTH");
    let previous_no_color = std::env::var_os("NO_COLOR");
    let previous_force_color = std::env::var_os("FORCE_COLOR");
    unsafe {
        std::env::set_var("WT_SESSION", "test-session");
        std::env::remove_var("CLOUDAGENT_COLOR_MODE");
        std::env::remove_var("CLOUDAGENT_COLOR_DEPTH");
        std::env::remove_var("NO_COLOR");
        std::env::remove_var("FORCE_COLOR");
    }
    let capabilities = TerminalCapabilities::detect();
    unsafe {
        match previous_wt_session {
            Some(value) => std::env::set_var("WT_SESSION", value),
            None => std::env::remove_var("WT_SESSION"),
        }
        match previous_mode {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_MODE", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_MODE"),
        }
        match previous_depth {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_DEPTH", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_DEPTH"),
        }
        match previous_no_color {
            Some(value) => std::env::set_var("NO_COLOR", value),
            None => std::env::remove_var("NO_COLOR"),
        }
        match previous_force_color {
            Some(value) => std::env::set_var("FORCE_COLOR", value),
            None => std::env::remove_var("FORCE_COLOR"),
        }
    }
    assert_eq!(capabilities.color_depth, ColorDepth::TrueColor);
}

#[test]
fn windows_terminal_session_promotes_even_when_stdout_reports_ansi16() {
    assert_eq!(
        effective_color_depth(
            ColorDepth::Ansi16,
            /*has_windows_terminal_session*/ true,
            /*has_known_truecolor_term_program*/ false,
        ),
        ColorDepth::TrueColor
    );
}

#[test]
fn known_truecolor_terminal_promotes_ansi16() {
    assert_eq!(
        effective_color_depth(
            ColorDepth::Ansi16,
            /*has_windows_terminal_session*/ false,
            /*has_known_truecolor_term_program*/ true,
        ),
        ColorDepth::TrueColor
    );
}

#[test]
fn windows_terminal_name_is_known_truecolor_terminal() {
    let _lock = env_lock();
    let previous_term_program = std::env::var_os("TERM_PROGRAM");
    unsafe {
        std::env::set_var("TERM_PROGRAM", "WindowsTerminal");
    }
    let detected = super::is_known_truecolor_term_program();
    unsafe {
        match previous_term_program {
            Some(value) => std::env::set_var("TERM_PROGRAM", value),
            None => std::env::remove_var("TERM_PROGRAM"),
        }
    }
    assert!(detected);
}

#[test]
fn windows_terminal_profile_id_is_windows_terminal_session() {
    let _lock = env_lock();
    let previous_wt_session = std::env::var_os("WT_SESSION");
    let previous_wt_profile_id = std::env::var_os("WT_PROFILE_ID");
    let previous_term_program = std::env::var_os("TERM_PROGRAM");
    unsafe {
        std::env::remove_var("WT_SESSION");
        std::env::set_var("WT_PROFILE_ID", "{test-profile}");
        std::env::remove_var("TERM_PROGRAM");
    }
    let detected = super::is_windows_terminal_session();
    unsafe {
        match previous_wt_session {
            Some(value) => std::env::set_var("WT_SESSION", value),
            None => std::env::remove_var("WT_SESSION"),
        }
        match previous_wt_profile_id {
            Some(value) => std::env::set_var("WT_PROFILE_ID", value),
            None => std::env::remove_var("WT_PROFILE_ID"),
        }
        match previous_term_program {
            Some(value) => std::env::set_var("TERM_PROGRAM", value),
            None => std::env::remove_var("TERM_PROGRAM"),
        }
    }
    assert!(detected);
}

#[test]
fn force_color_does_not_block_windows_terminal_truecolor_promotion() {
    let _lock = env_lock();
    let previous_wt_session = std::env::var_os("WT_SESSION");
    let previous_mode = std::env::var_os("CLOUDAGENT_COLOR_MODE");
    let previous_depth = std::env::var_os("CLOUDAGENT_COLOR_DEPTH");
    let previous_no_color = std::env::var_os("NO_COLOR");
    let previous_force_color = std::env::var_os("FORCE_COLOR");
    unsafe {
        std::env::set_var("WT_SESSION", "test-session");
        std::env::remove_var("CLOUDAGENT_COLOR_MODE");
        std::env::remove_var("CLOUDAGENT_COLOR_DEPTH");
        std::env::remove_var("NO_COLOR");
        std::env::set_var("FORCE_COLOR", "1");
    }
    let capabilities = TerminalCapabilities::detect();
    unsafe {
        match previous_wt_session {
            Some(value) => std::env::set_var("WT_SESSION", value),
            None => std::env::remove_var("WT_SESSION"),
        }
        match previous_mode {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_MODE", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_MODE"),
        }
        match previous_depth {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_DEPTH", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_DEPTH"),
        }
        match previous_no_color {
            Some(value) => std::env::set_var("NO_COLOR", value),
            None => std::env::remove_var("NO_COLOR"),
        }
        match previous_force_color {
            Some(value) => std::env::set_var("FORCE_COLOR", value),
            None => std::env::remove_var("FORCE_COLOR"),
        }
    }
    assert_eq!(capabilities.color_depth, ColorDepth::TrueColor);
}

#[test]
fn ansi256_is_not_promoted_by_terminal_name() {
    assert_eq!(
        effective_color_depth(
            ColorDepth::Ansi256,
            /*has_windows_terminal_session*/ false,
            /*has_known_truecolor_term_program*/ true,
        ),
        ColorDepth::Ansi256
    );
}

#[test]
fn apple_terminal_disables_synchronized_update_and_truecolor() {
    let _lock = env_lock();
    let previous_term_program = std::env::var_os("TERM_PROGRAM");
    let previous_no_color = std::env::var_os("NO_COLOR");
    let previous_depth = std::env::var_os("CLOUDAGENT_COLOR_DEPTH");
    let previous_mode = std::env::var_os("CLOUDAGENT_COLOR_MODE");
    unsafe {
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        std::env::remove_var("NO_COLOR");
        std::env::remove_var("CLOUDAGENT_COLOR_DEPTH");
        std::env::remove_var("CLOUDAGENT_COLOR_MODE");
    }
    let capabilities = TerminalCapabilities::detect();
    unsafe {
        match previous_term_program {
            Some(value) => std::env::set_var("TERM_PROGRAM", value),
            None => std::env::remove_var("TERM_PROGRAM"),
        }
        match previous_no_color {
            Some(value) => std::env::set_var("NO_COLOR", value),
            None => std::env::remove_var("NO_COLOR"),
        }
        match previous_depth {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_DEPTH", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_DEPTH"),
        }
        match previous_mode {
            Some(value) => std::env::set_var("CLOUDAGENT_COLOR_MODE", value),
            None => std::env::remove_var("CLOUDAGENT_COLOR_MODE"),
        }
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