#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerIntent {
    Submit(String),
    Interrupt,
    Compact,
    Sessions,
    NewConversation(String),
    SwitchConversation(String),
    ArchiveConversation(String),
    Exit,
    Reset,
    Copy,
    Help,
    UnknownCommand(String),
    None,
}
