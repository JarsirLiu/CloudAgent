#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerIntent {
    Submit(String),
    Interrupt,
    Exit,
    Reset,
    Copy,
    Help,
    UnknownCommand(String),
    None,
}
