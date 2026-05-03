use crate::app::TuiApp;
use crate::state::selectors::status_text_from_mode;
use crate::ui::widgets::runtime_status_panel::status_meta_from_projection;

pub(crate) struct StatusViewModel {
    pub(crate) text: String,
    pub(crate) meta: String,
}

pub(crate) fn build_status_view_model(app: &TuiApp) -> StatusViewModel {
    let fallback = status_text_from_mode(app.console_state.mode);
    let runtime_text = app.runtime_projection.status_text(fallback);
    let text = if runtime_text != fallback {
        runtime_text
    } else if let Some(notice) = app.run_state.current_system_notice() {
        notice.to_string()
    } else {
        runtime_text
    };

    let mut parts = Vec::new();
    parts.push(format!(
        "filter {}",
        if app.run_state.pre_llm_filter_enabled {
            "on"
        } else {
            "off"
        }
    ));
    parts.push(format!("perm {}", app.run_state.permission_mode));
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
    if let Some(runtime_meta) = status_meta_from_projection(&app.runtime_projection) {
        parts.push(runtime_meta);
    }

    StatusViewModel {
        text,
        meta: parts.join(" · "),
    }
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
