use crate::types::{NotificationDelivery, NotificationStream};
use crate::view_state::ConversationViewSnapshot;
use crate::{RequestId, UserTurnInput};
use agent_core::{
    CompactionContinuation, ConversationSummary, ConversationTurn, ModelRetryStage, ModelUsage,
    ServerRequest, ServerRequestDecision, SkillMetadata, TranscriptItem, TurnId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportInitializeCapabilities {
    pub experimental_api: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opt_out_notification_methods: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionBootstrapContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_root_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CommandExecutionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_root_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportInitializeParams {
    pub client_info: TransportClientInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<TransportInitializeCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_context: Option<SessionBootstrapContext>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportInitializeResult {
    pub server_info: TransportServerInfo,
    pub protocol_version: String,
    pub transport: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnlineNodeSummary {
    pub node_id: String,
    pub display_name: String,
    pub labels: Vec<String>,
    pub version: String,
    pub online: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationListResponse {
    // Typed read surface for conversation index bootstrap / explicit refresh.
    pub conversations: Vec<ConversationSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillsListResponse {
    pub skills: Vec<SkillMetadata>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationViewResponse {
    // Typed read surface for authoritative multi-client view state bootstrap.
    pub snapshot: ConversationViewSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationHistoryResponse {
    // Typed read surface for committed transcript bootstrap / explicit refresh.
    pub turns: Vec<ConversationTurn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationHistoryPageResponse {
    // Typed read surface for paged transcript bootstrap / explicit refresh.
    pub turns: Vec<ConversationTurn>,
    pub has_more: bool,
    pub next_before_turn_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnlineNodeListResponse {
    pub nodes: Vec<OnlineNodeSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectTargetNodeResponse {
    pub node_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeWorkerHealth {
    Running,
    Faulted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterruptDisposition {
    Requested,
    NoActiveTurn,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeWorkerStatus {
    // Compatibility/debug field. For local sources this is now a derived worker
    // instance key, not a raw directory identity.
    pub worker_scope_key: String,
    pub health: NodeWorkerHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_for_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeStatusResponse {
    pub listen_address: String,
    pub worker_running: bool,
    pub platform_runtime_count: usize,
    pub managed_platform_count: usize,
    #[serde(default)]
    pub data_root_dir: String,
    #[serde(default)]
    pub conversation_store_dir: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workers: Vec<NodeWorkerStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeStopResponse {
    pub stopping: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformControlEntry {
    pub platform: String,
    pub enabled: bool,
    #[serde(default)]
    pub configured: bool,
    pub managed_by: String,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformControlListResponse {
    pub platforms: Vec<PlatformControlEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformControlStatusResponse {
    pub platform: PlatformControlEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformControlUpdateResponse {
    pub platform: PlatformControlEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformConfigField {
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub is_set: bool,
    pub required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformConfigResponse {
    pub platform: String,
    pub configured: bool,
    pub fields: Vec<PlatformConfigField>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeixinLoginStartResponse {
    pub session_id: String,
    pub qr_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeixinLoginStatusResponse {
    pub session_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppClientCommand {
    SubmitTurn(UserTurnInput),
    ResolveServerRequest {
        conversation_id: String,
        request_id: RequestId,
        decision: ServerRequestDecision,
    },
    InterruptTurn {
        conversation_id: String,
    },
    CompactConversation {
        conversation_id: String,
    },
    ResetConversation {
        conversation_id: String,
    },
    RequestConversationView {
        conversation_id: String,
    },
    RequestConversationHistory {
        conversation_id: String,
    },
    RequestConversationHistoryPage {
        conversation_id: String,
        before_turn_id: Option<String>,
        limit: usize,
    },
    ListConversations,
    ListSkills,
    ListOnlineNodes,
    ListPlatforms,
    GetNodeStatus,
    StopNode,
    SetConversationTitle {
        conversation_id: String,
        title: String,
    },
    CreateConversation {
        conversation_id: String,
    },
    SwitchConversation {
        conversation_id: String,
    },
    SelectTargetNode {
        node_id: String,
    },
    GetPlatformStatus {
        platform: String,
    },
    GetPlatformConfig {
        platform: String,
    },
    SetPlatformEnabled {
        platform: String,
        enabled: bool,
    },
    SetPlatformConfigValue {
        platform: String,
        key: String,
        value: String,
    },
    ClearPlatformConfigValue {
        platform: String,
        key: String,
    },
    ReloadLlmConfig {
        api_key: String,
        base_url: String,
        model: String,
    },
    StartWeixinLogin,
    CheckWeixinLogin {
        session_id: String,
    },
    ArchiveConversation {
        conversation_id: String,
    },
    DeleteConversation {
        conversation_id: String,
    },
    SubscribeConversation {
        conversation_id: String,
    },
    UnsubscribeConversation {
        conversation_id: String,
    },
    Exit,
}

impl AppClientCommand {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::SubmitTurn(input) => Some(&input.conversation_id),
            Self::ResolveServerRequest {
                conversation_id, ..
            }
            | Self::InterruptTurn { conversation_id }
            | Self::CompactConversation { conversation_id }
            | Self::ResetConversation { conversation_id }
            | Self::RequestConversationView { conversation_id }
            | Self::RequestConversationHistory { conversation_id }
            | Self::RequestConversationHistoryPage {
                conversation_id, ..
            }
            | Self::CreateConversation { conversation_id }
            | Self::SetConversationTitle {
                conversation_id, ..
            }
            | Self::SwitchConversation { conversation_id }
            | Self::ArchiveConversation { conversation_id }
            | Self::DeleteConversation { conversation_id }
            | Self::SubscribeConversation { conversation_id }
            | Self::UnsubscribeConversation { conversation_id } => Some(conversation_id),
            Self::ListConversations
            | Self::ListSkills
            | Self::ListOnlineNodes
            | Self::ListPlatforms
            | Self::GetNodeStatus
            | Self::StopNode
            | Self::SelectTargetNode { .. }
            | Self::GetPlatformStatus { .. }
            | Self::GetPlatformConfig { .. }
            | Self::SetPlatformEnabled { .. }
            | Self::SetPlatformConfigValue { .. }
            | Self::ClearPlatformConfigValue { .. }
            | Self::ReloadLlmConfig { .. }
            | Self::StartWeixinLogin
            | Self::CheckWeixinLogin { .. }
            | Self::Exit => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerNotification {
    ConversationViewChanged {
        conversation_id: String,
        snapshot: ConversationViewSnapshot,
    },
    TurnStarted {
        conversation_id: String,
        turn_id: TurnId,
    },
    ItemStarted {
        conversation_id: String,
        turn_id: TurnId,
        call_id: Option<String>,
        item: TranscriptItem,
    },
    AgentMessageDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    PlanDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningSummaryPartAdded {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        summary_index: usize,
    },
    ReasoningSummaryTextDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        summary_index: usize,
        delta: String,
    },
    ReasoningTextDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        content_index: usize,
        delta: String,
    },
    CommandExecutionOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    ToolOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    FileChangeOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    TokenUsageUpdated {
        conversation_id: String,
        turn_id: TurnId,
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
    },
    ModelRetrying {
        conversation_id: String,
        turn_id: TurnId,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    },
    ContextCompacted {
        conversation_id: String,
        turn_id: Option<TurnId>,
        continuation: CompactionContinuation,
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    ContextCompactionStarted {
        conversation_id: String,
        turn_id: Option<TurnId>,
        continuation: CompactionContinuation,
        estimated_tokens: u64,
    },
    ItemCompleted {
        conversation_id: String,
        turn_id: TurnId,
        call_id: Option<String>,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        conversation_id: String,
        turn_id: TurnId,
        request: ServerRequest,
    },
    ServerRequestResolved {
        conversation_id: String,
        turn_id: TurnId,
        request_id: RequestId,
        request: ServerRequest,
        decision: ServerRequestDecision,
    },
    TurnCompleted {
        conversation_id: String,
        turn_id: TurnId,
    },
    TurnFailed {
        conversation_id: String,
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        conversation_id: String,
        turn_id: TurnId,
        reason: String,
    },
    InterruptResult {
        conversation_id: String,
        disposition: InterruptDisposition,
    },
    // Incremental/state-sync notification only. Clients should bootstrap transcript state
    // through typed conversation/history reads and treat this as a sync/update channel.
    ConversationHistory {
        conversation_id: String,
        turns: Vec<ConversationTurn>,
    },
    TurnSnapshot {
        conversation_id: String,
        turn: ConversationTurn,
    },
    // Incremental/state-sync notification only. Typed historyPage remains the bootstrap/read path.
    ConversationHistoryPage {
        conversation_id: String,
        turns: Vec<ConversationTurn>,
        has_more: bool,
        next_before_turn_id: Option<String>,
    },
    // Incremental/state-sync notification only. Typed conversation/list remains the
    // authoritative bootstrap/read path.
    ConversationList {
        conversation_id: String,
        conversations: Vec<ConversationSummary>,
    },
    SkillsChanged {
        conversation_id: String,
    },
    OnlineNodeList {
        conversation_id: String,
        nodes: Vec<OnlineNodeSummary>,
    },
    ConversationSwitched {
        conversation_id: String,
    },
    ConversationSubscriptionChanged {
        conversation_id: String,
        subscribed: bool,
    },
    Info {
        conversation_id: String,
        message: String,
    },
    Error {
        conversation_id: String,
        // Stable machine-readable error prefixes may be included in message text.
        // Current convention includes:
        // - ERR_CONVERSATION_BUSY: submitted turn rejected because conversation already has an active turn.
        message: String,
    },
}

impl AppServerNotification {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::ConversationViewChanged {
                conversation_id, ..
            }
            | Self::TurnStarted {
                conversation_id, ..
            }
            | Self::ItemStarted {
                conversation_id, ..
            }
            | Self::AgentMessageDelta {
                conversation_id, ..
            }
            | Self::PlanDelta {
                conversation_id, ..
            }
            | Self::ReasoningSummaryPartAdded {
                conversation_id, ..
            }
            | Self::ReasoningSummaryTextDelta {
                conversation_id, ..
            }
            | Self::ReasoningTextDelta {
                conversation_id, ..
            }
            | Self::CommandExecutionOutputDelta {
                conversation_id, ..
            }
            | Self::ToolOutputDelta {
                conversation_id, ..
            }
            | Self::FileChangeOutputDelta {
                conversation_id, ..
            }
            | Self::TokenUsageUpdated {
                conversation_id, ..
            }
            | Self::ModelRetrying {
                conversation_id, ..
            }
            | Self::ContextCompacted {
                conversation_id, ..
            }
            | Self::ContextCompactionStarted {
                conversation_id, ..
            }
            | Self::ItemCompleted {
                conversation_id, ..
            }
            | Self::ServerRequestRequested {
                conversation_id, ..
            }
            | Self::ServerRequestResolved {
                conversation_id, ..
            }
            | Self::TurnCompleted {
                conversation_id, ..
            }
            | Self::TurnFailed {
                conversation_id, ..
            }
            | Self::TurnCancelled {
                conversation_id, ..
            }
            | Self::InterruptResult {
                conversation_id, ..
            }
            | Self::ConversationHistory {
                conversation_id, ..
            }
            | Self::TurnSnapshot {
                conversation_id, ..
            }
            | Self::ConversationHistoryPage {
                conversation_id, ..
            }
            | Self::ConversationList {
                conversation_id, ..
            }
            | Self::SkillsChanged {
                conversation_id, ..
            }
            | Self::OnlineNodeList {
                conversation_id, ..
            }
            | Self::ConversationSwitched {
                conversation_id, ..
            }
            | Self::ConversationSubscriptionChanged {
                conversation_id, ..
            }
            | Self::Info {
                conversation_id, ..
            }
            | Self::Error {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerRequest {
    ServerRequest {
        request_id: RequestId,
        conversation_id: String,
        request: ServerRequest,
    },
}

impl AppServerRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::ServerRequest { request_id, .. } => request_id,
        }
    }

    pub fn conversation_id(&self) -> &str {
        match self {
            Self::ServerRequest {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppServerMessage {
    Notification(AppServerNotification),
    Request(AppServerRequest),
}

impl AppServerMessage {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::Notification(notification) => Some(notification.conversation_id()),
            Self::Request(request) => Some(request.conversation_id()),
        }
    }
}

pub fn classify_notification(
    notification: &AppServerNotification,
) -> (NotificationStream, NotificationDelivery) {
    match notification {
        AppServerNotification::AgentMessageDelta { .. }
        | AppServerNotification::PlanDelta { .. }
        | AppServerNotification::ReasoningSummaryPartAdded { .. }
        | AppServerNotification::ReasoningSummaryTextDelta { .. }
        | AppServerNotification::ReasoningTextDelta { .. }
        | AppServerNotification::ItemCompleted { .. }
        | AppServerNotification::TurnSnapshot { .. }
        | AppServerNotification::TurnCompleted { .. } => (
            NotificationStream::CoreTranscript,
            NotificationDelivery::Lossless,
        ),
        AppServerNotification::TurnStarted { .. }
        | AppServerNotification::ConversationViewChanged { .. }
        | AppServerNotification::ItemStarted { .. }
        | AppServerNotification::ServerRequestRequested { .. }
        | AppServerNotification::ServerRequestResolved { .. }
        | AppServerNotification::TokenUsageUpdated { .. }
        | AppServerNotification::ModelRetrying { .. }
        | AppServerNotification::ContextCompacted { .. }
        | AppServerNotification::ContextCompactionStarted { .. }
        | AppServerNotification::TurnFailed { .. }
        | AppServerNotification::TurnCancelled { .. }
        | AppServerNotification::InterruptResult { .. }
        | AppServerNotification::ConversationHistory { .. }
        | AppServerNotification::ConversationHistoryPage { .. }
        | AppServerNotification::ConversationList { .. }
        | AppServerNotification::SkillsChanged { .. }
        | AppServerNotification::OnlineNodeList { .. }
        | AppServerNotification::ConversationSwitched { .. }
        | AppServerNotification::ConversationSubscriptionChanged { .. } => {
            (NotificationStream::Control, NotificationDelivery::Lossless)
        }
        AppServerNotification::CommandExecutionOutputDelta { .. }
        | AppServerNotification::ToolOutputDelta { .. }
        | AppServerNotification::FileChangeOutputDelta { .. } => (
            NotificationStream::Control,
            NotificationDelivery::BestEffort,
        ),
        AppServerNotification::Info { .. } | AppServerNotification::Error { .. } => (
            NotificationStream::Diagnostic,
            NotificationDelivery::BestEffort,
        ),
    }
}
