use agent_core::conversation::InputItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerIntent {
    Submit(Vec<InputItem>),
    Interrupt,
    Compact,
    Session,
    NewConversation(String),
    SessionSwitch(String),
    SetTitle(String),
    ArchiveConversation(String),
    DeleteConversation(String),
    Filter(String),
    Permissions(String),
    Config,
    ConfigSave {
        api_key: String,
        base_url: String,
        model: String,
    },
    Exit,
    Reset,
    Copy,
    CopyText(String),
    Help,
    UnknownCommand(String),
    None,
}
