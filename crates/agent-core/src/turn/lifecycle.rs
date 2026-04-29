#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnLifecyclePhase {
    Prepare,
    Model,
    Tools,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnLifecycleClass {
    CoreTranscript,
    Control,
    Diagnostic,
}
