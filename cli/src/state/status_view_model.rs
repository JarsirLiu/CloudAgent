use crate::app::TuiApp;
use crate::state::selectors::status_text_from_mode;

pub(crate) struct StatusViewModel {
    pub(crate) live_text: String,
    pub(crate) bar_text: String,
    pub(crate) meta: String,
    pub(crate) hint_meta: String,
}

pub(crate) fn build_status_view_model(app: &TuiApp) -> StatusViewModel {
    let fallback = status_text_from_mode(app.console_state.mode);
    let live_text = if let Some(notice) = app.run_state.current_system_notice() {
        notice.to_string()
    } else if let Some(tool_title) = app.runtime_projection.active_tool_title.as_deref() {
        animate_status(tool_title, app.run_state.live_animation_frame)
    } else if let Some(live_label) = app.runtime_projection.live_label.as_deref() {
        animate_status(live_label, app.run_state.live_animation_frame)
    } else {
        String::new()
    };

    let mut parts = Vec::new();
    let hint_meta = format!(
        "filter {} · perm {}",
        if app.run_state.pre_llm_filter_enabled {
            "on"
        } else {
            "off"
        },
        app.run_state.permission_mode
    );
    if let Some(usage) = &app.run_state.last_turn_usage {
        parts.push(format!(
            "in {} · out {} · cached {} · total {}",
            format_tokens(usage.input_tokens),
            format_tokens(usage.output_tokens),
            format_tokens(usage.cached_input_tokens),
            format_tokens(usage.total_tokens)
        ));
    }
    if let (Some(last), Some(window)) = (
        &app.run_state.last_turn_usage,
        app.run_state.model_context_window,
    ) && window > 0
    {
        let percent = last.total_tokens.saturating_mul(100) / window;
        parts.push(format!("context {percent}%"));
    }
    StatusViewModel {
        live_text,
        bar_text: fallback.to_string(),
        meta: parts.join(" · "),
        hint_meta,
    }
}

fn animate_status(text: &str, frame: u64) -> String {
    const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
    format!("{} {}", FRAMES[(frame as usize) % FRAMES.len()], text)
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_tokens(value: u64) -> String {
    format!("{} tokens", compact_number(value))
}

#[cfg(test)]
mod tests {
    use super::build_status_view_model;
    use crate::app::TuiApp;
    use agent_protocol::FrontendMode;
    use std::path::PathBuf;

    fn test_app() -> TuiApp {
        TuiApp::new(
            "default".to_string(),
            "test",
            PathBuf::from("D:\\learn\\gifti\\cloudagent"),
            PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
            false,
            "ReadOnly".to_string(),
        )
    }

    #[test]
    fn active_tool_status_overrides_live_label() {
        let mut app = test_app();
        app.console_state.mode = FrontendMode::Running;
        app.run_state.clear_system_notice();
        app.run_state.live_animation_frame = 1;
        app.runtime_projection.live_label = Some("assistant is responding".to_string());
        app.runtime_projection.active_tool_title = Some("running command: rg cli".to_string());

        let status = build_status_view_model(&app);

        assert_eq!(status.live_text, "/ running command: rg cli");
        assert_eq!(status.bar_text, "Working");
    }

    #[test]
    fn live_label_animates_when_no_active_tool_or_notice() {
        let mut app = test_app();
        app.console_state.mode = FrontendMode::Running;
        app.run_state.clear_system_notice();
        app.run_state.live_animation_frame = 2;
        app.runtime_projection.live_label = Some("assistant is thinking".to_string());

        let status = build_status_view_model(&app);

        assert_eq!(status.live_text, "- assistant is thinking");
        assert_eq!(status.bar_text, "Working");
    }
}
