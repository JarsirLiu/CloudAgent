use agent_core::{ApprovalPolicy, PermissionProfile};
use agent_protocol::TurnPolicy;

pub(crate) struct PermissionModeSpec {
    pub(crate) mode: &'static str,
    pub(crate) label: &'static str,
}

pub(crate) const DEFAULT_PERMISSION_MODE: &str = "WorkspaceWrite";

pub(crate) const PERMISSION_MODE_SPECS: [PermissionModeSpec; 3] = [
    PermissionModeSpec {
        mode: "ReadOnly",
        label: "read operations only; writes and other changes need approval",
    },
    PermissionModeSpec {
        mode: "WorkspaceWrite",
        label: "workspace writes allowed; outside-workspace and risky actions need approval",
    },
    PermissionModeSpec {
        mode: "FullAccess",
        label: "full access; outside-workspace actions allowed; approvals are not requested",
    },
];

pub(crate) fn normalize_permission_mode(mode: &str) -> Option<&'static str> {
    let value = mode.trim();
    if value.eq_ignore_ascii_case("readonly") || value.eq_ignore_ascii_case("safe") {
        return Some("ReadOnly");
    }
    if value.eq_ignore_ascii_case("workspacewrite") || value.eq_ignore_ascii_case("balanced") {
        return Some("WorkspaceWrite");
    }
    if value.eq_ignore_ascii_case("fullaccess") || value.eq_ignore_ascii_case("danger") {
        return Some("FullAccess");
    }
    None
}

pub(crate) fn is_valid_permission_mode(mode: &str) -> bool {
    normalize_permission_mode(mode).is_some()
}

pub(crate) fn permission_mode_label(mode: &str) -> &'static str {
    let Some(mode) = normalize_permission_mode(mode) else {
        return "unknown mode";
    };
    PERMISSION_MODE_SPECS
        .iter()
        .find(|spec| spec.mode == mode)
        .map(|spec| spec.label)
        .unwrap_or("unknown mode")
}

pub(crate) fn canonical_permission_mode(mode: &str) -> &'static str {
    normalize_permission_mode(mode).unwrap_or(DEFAULT_PERMISSION_MODE)
}

pub(crate) fn turn_policy_for_mode(mode: &str) -> TurnPolicy {
    match canonical_permission_mode(mode) {
        "WorkspaceWrite" => TurnPolicy {
            permission_profile: PermissionProfile::WorkspaceWrite,
            approval_policy: ApprovalPolicy::OnRequest,
        },
        "FullAccess" => TurnPolicy {
            permission_profile: PermissionProfile::FullAccess,
            approval_policy: ApprovalPolicy::Never,
        },
        _ => TurnPolicy {
            permission_profile: PermissionProfile::ReadOnly,
            approval_policy: ApprovalPolicy::OnRequest,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_legacy_and_canonical_permission_modes() {
        assert_eq!(normalize_permission_mode("safe"), Some("ReadOnly"));
        assert_eq!(normalize_permission_mode("ReadOnly"), Some("ReadOnly"));
        assert_eq!(
            normalize_permission_mode("balanced"),
            Some("WorkspaceWrite")
        );
        assert_eq!(
            normalize_permission_mode("WorkspaceWrite"),
            Some("WorkspaceWrite")
        );
        assert_eq!(normalize_permission_mode("danger"), Some("FullAccess"));
        assert_eq!(normalize_permission_mode("FullAccess"), Some("FullAccess"));
    }

    #[test]
    fn turn_policy_uses_canonical_permission_profiles() {
        assert!(matches!(
            turn_policy_for_mode("ReadOnly").permission_profile,
            PermissionProfile::ReadOnly
        ));
        assert!(matches!(
            turn_policy_for_mode("WorkspaceWrite").permission_profile,
            PermissionProfile::WorkspaceWrite
        ));
        assert!(matches!(
            turn_policy_for_mode("FullAccess").permission_profile,
            PermissionProfile::FullAccess
        ));
    }
}
