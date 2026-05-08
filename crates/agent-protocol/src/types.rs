use agent_core::{ApprovalPolicy, InputItem, PermissionProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FrontendMode {
    Idle,
    Running,
    WaitingForServerRequest,
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
    pub content: Vec<InputItem>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_turn_input_preserves_structured_items_on_json_roundtrip() {
        let value = serde_json::json!({
            "conversation_id": "default",
            "content": [
                { "type": "text", "text": "look at this" },
                {
                    "type": "image",
                    "source": {
                        "type": "remote_url",
                        "url": "https://example.com/diagram.png"
                    },
                    "detail": "high",
                    "alt": "diagram"
                }
            ]
        });

        let parsed: UserTurnInput = serde_json::from_value(value.clone()).expect("parse input");
        let reserialized = serde_json::to_value(parsed).expect("serialize input");

        assert_eq!(reserialized["conversation_id"], value["conversation_id"]);
        assert_eq!(reserialized["content"], value["content"]);
        assert_eq!(
            reserialized["turn_policy"],
            serde_json::json!({
                "permission_profile": "read_only",
                "approval_policy": "on_request"
            })
        );
    }
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
