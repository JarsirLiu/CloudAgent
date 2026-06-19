use crate::ui::bottom_pane::input_pane::{InputPane, InputPaneAction};
use crate::input::intent::ComposerIntent;
use agent_core::ConversationSummary;
use agent_protocol::{PlatformConfigResponse, PlatformControlEntry};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crate::ui::bottom_pane::dialogs::selection::session_picker::SessionPickerMode;
use crate::ui::bottom_pane::dialogs::weixin_binding_view::WeixinBindingViewModel;

fn esc_key() -> KeyEvent {
    KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

#[test]
fn esc_closes_help_view_without_interrupting() {
    let mut pane = InputPane::new();
    pane.set_help_view();

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.requires_action());
}

#[test]
fn esc_closes_model_picker_without_interrupting() {
    let mut pane = InputPane::new();
    pane.set_model_picker(
        "gpt-4.1".to_string(),
        vec!["gpt-4.1".to_string(), "gpt-4o".to_string()],
    );

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.requires_action());
}

#[test]
fn esc_closes_config_panel_without_interrupting() {
    let mut pane = InputPane::new();
    pane.set_config_panel(
        "key".to_string(),
        "https://example.com".to_string(),
        "gpt-4.1".to_string(),
    );

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.requires_action());
}

#[test]
fn esc_closes_session_picker_without_interrupting() {
    let mut pane = InputPane::new();
    pane.set_session_picker(
        vec![ConversationSummary {
            conversation_id: "default".to_string(),
            title: Some("Default".to_string()),
            message_count: 0,
            updated_at_ms: 1,
        }],
        "default",
        SessionPickerMode::Switch,
    );

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.requires_action());
}

fn enter_key() -> KeyEvent {
    KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

#[test]
fn esc_closes_session_picker_loading_without_interrupting() {
    let mut pane = InputPane::new();
    pane.set_session_picker_loading(SessionPickerMode::Switch);

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(pane.no_modal_or_popup_active());
}

#[test]
fn esc_release_after_closing_session_picker_loading_is_ignored() {
    let mut pane = InputPane::new();
    pane.set_session_picker_loading(SessionPickerMode::Switch);

    let press = pane.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });
    let release = pane.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        press,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(release.is_none());
    assert!(!pane.requires_action());
}

#[test]
fn esc_release_after_closing_session_picker_does_not_interrupt() {
    let mut pane = InputPane::new();
    pane.set_session_picker(
        vec![ConversationSummary {
            conversation_id: "default".to_string(),
            title: Some("Default".to_string()),
            message_count: 0,
            updated_at_ms: 1,
        }],
        "default",
        SessionPickerMode::Switch,
    );

    let press = pane.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });
    let release = pane.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        press,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(release.is_none());
    assert!(pane.no_modal_or_popup_active());
}

#[test]
fn esc_closes_gateway_list_without_interrupting() {
    let mut pane = InputPane::new();
    pane.set_gateway_list_panel(vec![PlatformControlEntry {
        platform: "weixin".to_string(),
        enabled: false,
        configured: false,
        managed_by: "local".to_string(),
        updated_at_ms: 1,
    }]);

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.requires_action());
}

#[test]
fn esc_returns_from_gateway_edit_to_gateway_list() {
    let mut pane = InputPane::new();
    pane.set_gateway_list_panel(vec![PlatformControlEntry {
        platform: "weixin".to_string(),
        enabled: false,
        configured: false,
        managed_by: "local".to_string(),
        updated_at_ms: 1,
    }]);
    pane.push_gateway_edit_panel(
        PlatformControlEntry {
            platform: "weixin".to_string(),
            enabled: false,
            configured: false,
            managed_by: "local".to_string(),
            updated_at_ms: 1,
        },
        PlatformConfigResponse {
            platform: "weixin".to_string(),
            configured: false,
            fields: Vec::new(),
        },
    );

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.no_modal_or_popup_active());
}

#[test]
fn gateway_select_keeps_list_parent_for_edit_back_navigation() {
    let mut pane = InputPane::new();
    let entry = PlatformControlEntry {
        platform: "weixin".to_string(),
        enabled: false,
        configured: false,
        managed_by: "local".to_string(),
        updated_at_ms: 1,
    };
    pane.set_gateway_list_panel(vec![entry.clone()]);

    let action = pane.handle_key(enter_key());
    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::GatewaySelect(platform)))
        if platform == "weixin"
    ));

    pane.push_gateway_edit_panel(
        entry,
        PlatformConfigResponse {
            platform: "weixin".to_string(),
            configured: false,
            fields: Vec::new(),
        },
    );
    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.no_modal_or_popup_active());
}

#[test]
fn esc_returns_from_weixin_binding_to_gateway_page() {
    let mut pane = InputPane::new();
    pane.set_gateway_list_panel(vec![PlatformControlEntry {
        platform: "weixin".to_string(),
        enabled: false,
        configured: false,
        managed_by: "local".to_string(),
        updated_at_ms: 1,
    }]);
    pane.push_gateway_edit_panel(
        PlatformControlEntry {
            platform: "weixin".to_string(),
            enabled: false,
            configured: false,
            managed_by: "local".to_string(),
            updated_at_ms: 1,
        },
        PlatformConfigResponse {
            platform: "weixin".to_string(),
            configured: false,
            fields: Vec::new(),
        },
    );
    pane.push_weixin_binding_view(WeixinBindingViewModel {
        platform: "weixin".to_string(),
        session_id: "session-1".to_string(),
        qr_url: "https://example.com/qr".to_string(),
        status: "waiting".to_string(),
    });

    let action = pane.handle_key(esc_key());

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(!pane.no_modal_or_popup_active());
}
