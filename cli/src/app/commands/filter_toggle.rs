use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};

pub(crate) fn apply_filter_toggle(app: &mut TuiApp, raw_args: &str) -> Result<(), &'static str> {
    let arg = raw_args.trim().to_ascii_lowercase();
    match arg.as_str() {
        "on" => {
            app.run_state.pre_llm_filter_enabled = true;
            app.push_live_cell(HistoryCell::info(
                "context",
                "Pre-LLM input filter enabled for this project.",
                HistoryTone::Control,
            ));
            Ok(())
        }
        "off" => {
            app.run_state.pre_llm_filter_enabled = false;
            app.push_live_cell(HistoryCell::info(
                "context",
                "Pre-LLM input filter disabled for this project.",
                HistoryTone::Control,
            ));
            Ok(())
        }
        "status" => {
            let state = if app.run_state.pre_llm_filter_enabled {
                "on"
            } else {
                "off"
            };
            app.push_live_cell(HistoryCell::info(
                "context",
                format!("Pre-LLM input filter is currently `{state}`."),
                HistoryTone::Control,
            ));
            Ok(())
        }
        _ => Err("Invalid filter option. Use /filter and choose on/off."),
    }
}
