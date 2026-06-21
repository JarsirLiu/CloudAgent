use crate::app::TuiApp;
use crate::state::NoticeLevel;
use crate::ui::bottom_pane::dialogs::selection::session_picker::SessionPickerMode;
use crate::ui::bottom_pane::input_pane::InputPaneAction;
use agent_core::ConversationSummary;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn test_app() -> TuiApp {
    TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "WorkspaceWrite".to_string(),
    )
}

fn mark_running(app: &mut TuiApp) {
    app.apply_conversation_view_snapshot(running_snapshot(&app.conversation_id));
    app.bottom_pane.on_turn_started();
}

fn running_snapshot(conversation_id: &str) -> agent_protocol::ConversationViewSnapshot {
    agent_protocol::ConversationViewSnapshot {
        conversation_id: conversation_id.to_string(),
        status: agent_protocol::ConversationViewStatus::Active {
            active_turn_id: None,
            flags: vec![agent_protocol::ConversationActiveFlag::RunningTurn],
        },
        active_turn: None,
        pending_requests: Vec::new(),
        message_count: 0,
        updated_at_ms: 0,
    }
}

fn summary(id: &str) -> ConversationSummary {
    ConversationSummary {
        conversation_id: id.to_string(),
        title: Some(id.to_string()),
        message_count: 1,
        updated_at_ms: 1,
    }
}

#[test]
fn requested_session_picker_opens_after_loading_view_remains_active() {
    let mut app = test_app();
    app.bottom_pane
        .request_session_picker(SessionPickerMode::Switch);

    assert!(!app.bottom_pane.no_modal_or_popup_active());
    assert!(app.bottom_pane.present_requested_session_picker_page(
        vec![summary("default")],
        "default",
        false,
        None
    ));
    assert!(!app.bottom_pane.no_modal_or_popup_active());
}

#[test]
fn cancelled_session_picker_loading_ignores_late_response() {
    let mut app = test_app();
    app.bottom_pane
        .request_session_picker(SessionPickerMode::Switch);

    let action = app
        .bottom_pane
        .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(
            crate::input::intent::ComposerIntent::None
        ))
    ));
    assert!(app.bottom_pane.no_modal_or_popup_active());
    assert!(!app.bottom_pane.present_requested_session_picker_page(
        vec![summary("default")],
        "default",
        false,
        None
    ));
    assert!(app.bottom_pane.no_modal_or_popup_active());
}

#[test]
fn active_tool_status_overrides_live_label() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 1;
    mark_running(&mut app);
    app.bottom_pane
        .live_label_override_for_test(Some("Working".to_string()));
    app.bottom_pane
        .active_tool_title_override_for_test(Some("running command: rg cli".to_string()));

    let status = app.bottom_pane.build_status_view_model(&app);

    assert_eq!(status.text, "Working");
    assert_eq!(status.indicator.as_deref(), Some("⠙"));
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: rg cli")
    );
    assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
}

#[test]
fn command_output_delta_stays_in_runtime_banner() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 1;
    mark_running(&mut app);
    app.bottom_pane.on_active_item_started(
        "cmd-1",
        &agent_core::TurnItemKind::CommandExecution,
        Some("rg TODO"),
    );
    app.bottom_pane
        .on_command_output_delta(Some("cmd-1"), "src/main.rs:12: TODO clean this up\n");

    let status = app.bottom_pane.build_status_view_model(&app);

    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: rg TODO · src/main.rs:12: TODO clean this up")
    );

    app.bottom_pane.on_command_finished("cmd-1");
    let after = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(after.live_banner, None);
}

#[test]
fn web_search_delta_updates_runtime_banner() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 1;
    mark_running(&mut app);
    app.bottom_pane.on_active_item_started(
        "ws-1",
        &agent_core::TurnItemKind::ToolResult,
        Some("web_search"),
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(status.live_banner.as_deref(), Some("Web search"));

    app.bottom_pane
        .on_tool_output_delta(Some("ws-1"), "weather seattle");
    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("Web search · weather seattle")
    );

    app.bottom_pane.on_tool_finished_for_item(Some("ws-1"));
    let after = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(after.live_banner, None);
}

#[test]
fn command_output_delta_keeps_recent_tail_compact() {
    let mut app = test_app();
    mark_running(&mut app);
    app.bottom_pane.on_active_item_started(
        "cmd-1",
        &agent_core::TurnItemKind::CommandExecution,
        Some("long command"),
    );

    app.bottom_pane
        .on_command_output_delta(Some("cmd-1"), &"alpha ".repeat(80));
    app.bottom_pane
        .on_command_output_delta(Some("cmd-1"), "omega");

    let status = app.bottom_pane.build_status_view_model(&app);
    let banner = status.live_banner.expect("command banner");
    assert!(banner.starts_with("running command: long command · …"));
    assert!(banner.ends_with("omega"));
    assert!(banner.chars().count() <= "running command: long command · ".chars().count() + 121);
}

#[test]
fn stale_command_output_delta_does_not_update_current_banner() {
    let mut app = test_app();
    mark_running(&mut app);
    app.bottom_pane.on_active_item_started(
        "cmd-current",
        &agent_core::TurnItemKind::CommandExecution,
        Some("cargo check"),
    );

    app.bottom_pane
        .on_command_output_delta(Some("cmd-old"), "old command output");

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: cargo check")
    );
}

#[test]
fn stale_command_finish_does_not_clear_current_banner() {
    let mut app = test_app();
    mark_running(&mut app);
    app.bottom_pane.on_active_item_started(
        "cmd-current",
        &agent_core::TurnItemKind::CommandExecution,
        Some("cargo test"),
    );

    app.bottom_pane.on_command_finished("cmd-old");

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: cargo test")
    );
}

#[test]
fn in_progress_completion_keeps_command_runtime_until_final_completion() {
    let mut app = test_app();
    mark_running(&mut app);
    app.bottom_pane.on_active_item_started(
        "cmd-1",
        &agent_core::TurnItemKind::CommandExecution,
        Some("slow command"),
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: slow command")
    );

    app.bottom_pane
        .on_command_output_delta(Some("cmd-1"), "still running");
    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: slow command · still running")
    );
}

#[test]
fn reconnect_live_label_animates_when_no_active_tool_or_notice() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 2;
    mark_running(&mut app);
    app.bottom_pane.live_label_override_for_test(Some(
        "reconnecting (stream retry 2, next in 1.0s)".to_string(),
    ));

    let status = app.bottom_pane.build_status_view_model(&app);

    assert_eq!(status.text, "Working");
    assert_eq!(status.indicator.as_deref(), Some("⠹"));
    assert_eq!(
        status.live_banner.as_deref(),
        Some("reconnecting (stream retry 2, next in 1.0s)")
    );
    assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
}

#[test]
fn generic_live_label_hides_when_active_cell_is_visible() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 0;
    mark_running(&mut app);
    app.bottom_pane
        .live_label_override_for_test(Some("Thinking".to_string()));
    app.transcript_owner
        .push_live_cell(crate::ui::history_cell::HistoryCell::reasoning(
            "Reasoning",
            "streaming body",
        ));

    let status = app.bottom_pane.build_status_view_model(&app);

    assert_eq!(status.text, "Working");
    assert_eq!(status.live_banner.as_deref(), Some("Thinking"));
    assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
}

#[test]
fn generic_live_label_does_not_render_external_banner_without_active_cell() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 0;
    mark_running(&mut app);
    app.bottom_pane
        .live_label_override_for_test(Some("Thinking".to_string()));

    let status = app.bottom_pane.build_status_view_model(&app);

    assert_eq!(status.text, "Working");
    assert_eq!(status.live_banner.as_deref(), Some("Thinking"));
    assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
}

#[test]
fn working_without_runtime_does_not_show_elapsed_hint() {
    let mut app = test_app();
    app.apply_conversation_view_snapshot(running_snapshot(&app.conversation_id));
    app.bottom_pane
        .live_label_override_for_test(Some("Working".to_string()));

    let status = app.bottom_pane.build_status_view_model(&app);

    assert_eq!(status.text, "Working");
    assert_eq!(status.runtime_hint, None);
    assert_eq!(status.live_banner, None);
}

#[test]
fn compaction_runtime_status_renders_as_live_banner_and_clears_cleanly() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 3;
    mark_running(&mut app);
    app.bottom_pane.on_context_compaction_started(12_345);

    let during = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(during.text, "Working");
    assert_eq!(during.indicator.as_deref(), Some("⠸"));
    assert_eq!(
        during.live_banner.as_deref(),
        Some("Compacting context (~12.3k tokens)")
    );

    app.bottom_pane.on_context_compaction_finished();
    let after = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(after.text, "Working");
    assert_eq!(after.live_banner, None);
}

#[test]
fn transient_notice_renders_above_runtime_banner_and_expires() {
    let mut app = test_app();
    app.run_state.live_animation_frame = 1;
    mark_running(&mut app);
    app.bottom_pane
        .active_tool_title_override_for_test(Some("running command: rg cli".to_string()));
    app.bottom_pane.show_transient_notice(
        NoticeLevel::Info,
        "Deleted conversation `draft-1`".to_string(),
    );

    let during = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        during.live_banner.as_deref(),
        Some("Deleted conversation `draft-1`")
    );
    assert_eq!(during.live_banner_level, Some(NoticeLevel::Info));

    app.bottom_pane.expire_transient_notice_for_test();
    assert!(app.bottom_pane.handle_tick());

    let after = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        after.live_banner.as_deref(),
        Some("running command: rg cli")
    );
    assert_eq!(after.live_banner_level, None);
}
