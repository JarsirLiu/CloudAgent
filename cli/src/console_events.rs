use agent_protocol::{
    AppServerMessage, AppServerNotification, FrontendMode, TurnItemDeltaKind, TurnItemKind,
};

#[derive(Debug, Default, Clone)]
pub(crate) struct ConsoleMessageEffects {
    pub(crate) explicit_mode: Option<FrontendMode>,
    pub(crate) status_notice: Option<Option<String>>,
    pub(crate) last_message_count: Option<usize>,
    pub(crate) history_loaded: Option<bool>,
    pub(crate) clear_approval: bool,
    pub(crate) clear_last_tool_name: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ItemDispatch {
    AssistantStarted { turn_id: String, item_id: String },
    ToolLikeStarted { item_id: String, title: String },
    AssistantDelta { item_id: String, delta: String },
    AssistantCompleted { item_id: String },
    ToolLikeCompleted { item_id: String },
}

#[derive(Debug, Clone)]
pub(crate) enum TurnDispatch {
    Completed,
    Failed { error: String },
    Cancelled { reason: String },
}

pub(crate) fn derive_message_effects(message: &AppServerMessage) -> ConsoleMessageEffects {
    let mut effects = ConsoleMessageEffects::default();

    let AppServerMessage::Notification(notification) = message else {
        return effects;
    };

    match notification {
        AppServerNotification::FrontendStateChanged { mode, .. } => {
            effects.explicit_mode = Some(*mode);
        }
        AppServerNotification::SessionStatus { snapshot, .. } => {
            effects.last_message_count = Some(snapshot.message_count);
            effects.status_notice = Some(None);
        }
        AppServerNotification::SessionHistory { messages, .. } => {
            effects.last_message_count = Some(messages.len());
            effects.status_notice = Some(Some("Workspace context ready".to_string()));
            effects.history_loaded = Some(true);
        }
        AppServerNotification::Info { message, .. } | AppServerNotification::Error { message, .. } => {
            effects.status_notice = Some(Some(message.clone()));
        }
        AppServerNotification::TurnCompleted { .. }
        | AppServerNotification::TurnFailed { .. }
        | AppServerNotification::TurnCancelled { .. } => {
            effects.clear_approval = true;
            effects.clear_last_tool_name = true;
        }
        _ => {}
    }

    effects
}

pub(crate) fn derive_item_dispatch(notification: &AppServerNotification) -> Option<ItemDispatch> {
    match notification {
        AppServerNotification::ItemStarted {
            turn_id,
            item_id,
            kind,
            title,
            ..
        } if *kind == TurnItemKind::AssistantMessage => Some(ItemDispatch::AssistantStarted {
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
        }),
        AppServerNotification::ItemStarted {
            item_id,
            kind,
            title,
            ..
        } if (*kind == TurnItemKind::Reasoning || *kind == TurnItemKind::ToolCall)
            && title.is_some() =>
        {
            Some(ItemDispatch::ToolLikeStarted {
                item_id: item_id.clone(),
                title: title.clone().unwrap_or_default(),
            })
        }
        AppServerNotification::ItemDelta {
            item_id,
            kind,
            delta,
            ..
        } if *kind == TurnItemDeltaKind::Text => Some(ItemDispatch::AssistantDelta {
            item_id: item_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::ItemCompleted { item_id, kind, .. }
            if *kind == TurnItemKind::AssistantMessage =>
        {
            Some(ItemDispatch::AssistantCompleted {
                item_id: item_id.clone(),
            })
        }
        AppServerNotification::ItemCompleted { item_id, kind, .. }
            if *kind == TurnItemKind::Reasoning || *kind == TurnItemKind::ToolCall =>
        {
            Some(ItemDispatch::ToolLikeCompleted {
                item_id: item_id.clone(),
            })
        }
        _ => None,
    }
}

pub(crate) fn derive_turn_dispatch(notification: &AppServerNotification) -> Option<TurnDispatch> {
    match notification {
        AppServerNotification::TurnCompleted { .. } => Some(TurnDispatch::Completed),
        AppServerNotification::TurnFailed { error, .. } => Some(TurnDispatch::Failed {
            error: error.clone(),
        }),
        AppServerNotification::TurnCancelled { reason, .. } => Some(TurnDispatch::Cancelled {
            reason: reason.clone(),
        }),
        _ => None,
    }
}
