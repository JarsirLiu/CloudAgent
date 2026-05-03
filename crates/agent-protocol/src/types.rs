use crate::{TurnId, TurnState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FrontendMode {
    Idle,
    Running,
    WaitingForServerRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    OnRequest,
    Never,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnPolicy {
    #[serde(default = "default_permission_profile")]
    pub permission_profile: PermissionProfile,
    #[serde(default = "default_approval_policy")]
    pub approval_policy: ApprovalPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserTurnInput {
    pub conversation_id: String,
    pub content: String,
    #[serde(default = "default_turn_policy")]
    pub turn_policy: TurnPolicy,
}

fn default_permission_profile() -> PermissionProfile {
    PermissionProfile::ReadOnly
}

fn default_approval_policy() -> ApprovalPolicy {
    ApprovalPolicy::OnRequest
}

fn default_turn_policy() -> TurnPolicy {
    TurnPolicy {
        permission_profile: default_permission_profile(),
        approval_policy: default_approval_policy(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConversationStatus {
    Idle,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationSnapshot {
    pub conversation_id: String,
    pub conversation_status: ConversationStatus,
    pub active_turn: Option<TurnId>,
    pub turn_state: Option<TurnState>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub title: Option<String>,
    pub message_count: usize,
    pub updated_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDelivery {
    Lossless,
    BestEffort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationStream {
    CoreTranscript,
    Control,
    Diagnostic,
}
