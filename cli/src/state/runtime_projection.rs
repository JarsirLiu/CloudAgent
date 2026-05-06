use agent_protocol::{FrontendMode, ModelRetryStage, TurnItemKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuntimePhase {
    Idle,
    ModelStreaming,
    WaitingApproval,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RuntimeProjection {
    pub(crate) phase: Option<RuntimePhase>,
    pub(crate) active_tool_title: Option<String>,
    pub(crate) live_label: Option<String>,
}

impl RuntimeProjection {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn on_mode_changed(&mut self, mode: FrontendMode) {
        self.phase = Some(match mode {
            FrontendMode::Idle => RuntimePhase::Idle,
            FrontendMode::Running => RuntimePhase::ModelStreaming,
            FrontendMode::WaitingForServerRequest => RuntimePhase::WaitingApproval,
        });
        self.active_tool_title = None;
        match self.phase {
            Some(RuntimePhase::Idle) => self.live_label = None,
            Some(RuntimePhase::ModelStreaming) => {
                if self.live_label.is_none() {
                    self.live_label = Some("assistant is responding".to_string());
                }
            }
            Some(RuntimePhase::WaitingApproval) => {
                self.live_label = Some("waiting for approval".to_string());
            }
            None => {}
        }
    }

    pub(crate) fn on_tool_finished(&mut self) {
        self.active_tool_title = None;
        match self.phase {
            Some(RuntimePhase::Idle) => {
                self.live_label = None;
            }
            Some(RuntimePhase::WaitingApproval) => {
                self.live_label = Some("waiting for approval".to_string());
            }
            Some(RuntimePhase::ModelStreaming) | None => {
                self.phase = Some(RuntimePhase::ModelStreaming);
                self.live_label = Some("assistant is responding".to_string());
            }
        }
    }

    pub(crate) fn on_turn_finished(&mut self) {
        self.active_tool_title = None;
        self.phase = Some(RuntimePhase::Idle);
        self.live_label = None;
    }

    pub(crate) fn on_model_retrying(
        &mut self,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    ) {
        self.phase = Some(RuntimePhase::ModelStreaming);
        let seconds = (next_delay_ms as f64) / 1000.0;
        let stage_label = match stage {
            ModelRetryStage::Request => "request",
            ModelRetryStage::Streaming => "stream",
        };
        self.live_label = Some(format!(
            "reconnecting ({stage_label} retry {attempt}, next in {seconds:.1}s)"
        ));
    }

    pub(crate) fn on_turn_started(&mut self) {
        self.phase = Some(RuntimePhase::ModelStreaming);
        self.live_label = Some("assistant is responding".to_string());
    }

    pub(crate) fn on_active_item_started(
        &mut self,
        kind: &TurnItemKind,
        title: Option<&str>,
    ) {
        self.phase = Some(RuntimePhase::ModelStreaming);
        match kind {
            TurnItemKind::AssistantMessage => {
                self.active_tool_title = None;
                self.live_label = Some("assistant is responding".to_string());
            }
            TurnItemKind::Reasoning => {
                self.active_tool_title = None;
                self.live_label = Some("assistant is thinking".to_string());
            }
            TurnItemKind::CommandExecution => {
                self.active_tool_title = Some(match title.map(str::trim).filter(|s| !s.is_empty()) {
                    Some(command) => format!("running command: {command}"),
                    None => "running command".to_string(),
                });
            }
            TurnItemKind::ToolCall => {
                self.active_tool_title = Some(match title.map(str::trim).filter(|s| !s.is_empty()) {
                    Some(tool) => format!("executing tool: {}", humanize_runtime_title(tool)),
                    None => "executing tool".to_string(),
                });
            }
            _ => {
                self.active_tool_title = title
                    .map(humanize_runtime_title)
                    .filter(|s| !s.is_empty());
            }
        }
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
