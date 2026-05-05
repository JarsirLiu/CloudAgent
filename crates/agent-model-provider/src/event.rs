use agent_core::ModelUsage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderStreamEvent {
    TextDelta(String),
    ReasoningTextDelta(String),
    ToolCallDelta(ProviderToolCallDelta),
    Usage(ModelUsage),
    Metadata(ProviderMetadata),
    Completed(ProviderCompletion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments_delta: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub model_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderCompletion {
    pub finish_reason: Option<String>,
    pub end_turn: Option<bool>,
}
