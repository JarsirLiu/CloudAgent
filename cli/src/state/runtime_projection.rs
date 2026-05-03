use agent_protocol::FrontendMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuntimePhase {
    Idle,
    ModelStreaming,
    ToolRunning,
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
        if !matches!(self.phase, Some(RuntimePhase::ToolRunning)) {
            self.active_tool_title = None;
        }
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
            Some(RuntimePhase::ToolRunning) | None => {}
        }
    }

    pub(crate) fn on_tool_started(&mut self, title: String) {
        self.phase = Some(RuntimePhase::ToolRunning);
        self.active_tool_title = Some(title);
        self.live_label = self
            .active_tool_title
            .as_ref()
            .map(|t| format!("running {t}"))
            .or(Some("running tool".to_string()));
    }

    pub(crate) fn on_tool_finished(&mut self) {
        self.active_tool_title = None;
        self.phase = Some(RuntimePhase::ModelStreaming);
        self.live_label = Some("assistant is responding".to_string());
    }

    pub(crate) fn on_turn_finished(&mut self) {
        self.active_tool_title = None;
        self.phase = Some(RuntimePhase::Idle);
        self.live_label = None;
    }

    pub(crate) fn on_assistant_activity(&mut self) {
        self.phase = Some(RuntimePhase::ModelStreaming);
        self.live_label = Some("assistant is responding".to_string());
    }

    pub(crate) fn on_reasoning_activity(&mut self) {
        self.phase = Some(RuntimePhase::ModelStreaming);
        self.live_label = Some("reasoning".to_string());
    }

    pub(crate) fn on_turn_started(&mut self) {
        self.phase = Some(RuntimePhase::ModelStreaming);
        self.live_label = Some("assistant is responding".to_string());
    }
}
