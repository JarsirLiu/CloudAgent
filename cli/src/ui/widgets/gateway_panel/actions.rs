use super::state::{EditableField, GatewayPanelMode};
use super::GatewayPanel;
use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::BottomPaneViewAction;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn move_selection(panel: &mut GatewayPanel, delta: i32) {
    match &mut panel.mode {
        GatewayPanelMode::List { entries, selected } => {
            if entries.is_empty() {
                *selected = 0;
                return;
            }
            let max = entries.len().saturating_sub(1) as i32;
            *selected = (*selected as i32 + delta).clamp(0, max) as usize;
        }
        GatewayPanelMode::Edit {
            selected,
            fields,
            platform,
            ..
        } => {
            let extra_actions = if platform == "weixin" { 2 } else { 3 };
            let max = (fields.len() + extra_actions - 1) as i32;
            let next = (*selected as i32 + delta).clamp(0, max) as usize;
            if next != *selected {
                *selected = next;
            }
        }
    }
}

fn collect_updates(fields: &[EditableField]) -> Vec<crate::input::intent::GatewayConfigUpdate> {
    GatewayPanelMode::collect_updates(fields)
}

pub(crate) fn handle_paste(panel: &mut GatewayPanel, text: &str) -> BottomPaneViewAction {
    if let GatewayPanelMode::Edit {
        selected, fields, ..
    } = &mut panel.mode
        && *selected < fields.len()
    {
        let field = &mut fields[*selected];
        let _ = field.input.append_paste(text);
    }
    BottomPaneViewAction::None
}

pub(crate) fn handle_key_event(panel: &mut GatewayPanel, key: KeyEvent) -> BottomPaneViewAction {
    if !matches!(key.kind, KeyEventKind::Press) {
        return BottomPaneViewAction::None;
    }
    if key.modifiers == KeyModifiers::CONTROL
        && key.code == KeyCode::Char('a')
        && let GatewayPanelMode::Edit {
            selected, fields, ..
        } = &mut panel.mode
        && *selected < fields.len()
    {
        fields[*selected].input.select_all();
        return BottomPaneViewAction::None;
    }
    match &mut panel.mode {
        GatewayPanelMode::List { entries, selected } => match key.code {
            KeyCode::Up => move_selection(panel, -1),
            KeyCode::Down | KeyCode::Tab => move_selection(panel, 1),
            KeyCode::BackTab => move_selection(panel, -1),
            KeyCode::Enter => {
                if let Some(entry) = entries.get(*selected) {
                    return BottomPaneViewAction::ComposerWithoutDismiss(
                        ComposerIntent::GatewaySelect(entry.platform.clone()),
                    );
                }
            }
            KeyCode::Esc => return BottomPaneViewAction::Cancel,
            _ => {}
        },
        GatewayPanelMode::Edit {
            platform,
            enabled,
            configured,
            selected,
            fields,
            weixin_login,
            ..
        } => match key.code {
            KeyCode::Up => move_selection(panel, -1),
            KeyCode::Down | KeyCode::Tab => move_selection(panel, 1),
            KeyCode::BackTab => move_selection(panel, -1),
            KeyCode::Left if *selected < fields.len() => fields[*selected].input.move_left(),
            KeyCode::Right if *selected < fields.len() => {
                fields[*selected].input.move_right();
            }
            KeyCode::Home if *selected < fields.len() => {
                fields[*selected].input.move_to_start();
            }
            KeyCode::End if *selected < fields.len() => fields[*selected].input.move_to_end(),
            KeyCode::Backspace if *selected < fields.len() => {
                fields[*selected].input.backspace();
            }
            KeyCode::Delete if *selected < fields.len() => {
                fields[*selected].input.delete();
            }
            KeyCode::Char(' ') => {
                if *selected == fields.len() {
                    *enabled = !*enabled;
                } else {
                    let _ = fields[*selected].input.append_char(' ');
                }
            }
            KeyCode::Char(c) if *selected < fields.len() => {
                let _ = fields[*selected].input.append_char(c);
            }
            KeyCode::Enter => {
                let toggle_index = fields.len();
                let save_index = fields.len() + 1;
                let back_index = if platform == "weixin" {
                    fields.len() + 1
                } else {
                    fields.len() + 2
                };
                if *selected == toggle_index {
                    if platform == "weixin" && !*enabled && !*configured {
                        if let Some(session) = weixin_login.clone() {
                            return BottomPaneViewAction::ComposerWithoutDismiss(
                                ComposerIntent::GatewayWeixinLoginCheck {
                                    platform: platform.clone(),
                                    session_id: session.session_id,
                                    qr_url: session.qr_url,
                                },
                            );
                        }
                        return BottomPaneViewAction::ComposerWithoutDismiss(
                            ComposerIntent::GatewayWeixinLoginStart {
                                platform: platform.clone(),
                            },
                        );
                    }
                    return BottomPaneViewAction::ComposerWithoutDismiss(
                        ComposerIntent::GatewaySave {
                            platform: platform.clone(),
                            enabled: !*enabled,
                            updates: collect_updates(fields),
                        },
                    );
                } else if platform != "weixin" && *selected == save_index {
                    let updates = collect_updates(fields);
                    return BottomPaneViewAction::ComposerWithoutDismiss(
                        ComposerIntent::GatewaySave {
                            platform: platform.clone(),
                            enabled: *enabled,
                            updates,
                        },
                    );
                } else if *selected == back_index {
                    return BottomPaneViewAction::Back;
                } else {
                    move_selection(panel, 1);
                }
            }
            KeyCode::Esc => return BottomPaneViewAction::Back,
            _ => {}
        },
    }
    BottomPaneViewAction::None
}
