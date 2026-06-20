use super::layout::{COMPOSER_TOP_SPACER_HEIGHT, compute_desired_height, compute_input_layout};
use super::{InputPane, InputPaneRenderResult};
use crate::terminal::Frame;
use crate::ui::bottom_pane::bottom_pane_view::ViewKind;
use crate::ui::bottom_pane::support::footer::{hint_line, status_line};
use crate::ui::theme::{input_border_style, input_completion_border_style, input_title_style};
use agent_protocol::FrontendMode;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

pub(crate) struct InputPaneSnapshot {
    pub(super) layout: super::layout::InputPaneLayout,
    pub(super) input_lines: Vec<Line<'static>>,
    pub(super) popup_lines: Vec<Line<'static>>,
    pub(super) cursor_position: Option<(u16, u16)>,
    pub(super) height: u16,
}

struct RenderRequest<'a> {
    mode: FrontendMode,
    status_indicator: Option<&'a str>,
    status_text: &'a str,
    runtime_hint: Option<&'a str>,
    status_meta: &'a str,
    hint_meta: &'a str,
}

impl InputPane {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        mode: FrontendMode,
        status_indicator: Option<&str>,
        status_text: &str,
        runtime_hint: Option<&str>,
        status_meta: &str,
        hint_meta: &str,
    ) -> InputPaneRenderResult {
        let request = RenderRequest {
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            status_meta,
            hint_meta,
        };
        if self
            .navigator
            .active_view()
            .is_some_and(|view| view_uses_full_input_pane(view.kind()))
        {
            let (widget, lines_before_composer, _) = self.render_request_view(&request, area.width);
            frame.render_widget(widget, area);
            return InputPaneRenderResult {
                cursor_position: self.cursor_position(area, lines_before_composer, request.mode),
            };
        }

        let inner_width = area.width.saturating_sub(2) as usize;
        let snapshot = self.build_snapshot(area, &request, inner_width);
        frame.render_widget(
            input_block(snapshot.input_lines, input_border_style(request.mode)),
            snapshot.layout.input_area,
        );

        if let Some(popup_area) = snapshot.layout.popup_area {
            let panel = Paragraph::new(Text::from(snapshot.popup_lines)).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(input_completion_border_style()),
            );
            frame.render_widget(panel, popup_area);
        }

        InputPaneRenderResult {
            cursor_position: snapshot.cursor_position,
        }
    }

    fn render_request_view(
        &self,
        request: &RenderRequest<'_>,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let inner_width = area_width.saturating_sub(2) as usize;
        lines.push(status_line(
            request.mode,
            request.status_indicator,
            request.status_text,
            request.runtime_hint,
            "",
            inner_width,
        ));

        let mut lines_before_composer = 1u16;

        if let Some(view) = self.navigator.active_view() {
            lines.push(Line::raw(""));
            lines_before_composer += 1;
            let view_lines = view.render_lines(area_width.saturating_sub(2));
            lines_before_composer += view_lines.len() as u16;
            lines.extend(view_lines);
        }

        let total_lines = lines.len() as u16;
        (
            Paragraph::new(Text::from(lines)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(input_border_style(request.mode))
                    .title_style(input_title_style())
                    .title(" action "),
            ),
            lines_before_composer,
            total_lines,
        )
    }

    #[cfg(test)]
    pub(crate) fn render_lines_for_test(
        &self,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        area_width: u16,
    ) -> (Vec<Line<'static>>, u16) {
        let request = RenderRequest {
            mode,
            status_indicator: None,
            status_text,
            runtime_hint: None,
            status_meta,
            hint_meta: "",
        };
        if self
            .navigator
            .active_view()
            .is_some_and(|view| view_uses_full_input_pane(view.kind()))
        {
            let (widget, lines_before, _) = self.render_request_view(&request, area_width);
            let text = format!("{widget:?}");
            return (vec![Line::raw(text)], lines_before);
        }

        let inner_width = area_width.saturating_sub(2) as usize;
        let snapshot = self.build_snapshot(
            Rect::new(0, 0, area_width, self.desired_height(mode, area_width)),
            &request,
            inner_width,
        );
        let cursor_y = snapshot.cursor_position.map(|(_, y)| y).unwrap_or_default();
        (snapshot.input_lines, cursor_y)
    }

    #[cfg(test)]
    pub(crate) fn snapshot_for_test(
        &self,
        area: Rect,
        mode: FrontendMode,
        area_width: u16,
    ) -> InputPaneSnapshot {
        let request = RenderRequest {
            mode,
            status_indicator: None,
            status_text: "",
            runtime_hint: None,
            status_meta: "",
            hint_meta: "",
        };
        self.build_snapshot(area, &request, area_width.saturating_sub(2) as usize)
    }

    pub fn desired_height(&self, mode: FrontendMode, area_width: u16) -> u16 {
        let inner_width = area_width.saturating_sub(2) as usize;
        if let Some(view) = self.navigator.active_view()
            && view_uses_full_input_pane(view.kind())
        {
            return (4 + view.desired_height(area_width.saturating_sub(2))).max(7);
        }

        let request = RenderRequest {
            mode,
            status_indicator: None,
            status_text: "",
            runtime_hint: None,
            status_meta: "",
            hint_meta: "",
        };
        let snapshot =
            self.build_snapshot(Rect::new(0, 0, area_width, u16::MAX), &request, inner_width);
        snapshot.height
    }

    pub fn cursor_position(
        &self,
        area: Rect,
        lines_before: u16,
        mode: FrontendMode,
    ) -> Option<(u16, u16)> {
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1).saturating_add(lines_before),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(lines_before + 2),
        };
        if let Some(view) = self.navigator.active_view() {
            let view_area = Rect {
                x: area.x.saturating_add(1),
                y: area.y.saturating_add(3),
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(4),
            };
            return view.cursor_position(view_area);
        }
        Some(self.composer.cursor_position(inner, mode))
    }

    fn build_snapshot(
        &self,
        area: Rect,
        request: &RenderRequest<'_>,
        inner_width: usize,
    ) -> InputPaneSnapshot {
        let composer = self.composer.render(request.mode, inner_width);
        let popup_lines = match self.navigator.active_view() {
            Some(view) if !view_uses_full_input_pane(view.kind()) => {
                view.render_lines(area.width.saturating_sub(2))
            }
            Some(_) => Vec::new(),
            None => composer.completion_lines.clone(),
        };
        let popup_height = if popup_lines.is_empty() {
            None
        } else {
            Some((popup_lines.len() as u16).saturating_add(1))
        };
        let layout = compute_input_layout(area, composer.height, popup_height);
        let mut input_lines = vec![status_line(
            request.mode,
            request.status_indicator,
            request.status_text,
            request.runtime_hint,
            request.status_meta,
            inner_width,
        )];
        if COMPOSER_TOP_SPACER_HEIGHT > 0 {
            input_lines.push(Line::raw(""));
        }
        input_lines.extend(composer.lines);
        if layout.popup_area.is_none() {
            input_lines.push(hint_line(request.mode, inner_width, request.hint_meta));
        }
        let cursor_position = match (self.navigator.active_view(), layout.popup_area) {
            (Some(view), Some(popup_area)) if !view_uses_full_input_pane(view.kind()) => {
                view.cursor_position(popup_inner_area(popup_area))
            }
            _ => Some(self.composer.cursor_position(layout.composer_area, request.mode)),
        };
        let height = compute_desired_height(composer.height, popup_height);

        InputPaneSnapshot {
            layout,
            input_lines,
            popup_lines,
            cursor_position,
            height,
        }
    }
}

pub(super) fn input_block(lines: Vec<Line<'static>>, border_style: Style) -> Paragraph<'static> {
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style)
            .title_style(input_title_style())
            .title(" prompt "),
    )
}

fn view_uses_full_input_pane(kind: ViewKind) -> bool {
    matches!(kind, ViewKind::ServerRequest)
}

fn popup_inner_area(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    }
}
