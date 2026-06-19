use super::state::GatewayPanelMode;
use super::{GatewayPanel, WeixinLoginSessionView};
use crate::text_width::display_width;
use crate::ui::theme::{picker_selected_style, picker_unselected_style, title_style};
use crate::ui::bottom_pane::bottom_pane_view::ViewKind;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub(crate) fn kind(mode: &GatewayPanelMode) -> ViewKind {
    match mode {
        GatewayPanelMode::List { .. } => ViewKind::GatewayList,
        GatewayPanelMode::Edit { .. } => ViewKind::GatewayEdit,
    }
}

pub(crate) fn render_lines(panel: &GatewayPanel, _area_width: u16) -> Vec<Line<'static>> {
    let selected_style = picker_selected_style();
    let normal_style = picker_unselected_style();
    match &panel.mode {
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
                let config = if entry.configured { "configured" } else { "incomplete" };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                Span::styled(format!("{marker} {:<8}", entry.platform), if index == *selected { selected_style } else { normal_style }),
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
        } => render_edit(
            platform,
            *enabled,
            *configured,
            *selected,
            fields,
            weixin_login.as_ref(),
            selected_style,
            normal_style,
        ),
    }
}

fn render_edit(
    platform: &str,
    enabled: bool,
    configured: bool,
    selected: usize,
    fields: &[super::state::EditableField],
    weixin_login: Option<&WeixinLoginSessionView>,
    selected_style: Style,
    normal_style: Style,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![Span::styled(format!("  Gateway Panel · {platform}"), title_style())]),
        Line::from(if platform == "weixin" {
            "  Manage the connection here. Esc returns to the platform list"
        } else {
            "  Type or paste values, Tab/Up/Down switch fields, Esc returns to list"
        }),
        Line::from(if platform == "weixin" {
            format!(
                "  Status: {}",
                if enabled { "enabled" } else { "disabled" }
            )
        } else {
            format!(
                "  Status: {} · {}",
                if enabled { "enabled" } else { "disabled" },
                if configured { "configured" } else { "incomplete" }
            )
        }),
    ];
    for (index, field) in fields.iter().enumerate() {
        let prefix = if selected == index { ">" } else { " " };
        let meta = match (field.required, field.is_secret, field.was_set) {
            (true, true, true) => "required, secret, set",
            (true, true, false) => "required, secret",
            (true, false, _) => "required",
            (false, true, true) => "optional, secret, set",
            (false, true, false) => "optional, secret",
            (false, false, _) => "optional",
        };
        let value = if field.input.is_empty() {
            if selected == index {
                String::new()
            } else {
                "________".to_string()
            }
        } else {
            field.input.value().to_string()
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{prefix} {:<12}: ", field.key),
                if selected == index {
                    selected_style
                } else {
                    normal_style
                },
            ),
            Span::styled(
                format!("{value} ({meta})"),
                picker_unselected_style(),
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
    let show_weixin_login = platform == "weixin" && !enabled && !configured;
    let back_index = if platform == "weixin" {
        fields.len() + 1
    } else {
        fields.len() + 2
    };
    lines.push(Line::from("  "));
    lines.push(Line::from(Span::styled(
        if selected == toggle_index {
            format!(
                "  > [ {} ]",
                if platform == "weixin" {
                    if enabled {
                        "Stop Connection"
                    } else if configured {
                        "Start Connection"
                    } else if weixin_login.is_some() {
                        "Check QR Login Status"
                    } else {
                        "Start Connection (QR Login)"
                    }
                } else if enabled {
                    "Connection: Enabled - press Enter to toggle and apply"
                } else {
                    "Connection: Disabled - press Enter to toggle and apply"
                }
            )
        } else {
            format!(
                "    [ {} ]",
                if platform == "weixin" {
                    if enabled {
                        "Stop Connection"
                    } else if configured {
                        "Start Connection"
                    } else if weixin_login.is_some() {
                        "Check QR Login Status"
                    } else {
                        "Start Connection (QR Login)"
                    }
                } else if enabled {
                    "Connection: Enabled - press Enter to toggle and apply"
                } else {
                    "Connection: Disabled - press Enter to toggle and apply"
                }
            )
        },
        if selected == toggle_index {
            selected_style
        } else {
            normal_style
        },
    )));
    if show_weixin_login && let Some(session) = weixin_login {
        lines.push(Line::from(format!("  QR session: {}", session.session_id)));
        lines.push(Line::from("  Scan this URL with WeChat:"));
        lines.push(Line::from(format!("  {}", session.qr_url)));
    }
    if platform != "weixin" {
        lines.push(Line::from(Span::styled(
            if selected == save_index {
                "  > [ Save Platform Settings ]"
            } else {
                "    [ Save Platform Settings ]"
            },
            if selected == save_index {
                selected_style
            } else {
                normal_style
            },
        )));
    }
    lines.push(Line::from(Span::styled(
        if selected == back_index {
            "  > [ Back to list ]"
        } else {
            "    [ Back to list ]"
        },
        if selected == back_index {
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

pub(crate) fn cursor_position(panel: &GatewayPanel, area: Rect) -> Option<(u16, u16)> {
    let GatewayPanelMode::Edit {
        selected, fields, ..
    } = &panel.mode
    else {
        return None;
    };
    if *selected >= fields.len() {
        return None;
    }
    let field = &fields[*selected];
    let prefix = format!("  > {:<12}: ", field.key);
    Some((
        area.x
            .saturating_add(display_width(&prefix) as u16)
            .saturating_add(fields[*selected].input.cursor_display_column() as u16),
        area.y.saturating_add(3 + *selected as u16),
    ))
}

