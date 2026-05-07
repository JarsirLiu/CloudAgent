use std::time::Instant;

use agent_protocol::{ModelRetryStage, TurnItemKind};

#[derive(Clone, Debug, Default)]
pub(crate) struct BottomPaneRuntimeState {
    pub(crate) active_tool_title: Option<String>,
    pub(crate) live_label: Option<String>,
    pub(crate) turn_active: bool,
    pub(crate) turn_started_at: Option<Instant>,
}

impl BottomPaneRuntimeState {
    pub(crate) fn reset(&mut self) {
        self.active_tool_title = None;
        self.live_label = None;
        self.turn_active = false;
        self.turn_started_at = None;
    }

    pub(crate) fn on_turn_started(&mut self) {
        self.turn_active = true;
        self.turn_started_at = Some(Instant::now());
        self.live_label = Some("Working".to_string());
        self.active_tool_title = None;
    }

    pub(crate) fn on_tool_finished(&mut self) {
        self.active_tool_title = None;
        if self.live_label.is_none() {
            self.live_label = Some("Working".to_string());
        }
    }

    pub(crate) fn on_turn_finished(&mut self) {
        self.reset();
    }

    pub(crate) fn on_model_retrying(
        &mut self,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    ) {
        let seconds = (next_delay_ms as f64) / 1000.0;
        let stage_label = match stage {
            ModelRetryStage::Request => "request",
            ModelRetryStage::Streaming => "stream",
        };
        self.live_label = Some(format!(
            "reconnecting ({stage_label} retry {attempt}, next in {seconds:.1}s)"
        ));
    }

    pub(crate) fn on_active_item_started(&mut self, kind: &TurnItemKind, title: Option<&str>) {
        match kind {
            TurnItemKind::AssistantMessage => {
                self.active_tool_title = None;
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::Reasoning => {
                self.active_tool_title = None;
                self.live_label = Some("Thinking".to_string());
            }
            TurnItemKind::CommandExecution => {
                self.active_tool_title =
                    Some(match title.map(str::trim).filter(|s| !s.is_empty()) {
                        Some(command) => format!("running command: {command}"),
                        None => "running command".to_string(),
                    });
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::ToolCall => {
                self.active_tool_title =
                    Some(match title.map(str::trim).filter(|s| !s.is_empty()) {
                        Some(tool) => format!("executing tool: {}", humanize_runtime_title(tool)),
                        None => "executing tool".to_string(),
                    });
                self.live_label = Some("Working".to_string());
            }
            _ => {
                self.active_tool_title =
                    title.map(humanize_runtime_title).filter(|s| !s.is_empty());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn set_live_label_for_test(&mut self, label: Option<String>) {
        self.live_label = label;
    }

    #[cfg(test)]
    pub(crate) fn set_active_tool_title_for_test(&mut self, title: Option<String>) {
        self.active_tool_title = title;
    }
}

fn humanize_runtime_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return String::new();
    }
    title
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
