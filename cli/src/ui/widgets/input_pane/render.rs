use super::layout::{
    COMPOSER_TOP_SPACER_HEIGHT, compute_desired_height, compute_input_layout,
};
use super::{InputPane, InputPaneRenderResult};
use crate::terminal::Frame;
use crate::ui::theme::{input_border_style, input_completion_border_style, input_title_style};
use crate::ui::widgets::footer::{hint_line, status_line};
use agent_protocol::FrontendMode;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

pub(super) struct InputPaneSnapshot {
    pub(super) layout: super::layout::InputPaneLayout,
    pub(super) input_lines: Vec<Line<'static>>,
    pub(super) completion_lines: Vec<Line<'static>>,
    pub(super) cursor_position: Option<(u16, u16)>,
    pub(super) height: u16,
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
        if self
            .navigator
            .active_view()
            .is_some_and(|view| view.requires_action())
        {
            let (widget, lines_before_composer, _) = self.render_request_view(
                mode,
                status_indicator,
                status_text,
                runtime_hint,
                status_meta,
                area.width,
            );
            frame.render_widget(widget, area);
            return InputPaneRenderResult {
                cursor_position: self.cursor_position(area, lines_before_composer, mode),
            };
        }

        let inner_width = area.width.saturating_sub(2) as usize;
        let snapshot = self.build_snapshot(
            area,
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            status_meta,
            hint_meta,
            inner_width,
        );
        frame.render_widget(
            input_block(snapshot.input_lines, input_border_style(mode)),
            snapshot.layout.input_area,
        );

        if let Some(completion_area) = snapshot.layout.completion_area {
            let panel = Paragraph::new(Text::from(snapshot.completion_lines)).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(input_completion_border_style()),
            );
            frame.render_widget(panel, completion_area);
        }

        InputPaneRenderResult {
            cursor_position: snapshot.cursor_position,
        }
    }

    fn render_request_view(
        &self,
        mode: FrontendMode,
        status_indicator: Option<&str>,
        status_text: &str,
        runtime_hint: Option<&str>,
        _status_meta: &str,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let inner_width = area_width.saturating_sub(2) as usize;
        lines.push(status_line(
            mode,
            status_indicator,
            status_text,
            runtime_hint,
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
                    .border_style(input_border_style(mode))
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
        if self
            .navigator
            .active_view()
            .is_some_and(|view| view.requires_action())
        {
            let (widget, lines_before, _) =
                self.render_request_view(mode, None, status_text, None, status_meta, area_width);
            let text = format!("{widget:?}");
            return (vec![Line::raw(text)], lines_before);
        }

        let inner_width = area_width.saturating_sub(2) as usize;
        let snapshot = self.build_snapshot(
            Rect::new(0, 0, area_width, self.desired_height(mode, area_width)),
            mode,
            None,
            status_text,
            None,
            status_meta,
            "",
            inner_width,
        );
        let cursor_y = snapshot.cursor_position.map(|(_, y)| y).unwrap_or_default();
        (snapshot.input_lines, cursor_y)
    }

    pub fn desired_height(&self, mode: FrontendMode, area_width: u16) -> u16 {
        let inner_width = area_width.saturating_sub(2) as usize;
        if let Some(view) = self.navigator.active_view()
            && view.requires_action()
        {
            return (4 + view.desired_height(area_width.saturating_sub(2))).max(7);
        }

        let snapshot = self.build_snapshot(
            Rect::new(0, 0, area_width, u16::MAX),
            mode,
            None,
            "",
            None,
            "",
            "",
            inner_width,
        );
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

    pub(super) fn build_snapshot(
        &self,
        area: Rect,
        mode: FrontendMode,
        status_indicator: Option<&str>,
        status_text: &str,
        runtime_hint: Option<&str>,
        status_meta: &str,
        hint_meta: &str,
        inner_width: usize,
    ) -> InputPaneSnapshot {
        let composer = self.composer.render(mode, inner_width);
        let completion_lines = if let Some(view) = self.navigator.active_view() {
            view.render_lines(area.width.saturating_sub(2))
        } else {
            composer.completion_lines.clone()
        };
        let layout = compute_input_layout(area, composer.height, completion_lines.len());
        let mut input_lines = vec![status_line(
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            status_meta,
            inner_width,
        )];
        if COMPOSER_TOP_SPACER_HEIGHT > 0 {
            input_lines.push(Line::raw(""));
        }
        input_lines.extend(composer.lines);
        if layout.completion_area.is_none() {
            input_lines.push(hint_line(mode, inner_width, hint_meta));
        }
        let cursor_position = Some(self.composer.cursor_position(layout.composer_area, mode));
        let height = compute_desired_height(composer.height, completion_lines.len());

        InputPaneSnapshot {
            layout,
            input_lines,
            completion_lines,
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
