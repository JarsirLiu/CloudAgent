use crate::app::TuiApp;
use crate::state::NoticeLevel;

pub(crate) fn apply_filter_toggle(app: &mut TuiApp, raw_args: &str) -> Result<(), &'static str> {
    let arg = raw_args.trim().to_ascii_lowercase();
    match arg.as_str() {
        "on" => {
            app.run_state.pre_llm_filter_enabled = true;
            app.bottom_pane.show_transient_notice(
                NoticeLevel::Info,
                "Pre-LLM input filter enabled for this project.".to_string(),
            );
            Ok(())
        }
        "off" => {
            app.run_state.pre_llm_filter_enabled = false;
            app.bottom_pane.show_transient_notice(
                NoticeLevel::Info,
                "Pre-LLM input filter disabled for this project.".to_string(),
            );
            Ok(())
        }
        "status" => {
            let state = if app.run_state.pre_llm_filter_enabled {
                "on"
            } else {
                "off"
            };
            app.bottom_pane.show_transient_notice(
                NoticeLevel::Info,
                format!("Pre-LLM input filter is currently `{state}`."),
            );
            Ok(())
        }
        _ => Err("Invalid filter option. Use /filter and choose on/off."),
    }
}
