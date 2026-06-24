use crate::input::intent::GatewayConfigUpdate;
use crate::state::NoticeLevel;
use crate::state::reducer_routes::route_server_message;
use crate::ui::bottom_pane::dialogs::server_request::server_request_model::ServerRequestPresentation;
use agent_core::conversation::{ConversationSummary, ConversationTurn, TranscriptItem};
use agent_core::turn::TurnId;
use agent_core::{
    ModelRetryStage, ModelUsage, RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress,
    ServerRequestDecisionKind,
};
use agent_protocol::{
    AppClientCommand, AppServerMessage, ConversationViewSnapshot, InterruptDisposition, RequestId,
};

#[derive(Debug, Clone)]
pub(crate) enum TurnDispatch {
    Completed,
    Failed { error: String },
    Cancelled { reason: String },
}

#[derive(Debug, Clone)]
pub(crate) enum UiInputEvent {
    Command(AppClientCommand),
    LocalSessionListNextPage {
        cursor: String,
    },
    LocalConversationCreate(String),
    LocalConversationSwitch(String),
    LocalConversationTitle(String),
    LocalConversationArchive(String),
    LocalConversationDelete(String),
    LocalFilterToggle(String),
    LocalPermissionMode(String),
    LocalConfig {
        api_key: String,
        base_url: String,
        model: String,
    },
    LocalReasoning(String),
    LocalModel(String),
    LocalSkillInsert(String),
    LocalSkillsOpen,
    LocalGatewayOpen,
    LocalGatewaySelect(String),
    LocalGatewayWeixinLoginStart(String),
    LocalGatewayWeixinLoginCheck {
        platform: String,
        session_id: String,
        qr_url: String,
    },
    LocalGatewaySave {
        platform: String,
        enabled: bool,
        updates: Vec<GatewayConfigUpdate>,
    },
    ServerRequestAnswer {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
    LocalCopy,
    LocalCopyText(String),
    LocalImagePaste,
    LocalHelp,
    LocalInputError(String),
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ServerMessageReduce {
    pub(crate) actions: Vec<ServerAction>,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ServerAction {
    SetConversationListPage {
        conversations: Vec<ConversationSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    },
    InvalidateSkillsCatalog,
    SetConversationView(ConversationViewSnapshot),
    SwitchConversation(String),
    ClearCurrentTurnUsage,
    SetTokenUsage {
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
    },
    SetRetryStatus {
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    },
    SetContextCompactionStatus {
        estimated_tokens: u64,
    },
    ClearContextCompactionStatus,
    DismissServerRequestView(RequestId),
    ClearActiveRuntime {
        item_id: Option<String>,
    },
    ReplaceHistory(Vec<ConversationTurn>),
    ReplaceHistoryPage {
        turns: Vec<ConversationTurn>,
        has_more: bool,
        next_before_turn_id: Option<String>,
    },
    PrependHistoryPage {
        turns: Vec<ConversationTurn>,
        has_more: bool,
        next_before_turn_id: Option<String>,
    },
    UpsertTurnSnapshot(ConversationTurn),
    BindActiveTurn(TurnId),
    StartActiveTurnItem {
        turn_id: TurnId,
        item: RuntimeItem,
    },
    AppendActiveAgentDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendActiveReasoningDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendActiveRuntimeDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendActivePatchDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    UpdateActiveItemProgress {
        turn_id: TurnId,
        item_id: String,
        progress: RuntimeItemProgress,
    },
    UpdateActiveItemMetrics {
        turn_id: TurnId,
        item_id: String,
        metrics: RuntimeItemMetrics,
    },
    AppendActiveRuntimeOutputDelta {
        item_id: String,
        delta: String,
    },
    CompleteActiveTurnItem {
        turn_id: TurnId,
        item: RuntimeItem,
        transcript_item: TranscriptItem,
    },
    PushNoticeCell {
        label: String,
        message: String,
        level: NoticeLevel,
    },
    InterruptResult(InterruptDisposition),
    PushErrorCell(String),
    TurnDispatch(TurnDispatch),
    ShowServerRequestPrompt {
        request_id: RequestId,
        request: ServerRequestPresentation,
    },
}

pub(crate) fn apply_server_message(message: &AppServerMessage) -> ServerMessageReduce {
    ServerMessageReduce {
        actions: route_server_message(message),
    }
}

#[cfg(test)]
#[path = "reducer_tests.rs"]
mod tests;
