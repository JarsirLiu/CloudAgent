pub(crate) struct PermissionModeSpec {
    pub(crate) mode: &'static str,
    pub(crate) label: &'static str,
}

pub(crate) const DEFAULT_PERMISSION_MODE: &str = "safe";

pub(crate) const PERMISSION_MODE_SPECS: [PermissionModeSpec; 3] = [
    PermissionModeSpec {
        mode: "safe",
        label: "read any dir; write only workspace; risky commands require approval",
    },
    PermissionModeSpec {
        mode: "balanced",
        label: "read any dir; write only workspace; fewer command approvals",
    },
    PermissionModeSpec {
        mode: "danger",
        label: "read/write any dir and run commands without approval",
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
