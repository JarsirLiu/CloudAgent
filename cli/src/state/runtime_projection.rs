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
    }

    pub(crate) fn on_tool_started(&mut self, title: String) {
        self.phase = Some(RuntimePhase::ToolRunning);
        self.active_tool_title = Some(title);
    }

    pub(crate) fn on_tool_finished(&mut self) {
        self.active_tool_title = None;
        self.phase = Some(RuntimePhase::ModelStreaming);
    }

    pub(crate) fn on_turn_finished(&mut self) {
        self.active_tool_title = None;
        self.phase = Some(RuntimePhase::Idle);
    }
}
