use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ExecutionPolicy {
    pub max_tool_roundtrips: Option<usize>,
}

impl ExecutionPolicy {
    pub fn new(max_tool_roundtrips: Option<usize>) -> Self {
        Self {
            max_tool_roundtrips: max_tool_roundtrips.map(|value| value.max(1)),
        }
    }
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
