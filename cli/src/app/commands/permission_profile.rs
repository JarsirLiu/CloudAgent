use agent_protocol::{ApprovalPolicy, PermissionProfile, TurnPolicy};

pub(crate) struct PermissionModeSpec {
    pub(crate) mode: &'static str,
    pub(crate) label: &'static str,
}

pub(crate) const DEFAULT_PERMISSION_MODE: &str = "safe";

pub(crate) const PERMISSION_MODE_SPECS: [PermissionModeSpec; 3] = [
    PermissionModeSpec {
        mode: "safe",
        label: "read-only mode; only read tools are visible; writes and risky commands require approval",
    },
    PermissionModeSpec {
        mode: "balanced",
        label: "workspace-write mode; write tools are visible for workspace use; risky commands require approval",
    },
    PermissionModeSpec {
        mode: "danger",
        label: "full-access mode; all tools are visible; dangerous commands (e.g. rm -rf) still require approval",
    },
];

pub(crate) fn is_valid_permission_mode(mode: &str) -> bool {
    PERMISSION_MODE_SPECS.iter().any(|spec| spec.mode == mode)
}

pub(crate) fn permission_mode_label(mode: &str) -> &'static str {
    PERMISSION_MODE_SPECS
        .iter()
        .find(|spec| spec.mode == mode)
        .map(|spec| spec.label)
        .unwrap_or("unknown mode")
}

pub(crate) fn turn_policy_for_mode(mode: &str) -> TurnPolicy {
    match mode {
        "balanced" => TurnPolicy {
            permission_profile: PermissionProfile::WorkspaceWrite,
            approval_policy: ApprovalPolicy::OnRequest,
        },
        "danger" => TurnPolicy {
            permission_profile: PermissionProfile::FullAccess,
            approval_policy: ApprovalPolicy::Never,
        },
        _ => TurnPolicy {
            permission_profile: PermissionProfile::ReadOnly,
            approval_policy: ApprovalPolicy::OnRequest,
        },
    }
}
