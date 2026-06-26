use super::*;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn text_items(text: &str) -> Vec<InputItem> {
    vec![InputItem::Text {
        text: text.to_string(),
    }]
}

fn create_test_png() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("cloudagent-test-{nonce}.png"));
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
        .save(&path)
        .expect("save temp image");
    path
}

fn image_path_string(path: &Path) -> String {
    path.display().to_string()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn type_text(composer: &mut ChatComposer, text: &str) {
    for ch in text.chars() {
        composer.handle_key(key(KeyCode::Char(ch)));
        std::thread::sleep(std::time::Duration::from_millis(40));
        let _ = composer.flush_paste_burst_if_due();
    }
}

#[test]
fn slash_opens_completion_and_tab_completes() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "/co");

    assert!(composer.completion.is_active());
    composer.handle_key(key(KeyCode::Tab));
    assert_eq!(composer.textarea.text(), "/copy");
}

#[test]
fn enter_dispatches_selected_completion() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "/co");

    let action = composer.handle_key(key(KeyCode::Enter));
    assert_eq!(action, Some(ComposerIntent::Copy));
    assert!(composer.textarea.is_empty());
}

#[test]
fn dollar_skill_mention_opens_completion_and_inserts_structured_skill() {
    let mut composer = ChatComposer::new();
    composer.set_available_skills(vec![SkillCompletion {
        name: "repo-reader".to_string(),
        description: "Read repository structure".to_string(),
        path: "D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md".to_string(),
    }]);
    type_text(&mut composer, "$rep");

    assert!(composer.completion.is_active());

    let action = composer.handle_key(key(KeyCode::Enter));
    assert_eq!(action, Some(ComposerIntent::None));
    assert_eq!(composer.textarea.text(), "$repo-reader ");

    let submit = composer.handle_key(key(KeyCode::Enter));
    assert_eq!(
        submit,
        Some(ComposerIntent::Submit(vec![InputItem::Skill {
            name: "repo-reader".to_string(),
            path: "D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md".to_string(),
        }]))
    );
}

#[test]
fn exact_slash_command_dispatches_without_reducer_parsing() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "/clear");

    let action = composer.handle_key(key(KeyCode::Enter));
    assert_eq!(action, Some(ComposerIntent::Reset));
}

#[test]
fn leading_space_slash_submits_as_message() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, " /clear");

    let action = composer.handle_key(key(KeyCode::Enter));
    assert_eq!(action, Some(ComposerIntent::Submit(text_items("/clear"))));
}

#[test]
fn completion_popup_does_not_shift_cursor() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "/co");

    let (_, y) = composer.cursor_position(Rect::new(0, 10, 80, 5), FrontendMode::Idle);
    assert_eq!(y, 10);
}

#[test]
fn completion_popup_scrolls_to_selected_command() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "/");
    for _ in 0..4 {
        composer.handle_key(key(KeyCode::Down));
    }

    let rendered = composer.render(FrontendMode::Idle, 80);
    let visible_text = rendered
        .completion_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(visible_text.contains("> /"));
}

#[test]
fn bracketed_paste_inserts_text_without_submitting() {
    let mut composer = ChatComposer::new();

    let action = composer.handle_paste("first line\nsecond line");

    assert_eq!(action, ComposerIntent::None);
    assert_eq!(composer.textarea.text(), "first line\nsecond line");
}

#[test]
fn bracketed_paste_only_submits_after_explicit_enter() {
    let mut composer = ChatComposer::new();

    let paste_action = composer.handle_paste("first line\nsecond line");
    let submit_action = composer.handle_key(key(KeyCode::Enter));

    assert_eq!(paste_action, ComposerIntent::None);
    assert_eq!(
        submit_action,
        Some(ComposerIntent::Submit(text_items(
            "first line\nsecond line"
        )))
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn trailing_space_remains_visible_in_rendered_composer() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "abc ");

    let rendered = composer.render(FrontendMode::Idle, 80);
    let visible_text = rendered.lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(
        visible_text.ends_with("abc "),
        "expected rendered composer to preserve trailing space, got {visible_text:?}"
    );
}

#[test]
fn trailing_space_wraps_into_continuation_row() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "abc ");

    let visible_lines = composer.textarea.wrapped_lines(composer.textarea.text(), 3);

    assert_eq!(visible_lines.len(), 2);
    assert_eq!(visible_lines[0], "abc");
    assert_eq!(visible_lines[1], " ");
}

#[test]
fn long_multiline_input_caps_visible_height() {
    let mut composer = ChatComposer::new();
    let text = (1..=20)
        .map(|idx| format!("line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = composer.handle_paste(&text);

    let rendered = composer.render(FrontendMode::Idle, 80);

    assert_eq!(rendered.height, MAX_VISIBLE_COMPOSER_ROWS as u16);
    assert_eq!(rendered.lines.len(), MAX_VISIBLE_COMPOSER_ROWS);
    assert_eq!(rendered.cursor_row, MAX_VISIBLE_COMPOSER_ROWS as u16 - 1);
}

#[test]
fn shift_enter_inserts_newline_without_submitting() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");

    let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    type_text(&mut composer, "second");

    assert_eq!(action, None);
    assert_eq!(composer.textarea.text(), "first\nsecond");
}

#[test]
fn shift_enter_is_treated_as_newline_shortcut_even_with_extra_state() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");

    let action = composer.handle_key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::SHIFT,
        kind: crossterm::event::KeyEventKind::Press,
        state: crossterm::event::KeyEventState::CAPS_LOCK,
    });
    type_text(&mut composer, "second");

    assert_eq!(action, None);
    assert_eq!(composer.textarea.text(), "first\nsecond");
}

#[test]
fn alt_enter_inserts_newline_without_submitting() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");

    let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    type_text(&mut composer, "second");

    assert_eq!(action, None);
    assert_eq!(composer.textarea.text(), "first\nsecond");
}

#[test]
fn ctrl_enter_inserts_newline_without_submitting() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");

    let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    type_text(&mut composer, "second");

    assert_eq!(action, None);
    assert_eq!(composer.textarea.text(), "first\nsecond");
}

#[test]
fn manual_newline_shortcut_submits_multiline_text_only_after_plain_enter() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");

    let newline_action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    type_text(&mut composer, "second");
    let submit_action = composer.handle_key(key(KeyCode::Enter));

    assert_eq!(newline_action, None);
    assert_eq!(
        submit_action,
        Some(ComposerIntent::Submit(text_items("first\nsecond")))
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn pasted_image_path_attaches_placeholder_without_inserting_raw_path() {
    let mut composer = ChatComposer::new();
    let image_path = create_test_png();

    let action = composer.handle_paste(&image_path_string(&image_path));

    assert_eq!(action, ComposerIntent::None);
    assert_eq!(composer.textarea.text(), "[Image #1]");
    assert_eq!(composer.attached_image_paths(), vec![image_path]);
}

#[test]
fn submit_preserves_text_and_image_order() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "look at this");
    let image_path = create_test_png();
    composer.attach_image(image_path.clone());
    type_text(&mut composer, "please");

    let action = composer.handle_key(key(KeyCode::Enter));

    assert_eq!(
        action,
        Some(ComposerIntent::Submit(vec![
            InputItem::Text {
                text: "look at this".to_string(),
            },
            InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: image_path_string(&image_path),
                },
                detail: None,
                alt: None,
            },
            InputItem::Text {
                text: "please".to_string(),
            },
        ]))
    );
}

#[test]
fn submit_preserves_text_skill_and_image_order() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "use this");
    composer.attach_skill(
        "repo-reader".to_string(),
        "D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md".to_string(),
    );
    let image_path = create_test_png();
    composer.attach_image(image_path.clone());
    type_text(&mut composer, "now");

    let action = composer.handle_key(key(KeyCode::Enter));

    assert_eq!(
        action,
        Some(ComposerIntent::Submit(vec![
            InputItem::Text {
                text: "use this".to_string(),
            },
            InputItem::Skill {
                name: "repo-reader".to_string(),
                path: "D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md".to_string(),
            },
            InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: image_path_string(&image_path),
                },
                detail: None,
                alt: None,
            },
            InputItem::Text {
                text: "now".to_string(),
            },
        ]))
    );
}

#[test]
fn deleting_first_placeholder_renumbers_remaining_images() {
    let mut composer = ChatComposer::new();
    let first = create_test_png();
    let second = create_test_png();

    composer.attach_image(first);
    composer.attach_image(second.clone());
    composer.textarea.handle_key(key(KeyCode::Home));
    composer.handle_key(key(KeyCode::Delete));

    assert_eq!(composer.textarea.text(), "[Image #1]");
    assert_eq!(composer.attached_image_paths(), vec![second]);
}

#[test]
fn backspace_deletes_image_placeholder_atomically() {
    let mut composer = ChatComposer::new();
    let image_path = create_test_png();
    composer.attach_image(image_path);

    composer.handle_key(key(KeyCode::Backspace));

    assert!(composer.textarea.is_empty());
    assert!(composer.attached_image_paths().is_empty());
}

#[test]
fn delete_deletes_image_placeholder_atomically() {
    let mut composer = ChatComposer::new();
    let image_path = create_test_png();
    composer.attach_image(image_path);
    composer.textarea.handle_key(key(KeyCode::Home));

    composer.handle_key(key(KeyCode::Delete));

    assert!(composer.textarea.is_empty());
    assert!(composer.attached_image_paths().is_empty());
}

#[test]
fn local_and_remote_images_share_numbering_and_submit_order() {
    let mut composer = ChatComposer::new();
    let local_path = create_test_png();
    composer.attach_remote_image("https://example.com/a.png");
    composer.attach_image(local_path.clone());
    type_text(&mut composer, "describe both");

    assert_eq!(
        composer.textarea.text(),
        "[Image #1][Image #2]describe both"
    );

    let action = composer.handle_key(key(KeyCode::Enter));

    assert_eq!(
        action,
        Some(ComposerIntent::Submit(vec![
            InputItem::Image {
                source: AttachmentRef::RemoteUrl {
                    url: "https://example.com/a.png".to_string(),
                },
                detail: None,
                alt: None,
            },
            InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: image_path_string(&local_path),
                },
                detail: None,
                alt: None,
            },
            InputItem::Text {
                text: "describe both".to_string(),
            },
        ]))
    );
}

#[test]
fn deleting_remote_image_relabels_local_images() {
    let mut composer = ChatComposer::new();
    let local_path = create_test_png();
    composer.attach_remote_image("https://example.com/a.png");
    composer.attach_image(local_path.clone());
    composer.textarea.handle_key(key(KeyCode::Home));

    composer.handle_key(key(KeyCode::Delete));

    assert_eq!(composer.textarea.text(), "[Image #1]");
    assert_eq!(composer.attached_remote_image_urls(), Vec::<String>::new());
    assert_eq!(composer.attached_image_paths(), vec![local_path]);
}

#[test]
fn ctrl_a_selects_all_current_draft() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "alpha\nbeta");

    composer.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert_eq!(
        composer.textarea.selected_text().as_deref(),
        Some("alpha\nbeta")
    );
}

#[test]
fn ctrl_a_then_ctrl_x_cuts_entire_draft_and_returns_copy_intent() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "alpha\nbeta");

    composer.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    let action = composer.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));

    assert_eq!(
        action,
        Some(ComposerIntent::CopyText("alpha\nbeta".to_string()))
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn ctrl_d_still_exits_even_with_existing_text() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "alpha");

    let action = composer.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));

    assert_eq!(action, Some(ComposerIntent::Exit));
    assert_eq!(composer.textarea.text(), "alpha");
}

#[test]
fn esc_with_existing_text_is_not_consumed_by_composer() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "alpha");

    let action = composer.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(action, None);
    assert_eq!(composer.textarea.text(), "alpha");
}

#[test]
fn placeholder_cursor_stays_at_input_start() {
    let composer = ChatComposer::new();
    let (x, y) = composer.cursor_position(Rect::new(0, 10, 80, 5), FrontendMode::Idle);
    assert_eq!(y, 10);
    assert_eq!(x, 4);
}

#[test]
fn down_moves_to_next_line_and_then_end_of_text() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first\nsecond");

    composer.handle_key(key(KeyCode::Home));
    composer.handle_key(key(KeyCode::Down));
    assert_eq!(
        composer
            .textarea
            .text()
            .chars()
            .take(composer.textarea.cursor())
            .collect::<String>(),
        "first\n"
    );

    composer.handle_key(key(KeyCode::Down));
    assert_eq!(
        composer.textarea.cursor(),
        composer.textarea.text().chars().count()
    );
}

#[test]
fn history_navigation_only_activates_for_empty_or_recalled_boundary_text() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");
    let _ = composer.handle_key(key(KeyCode::Enter));
    type_text(&mut composer, "second");
    let _ = composer.handle_key(key(KeyCode::Enter));

    composer.handle_key(key(KeyCode::Up));
    assert_eq!(composer.textarea.text(), "second");

    composer.handle_key(key(KeyCode::Home));
    composer.handle_key(key(KeyCode::Up));
    assert_eq!(composer.textarea.text(), "first");

    composer.handle_key(key(KeyCode::Down));
    assert_eq!(composer.textarea.text(), "second");

    composer.handle_key(key(KeyCode::End));
    type_text(&mut composer, "!");
    composer.handle_key(key(KeyCode::Up));
    assert_eq!(composer.textarea.text(), "second!");
}

#[test]
fn down_past_newest_history_clears_composer() {
    let mut composer = ChatComposer::new();
    type_text(&mut composer, "first");
    let _ = composer.handle_key(key(KeyCode::Enter));

    composer.handle_key(key(KeyCode::Up));
    assert_eq!(composer.textarea.text(), "first");

    composer.handle_key(key(KeyCode::Down));
    assert!(composer.textarea.is_empty());
}

#[test]
fn single_plain_char_is_flushed_on_tick() {
    let mut composer = ChatComposer::new();

    let action = composer.handle_key(key(KeyCode::Char('a')));
    assert_eq!(action, Some(ComposerIntent::None));
    assert_eq!(composer.textarea.text(), "");

    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(composer.flush_paste_burst_if_due());
    assert_eq!(composer.textarea.text(), "a");
}

#[test]
fn two_fast_chars_flush_as_paste() {
    let mut composer = ChatComposer::new();

    let _ = composer.handle_key(key(KeyCode::Char('a')));
    let _ = composer.handle_key(key(KeyCode::Char('b')));
    assert_eq!(composer.textarea.text(), "");

    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(composer.flush_paste_burst_if_due());
    assert_eq!(composer.textarea.text(), "ab");
}

#[test]
fn enter_during_paste_burst_does_not_submit_multiline_text() {
    let mut composer = ChatComposer::new();

    let _ = composer.handle_key(key(KeyCode::Char('a')));
    let _ = composer.handle_key(key(KeyCode::Char('b')));
    let action = composer.handle_key(key(KeyCode::Enter));

    assert_eq!(action, Some(ComposerIntent::None));
    assert_eq!(composer.textarea.text(), "");

    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(composer.flush_paste_burst_if_due());
    assert_eq!(composer.textarea.text(), "ab\n");
}

#[test]
fn paste_burst_with_multiple_newlines_does_not_submit() {
    let mut composer = ChatComposer::new();

    let _ = composer.handle_key(key(KeyCode::Char('a')));
    let _ = composer.handle_key(key(KeyCode::Char('b')));
    let first_enter = composer.handle_key(key(KeyCode::Enter));
    let _ = composer.handle_key(key(KeyCode::Char('c')));
    let second_enter = composer.handle_key(key(KeyCode::Enter));
    let _ = composer.handle_key(key(KeyCode::Char('d')));

    assert_eq!(first_enter, Some(ComposerIntent::None));
    assert_eq!(second_enter, Some(ComposerIntent::None));
    assert_eq!(composer.textarea.text(), "");

    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(composer.flush_paste_burst_if_due());
    assert_eq!(composer.textarea.text(), "ab\nc\nd");
}
