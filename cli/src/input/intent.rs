#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerIntent {
    Submit(String),
    Interrupt,
    Compact,
    Session,
    NewConversation(String),
    SessionSwitch(String),
    SetTitle(String),
    ArchiveConversation(String),
    Filter(String),
    Permissions(String),
    Exit,
    Reset,
    Copy,
    Help,
    UnknownCommand(String),
    None,
}
