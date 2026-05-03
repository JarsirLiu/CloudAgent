#[derive(Clone, Debug, Default)]
pub(crate) struct RunLiveState {
    pub(crate) phase: LivePhase,
}

#[derive(Clone, Debug, Default)]
pub(crate) enum LivePhase {
    #[default]
    Idle,
    AssistantResponding,
    Reasoning,
    ToolRunning { title: String },
}
