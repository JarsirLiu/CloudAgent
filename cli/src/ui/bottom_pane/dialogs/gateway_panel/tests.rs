use super::GatewayPanel;
use crate::input::intent::{ComposerIntent, GatewayConfigUpdate};
use crate::ui::bottom_pane::bottom_pane_view::BottomPaneView;
use crate::ui::bottom_pane::bottom_pane_view::BottomPaneViewAction;
use agent_protocol::{PlatformConfigField, PlatformConfigResponse, PlatformControlEntry};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn entry(platform: &str, enabled: bool) -> PlatformControlEntry {
    PlatformControlEntry {
        platform: platform.to_string(),
        enabled,
        configured: enabled,
        managed_by: "node".to_string(),
        updated_at_ms: 0,
    }
}

#[test]
fn list_panel_renders_platform_statuses() {
    let panel = GatewayPanel::list(vec![entry("feishu", true), entry("wecom", false)]);
    let lines = panel
        .render_lines(80)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(lines.iter().any(|line| line.contains("Gateway Panel")));
    assert!(lines.iter().any(|line| line.contains("feishu")));
    assert!(lines.iter().any(|line| line.contains("enabled")));
    assert!(lines.iter().any(|line| line.contains("configured")));
    assert!(lines.iter().any(|line| line.contains("wecom")));
    assert!(lines.iter().any(|line| line.contains("disabled")));
}

#[test]
fn list_panel_enter_selects_current_platform() {
    let mut panel = GatewayPanel::list(vec![entry("feishu", true), entry("wecom", false)]);
    let action = panel.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::ComposerWithoutDismiss(ComposerIntent::GatewaySelect(platform))
        if platform == "feishu"
    ));
}

#[test]
fn edit_panel_save_emits_gateway_update_intent() {
    let mut panel = GatewayPanel::edit(
        entry("feishu", false),
        PlatformConfigResponse {
            platform: "feishu".to_string(),
            configured: false,
            fields: vec![
                PlatformConfigField {
                    key: "app_id".to_string(),
                    value: None,
                    required: true,
                    is_secret: false,
                    is_set: false,
                },
                PlatformConfigField {
                    key: "app_secret".to_string(),
                    value: None,
                    required: true,
                    is_secret: true,
                    is_set: false,
                },
            ],
        },
        None,
    );

    let _ = panel.handle_key_event(key(KeyCode::Char('c')));
    let _ = panel.handle_key_event(key(KeyCode::Char('l')));
    let _ = panel.handle_key_event(key(KeyCode::Char('i')));
    let _ = panel.handle_key_event(key(KeyCode::Tab));
    let _ = panel.handle_key_event(key(KeyCode::Char('s')));
    let _ = panel.handle_key_event(key(KeyCode::Char('e')));
    let _ = panel.handle_key_event(key(KeyCode::Char('c')));
    let _ = panel.handle_key_event(key(KeyCode::Tab));
    let _ = panel.handle_key_event(key(KeyCode::Char(' ')));
    let _ = panel.handle_key_event(key(KeyCode::Tab));
    let action = panel.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::ComposerWithoutDismiss(ComposerIntent::GatewaySave {
            platform,
            enabled: true,
            updates,
        }) if platform == "feishu"
            && updates == vec![
                GatewayConfigUpdate {
                    key: "app_id".to_string(),
                    value: Some("cli".to_string()),
                },
                GatewayConfigUpdate {
                    key: "app_secret".to_string(),
                    value: Some("sec".to_string()),
                },
            ]
    ));
}

#[test]
fn edit_panel_connection_enter_applies_immediately() {
    let mut panel = GatewayPanel::edit(
        entry("feishu", false),
        PlatformConfigResponse {
            platform: "feishu".to_string(),
            configured: true,
            fields: vec![PlatformConfigField {
                key: "app_id".to_string(),
                value: Some("cli".to_string()),
                required: true,
                is_secret: false,
                is_set: true,
            }],
        },
        None,
    );

    let _ = panel.handle_key_event(key(KeyCode::Tab));
    let action = panel.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::ComposerWithoutDismiss(ComposerIntent::GatewaySave {
            platform,
            enabled: true,
            updates,
        }) if platform == "feishu" && updates.is_empty()
    ));
}

#[test]
fn edit_panel_connection_enter_includes_dirty_field_updates() {
    let mut panel = GatewayPanel::edit(
        entry("feishu", false),
        PlatformConfigResponse {
            platform: "feishu".to_string(),
            configured: false,
            fields: vec![
                PlatformConfigField {
                    key: "app_id".to_string(),
                    value: None,
                    required: true,
                    is_secret: false,
                    is_set: false,
                },
                PlatformConfigField {
                    key: "app_secret".to_string(),
                    value: None,
                    required: true,
                    is_secret: true,
                    is_set: false,
                },
            ],
        },
        None,
    );

    let _ = panel.handle_key_event(key(KeyCode::Char('c')));
    let _ = panel.handle_key_event(key(KeyCode::Char('l')));
    let _ = panel.handle_key_event(key(KeyCode::Char('i')));
    let _ = panel.handle_key_event(key(KeyCode::Tab));
    let _ = panel.handle_key_event(key(KeyCode::Char('s')));
    let _ = panel.handle_key_event(key(KeyCode::Char('e')));
    let _ = panel.handle_key_event(key(KeyCode::Char('c')));
    let _ = panel.handle_key_event(key(KeyCode::Tab));
    let action = panel.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::ComposerWithoutDismiss(ComposerIntent::GatewaySave {
            platform,
            enabled: true,
            updates,
        }) if platform == "feishu"
            && updates == vec![
                GatewayConfigUpdate {
                    key: "app_id".to_string(),
                    value: Some("cli".to_string()),
                },
                GatewayConfigUpdate {
                    key: "app_secret".to_string(),
                    value: Some("sec".to_string()),
                },
            ]
    ));
}

#[test]
fn edit_panel_ignores_immediate_char_echo_after_paste() {
    let mut panel = GatewayPanel::edit(
        entry("feishu", false),
        PlatformConfigResponse {
            platform: "feishu".to_string(),
            configured: false,
            fields: vec![PlatformConfigField {
                key: "app_id".to_string(),
                value: None,
                required: true,
                is_secret: false,
                is_set: false,
            }],
        },
        None,
    );

    let _ = panel.handle_paste("token123");
    for ch in "token123".chars() {
        let _ = panel.handle_key_event(key(KeyCode::Char(ch)));
    }
    let action = panel.handle_key_event(key(KeyCode::Tab));
    assert!(matches!(action, BottomPaneViewAction::None));

    let rendered = panel
        .render_lines(80)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("token123 (required)"))
    );
}

