use crate::input::intent::{ComposerIntent, GatewayConfigUpdate};
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use agent_protocol::{PlatformConfigResponse, PlatformControlEntry};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct GatewayPanel {
    mode: GatewayPanelMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinLoginSessionView {
    pub session_id: String,
    pub qr_url: String,
}

enum GatewayPanelMode {
    List {
        entries: Vec<PlatformControlEntry>,
        selected: usize,
    },
    Edit {
        platform: String,
        enabled: bool,
        configured: bool,
        selected: usize,
        fields: Vec<EditableField>,
        replace_on_next_input: bool,
        weixin_login: Option<WeixinLoginSessionView>,
    },
}

struct EditableField {
    key: String,
    value: String,
    required: bool,
    is_secret: bool,
    was_set: bool,
    dirty: bool,
}

impl GatewayPanel {
    pub fn list(entries: Vec<PlatformControlEntry>) -> Self {
        Self {
            mode: GatewayPanelMode::List {
                entries,
                selected: 0,
            },
        }
    }

    pub fn edit(
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
        weixin_login: Option<WeixinLoginSessionView>,
    ) -> Self {
        let fields = config
            .fields
            .into_iter()
            .map(|field| EditableField {
                key: field.key,
                value: field.value.unwrap_or_default(),
                required: field.required,
                is_secret: field.is_secret,
                was_set: field.is_set,
                dirty: false,
            })
            .collect();
        Self {
            mode: GatewayPanelMode::Edit {
                platform: entry.platform,
                enabled: entry.enabled,
                configured: config.configured,
                selected: 0,
                fields,
                replace_on_next_input: true,
                weixin_login,
            },
        }
    }

    fn move_selection(&mut self, delta: i32) {
        match &mut self.mode {
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
                replace_on_next_input,
                platform,
                configured: _,
                enabled: _,
                ..
            } => {
                let extra_actions = if platform == "weixin" { 2 } else { 3 };
                let max = (fields.len() + extra_actions - 1) as i32;
                let next = (*selected as i32 + delta).clamp(0, max) as usize;
                if next != *selected {
                    *selected = next;
                    *replace_on_next_input = true;
                }
            }
        }
    }

    fn current_field_mut(&mut self) -> Option<&mut EditableField> {
        let GatewayPanelMode::Edit {
            selected,
            fields,
            replace_on_next_input,
            ..
        } = &mut self.mode
        else {
            return None;
        };
        if *selected >= fields.len() {
            return None;
        }
        let field = &mut fields[*selected];
        if *replace_on_next_input {
            field.value.clear();
            field.dirty = true;
            *replace_on_next_input = false;
        }
        Some(field)
    }

    fn collect_updates(fields: &[EditableField]) -> Vec<GatewayConfigUpdate> {
        fields
            .iter()
            .filter(|field| field.dirty)
            .map(|field| GatewayConfigUpdate {
                key: field.key.clone(),
                value: if field.value.trim().is_empty() {
                    None
                } else {
                    Some(field.value.trim().to_string())
                },
            })
            .collect()
    }
}

impl BottomPaneView for GatewayPanel {
    fn handle_paste(&mut self, text: &str) -> BottomPaneViewAction {
        if let Some(field) = self.current_field_mut() {
            field.value.push_str(&text.replace('\n', ""));
            field.dirty = true;
        }
        BottomPaneViewAction::None
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        match &mut self.mode {
            GatewayPanelMode::List { entries, selected } => match key.code {
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Down | KeyCode::Tab => self.move_selection(1),
                KeyCode::BackTab => self.move_selection(-1),
                KeyCode::Enter => {
                    if let Some(entry) = entries.get(*selected) {
                        return BottomPaneViewAction::Composer(ComposerIntent::GatewaySelect(
                            entry.platform.clone(),
                        ));
                    }
                }
                KeyCode::Esc => return BottomPaneViewAction::Close,
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
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Down | KeyCode::Tab => self.move_selection(1),
                KeyCode::BackTab => self.move_selection(-1),
                KeyCode::Backspace => {
                    if let Some(field) = self.current_field_mut() {
                        field.value.pop();
                        field.dirty = true;
                    }
                }
                KeyCode::Char(' ') => {
                    if *selected == fields.len() {
                        *enabled = !*enabled;
                    } else if let Some(field) = self.current_field_mut() {
                        field.value.push(' ');
                        field.dirty = true;
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(field) = self.current_field_mut() {
                        field.value.push(c);
                        field.dirty = true;
                    }
                }
                KeyCode::Enter => {
                    let toggle_index = fields.len();
                    let save_index = fields.len() + 1;
                    let back_index = if platform == "weixin" { fields.len() + 1 } else { fields.len() + 2 };
                    if *selected == toggle_index {
                        if platform == "weixin" && !*enabled && !*configured {
                            if let Some(session) = weixin_login.clone() {
                                return BottomPaneViewAction::Composer(
                                    ComposerIntent::GatewayWeixinLoginCheck {
                                        platform: platform.clone(),
                                        session_id: session.session_id,
                                        qr_url: session.qr_url,
                                    },
                                );
                            }
                            return BottomPaneViewAction::Composer(
                                ComposerIntent::GatewayWeixinLoginStart {
                                    platform: platform.clone(),
                                },
                            );
                        }
                        return BottomPaneViewAction::Composer(ComposerIntent::GatewaySave {
                            platform: platform.clone(),
                            enabled: !*enabled,
                            updates: Self::collect_updates(fields),
                        });
                    } else if platform != "weixin" && *selected == save_index {
                        let updates = Self::collect_updates(fields);
                        return BottomPaneViewAction::Composer(ComposerIntent::GatewaySave {
                            platform: platform.clone(),
                            enabled: *enabled,
                            updates,
                        });
                    } else if *selected == back_index {
                        return BottomPaneViewAction::Composer(ComposerIntent::Gateway);
                    } else {
                        self.move_selection(1);
                    }
                }
                KeyCode::Esc => return BottomPaneViewAction::Composer(ComposerIntent::Gateway),
                _ => {}
            },
        }
        BottomPaneViewAction::None
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let selected_style = Style::default()
            .fg(Color::Rgb(190, 220, 255))
            .add_modifier(Modifier::BOLD);
        let normal_style = Style::default().fg(Color::Rgb(140, 150, 180));
        match &self.mode {
            GatewayPanelMode::List { entries, selected } => {
                let mut lines = vec![
                    Line::from("  Gateway Panel"),
                    Line::from("  Enter to edit a platform, Esc to close"),
                    Line::from(
                        "  List shows connection state only; edit a platform to view or change config",
                    ),
                ];
                for (index, entry) in entries.iter().enumerate() {
                    let marker = if index == *selected { ">" } else { " " };
                    let connection = if entry.enabled { "enabled" } else { "disabled" };
                    let config = if entry.configured {
                        "configured"
                    } else {
                        "incomplete"
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!("{marker} {:<8}", entry.platform),
                            if index == *selected {
                                selected_style
                            } else {
                                normal_style
                            },
                        ),
                        Span::raw(" "),
                        Span::styled(
                            connection.to_string(),
                            Style::default().fg(if entry.enabled {
                                Color::LightGreen
                            } else {
                                Color::Gray
                            }),
                        ),
                        Span::raw(" · "),
                        Span::styled(
                            config.to_string(),
                            Style::default().fg(if entry.configured {
                                Color::LightCyan
                            } else {
                                Color::DarkGray
                            }),
                        ),
                    ]));
                }
                lines
            }
            GatewayPanelMode::Edit {
                platform,
                enabled,
                configured,
                selected,
                fields,
                weixin_login,
                ..
            } => {
                let mut lines = vec![
                    Line::from(format!("  Gateway Panel · {platform}")),
                    Line::from(if platform == "weixin" {
                        "  Manage the connection here. Esc returns to the platform list"
                    } else {
                        "  Type or paste values, Tab/Up/Down switch fields, Esc returns to list"
                    }),
                    Line::from(if platform == "weixin" {
                        format!("  Status: {}", if *enabled { "enabled" } else { "disabled" })
                    } else {
                        format!(
                            "  Status: {} · {}",
                            if *enabled { "enabled" } else { "disabled" },
                            if *configured {
                                "configured"
                            } else {
                                "incomplete"
                            }
                        )
                    }),
                ];
                for (index, field) in fields.iter().enumerate() {
                    let prefix = if *selected == index { ">" } else { " " };
                    let meta = match (field.required, field.is_secret, field.was_set) {
                        (true, true, true) => "required, secret, set",
                        (true, true, false) => "required, secret",
                        (true, false, _) => "required",
                        (false, true, true) => "optional, secret, set",
                        (false, true, false) => "optional, secret",
                        (false, false, _) => "optional",
                    };
                    let value = if field.value.is_empty() {
                        "________".to_string()
                    } else {
                        field.value.clone()
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!("{prefix} {:<12}: ", field.key),
                            if *selected == index {
                                selected_style
                            } else {
                                normal_style
                            },
                        ),
                        Span::styled(
                            format!("{value} ({meta})"),
                            Style::default().fg(Color::Rgb(210, 215, 225)),
                        ),
                    ]));
                }
                if platform != "weixin" {
                    lines.push(Line::from(
                        "  Config values and connection state are separate: save stores fields, connection controls whether this platform is running.",
                    ));
                }
                let toggle_index = fields.len();
                let save_index = fields.len() + 1;
                let show_weixin_login = platform == "weixin" && !*enabled && !*configured;
                let back_index = if platform == "weixin" { fields.len() + 1 } else { fields.len() + 2 };
                lines.push(Line::from("  "));
                lines.push(Line::from(Span::styled(
                    if *selected == toggle_index {
                        format!(
                            "  > [ {} ]",
                            if platform == "weixin" {
                                if *enabled {
                                    "Stop Connection"
                                } else if *configured {
                                    "Start Connection"
                                } else if weixin_login.is_some() {
                                    "Check QR Login Status"
                                } else {
                                    "Start Connection (QR Login)"
                                }
                            } else if *enabled {
                                "Connection: Enabled - press Enter to toggle and apply"
                            } else {
                                "Connection: Disabled - press Enter to toggle and apply"
                            }
                        )
                    } else {
                        format!(
                            "    [ {} ]",
                            if platform == "weixin" {
                                if *enabled {
                                    "Stop Connection"
                                } else if *configured {
                                    "Start Connection"
                                } else if weixin_login.is_some() {
                                    "Check QR Login Status"
                                } else {
                                    "Start Connection (QR Login)"
                                }
                            } else if *enabled {
                                "Connection: Enabled - press Enter to toggle and apply"
                            } else {
                                "Connection: Disabled - press Enter to toggle and apply"
                            }
                        )
                    },
                    if *selected == toggle_index {
                        selected_style
                    } else {
                        normal_style
                    },
                )));
                if show_weixin_login {
                    if let Some(session) = weixin_login {
                        lines.push(Line::from(format!(
                            "  QR session: {}",
                            session.session_id
                        )));
                        lines.push(Line::from("  Scan this URL with WeChat:"));
                        lines.push(Line::from(format!("  {}", session.qr_url)));
                    }
                }
                if platform != "weixin" {
                    lines.push(Line::from(Span::styled(
                        if *selected == save_index {
                            "  > [ Save Platform Settings ]"
                        } else {
                            "    [ Save Platform Settings ]"
                        },
                        if *selected == save_index {
                            selected_style
                        } else {
                            normal_style
                        },
                    )));
                }
                lines.push(Line::from(Span::styled(
                    if *selected == back_index {
                        "  > [ Back to list ]"
                    } else {
                        "    [ Back to list ]"
                    },
                    if *selected == back_index {
                        selected_style
                    } else {
                        normal_style
                    },
                )));
                lines.push(Line::from(if platform == "weixin" {
                    "  Start Connection opens QR binding. Stop Connection closes the running adapter."
                } else {
                    "  Enter on Connection applies the state immediately; Save writes field changes."
                }));
                lines
            }
        }
    }

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::GatewayPanel;
    use crate::input::intent::{ComposerIntent, GatewayConfigUpdate};
    use crate::ui::widgets::bottom_pane_view::BottomPaneView;
    use crate::ui::widgets::bottom_pane_view::BottomPaneViewAction;
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
            BottomPaneViewAction::Composer(ComposerIntent::GatewaySelect(platform))
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
            BottomPaneViewAction::Composer(ComposerIntent::GatewaySave {
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
            BottomPaneViewAction::Composer(ComposerIntent::GatewaySave {
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
            BottomPaneViewAction::Composer(ComposerIntent::GatewaySave {
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
}
