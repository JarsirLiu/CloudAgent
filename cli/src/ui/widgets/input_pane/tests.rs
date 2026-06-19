use super::*;
use super::render::input_block;
use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Widget;

fn command_request(id: &str, title: &str) -> ServerRequestInlineState {
    ServerRequestInlineState {
        request_id: RequestId::String(id.to_string()),
        presentation: crate::ui::widgets::server_request_model::ServerRequestPresentation::command(
            title,
            "needs approval",
            "Get-Content file.rs",
        ),
    }
}

#[test]
fn esc_is_consumed_when_server_request_overlay_is_active() {
    let mut pane = InputPane::new();
    pane.set_server_request(command_request("req-1", "Run command?"));

    let action = pane.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
    assert!(pane.requires_action());
    assert_eq!(
        pane.active_server_request_id(),
        Some(RequestId::String("req-1".to_string()))
    );
}

#[test]
fn server_request_overlay_queues_new_requests_instead_of_replacing_current() {
    let mut pane = InputPane::new();
    pane.set_server_request(command_request("req-1", "First command"));
    pane.set_server_request(command_request("req-2", "Second command"));

    let first = pane.handle_key(KeyEvent {
        code: KeyCode::Char('1'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });
    let second = pane.handle_key(KeyEvent {
        code: KeyCode::Char('3'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        first,
        Some(InputPaneAction::ServerRequestSubmit {
            request_id: RequestId::String(id),
            decision: ServerRequestDecisionKind::Accept,
            ..
        }) if id == "req-1"
    ));
    assert!(matches!(
        second,
        Some(InputPaneAction::ServerRequestSubmit {
            request_id: RequestId::String(id),
            decision: ServerRequestDecisionKind::Decline,
            ..
        }) if id == "req-2"
    ));
}

#[test]
fn queued_server_request_remains_action_required_after_first_submit() {
    let mut pane = InputPane::new();
    pane.set_server_request(command_request("req-1", "First command"));
    pane.set_server_request(command_request("req-2", "Second command"));

    let _ = pane.handle_key(KeyEvent {
        code: KeyCode::Char('1'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(pane.requires_action());
    assert_eq!(
        pane.active_server_request_id(),
        Some(RequestId::String("req-2".to_string()))
    );
}

#[test]
fn approval_selection_mode_does_not_force_a_text_cursor() {
    let mut pane = InputPane::new();
    pane.set_server_request(command_request("req-1", "Run command?"));

    assert_eq!(
        pane.cursor_position(
            Rect::new(0, 20, 100, 8),
            1,
            FrontendMode::WaitingForServerRequest
        ),
        None
    );
}

#[test]
fn completion_popup_is_part_of_input_pane_height() {
    let mut pane = InputPane::new();
    let before = pane.desired_height(FrontendMode::Idle, 100);
    assert_eq!(before, 6);

    let _ = pane.handle_key(KeyEvent {
        code: KeyCode::Char('/'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    let after = pane.desired_height(FrontendMode::Idle, 100);
    assert!(after > before);
    let (lines, _) = pane.render_lines_for_test(FrontendMode::Idle, "Idle", "test", 100);
    assert!(!lines.is_empty());
}

#[test]
fn completion_popup_area_stays_inside_input_pane() {
    let mut pane = InputPane::new();
    let _ = pane.handle_key(KeyEvent {
        code: KeyCode::Char('/'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    let height = pane.desired_height(FrontendMode::Idle, 100);
    let snapshot = pane.build_snapshot(
        Rect::new(0, 10, 100, height),
        FrontendMode::Idle,
        None,
        "",
        None,
        "",
        "",
        98,
    );
    let completion_area = snapshot
        .layout
        .completion_area
        .expect("completion popup should render");

    assert_eq!(snapshot.layout.input_area.y, 10);
    assert!(completion_area.y >= 10);
    assert!(completion_area.bottom() <= 10 + height);
    assert_eq!(snapshot.height, height);
}

#[test]
fn esc_interrupts_even_when_composer_has_text() {
    let mut pane = InputPane::new();
    let _ = pane.handle_paste("draft message");

    let action = pane.handle_key(KeyEvent {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::Interrupt))
    ));
}

#[test]
fn esc_closes_completion_menu_before_interrupting() {
    let mut pane = InputPane::new();
    let _ = pane.handle_key(KeyEvent {
        code: KeyCode::Char('/'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        pane.handle_key(KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }),
        Some(InputPaneAction::Composer(ComposerIntent::None))
    ));
}

#[test]
fn ctrl_d_exits_even_when_composer_has_text() {
    let mut pane = InputPane::new();
    let _ = pane.handle_paste("draft message");

    let action = pane.handle_key(KeyEvent {
        code: KeyCode::Char('d'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert!(matches!(
        action,
        Some(InputPaneAction::Composer(ComposerIntent::Exit))
    ));
}

#[test]
fn input_block_preserves_trailing_space_without_extra_wrapped_row() {
    let area = Rect::new(0, 0, 10, 4);
    let mut buf = Buffer::empty(area);
    let widget = input_block(vec![Line::raw("abc ")], Style::default());

    widget.render(area, &mut buf);

    let content_row = (1..area.width.saturating_sub(1))
        .map(|x| buf[(x, 1)].symbol())
        .collect::<String>();
    let next_row = (1..area.width.saturating_sub(1))
        .map(|x| buf[(x, 2)].symbol())
        .collect::<String>();

    assert!(content_row.starts_with("abc "));
    assert_eq!(next_row.trim(), "");
}
