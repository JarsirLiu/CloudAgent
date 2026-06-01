use agent_core::conversation::InputItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayConfigUpdate {
    pub(crate) key: String,
    pub(crate) value: Option<String>,
}

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
    Reasoning(String),
    Skill(String),
    Gateway,
    Skills,
    GatewaySelect(String),
    GatewayWeixinLoginStart {
        platform: String,
    },
    GatewayWeixinLoginCheck {
        platform: String,
        session_id: String,
        qr_url: String,
    },
    GatewaySave {
        platform: String,
        enabled: bool,
        updates: Vec<GatewayConfigUpdate>,
    },
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
