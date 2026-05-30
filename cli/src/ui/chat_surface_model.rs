use crate::app::TuiApp;
use crate::app::runtime::display::should_show_welcome;
use crate::ui::widgets::history_cell::HistoryCell;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;

pub(crate) enum ChatSurfaceBody {
    Welcome,
    ActiveCell(ActiveCellSurface),
}

pub(crate) struct ActiveCellSurface {
    pub(crate) lines: Vec<Line<'static>>,
}

pub(crate) struct ChatSurfaceModel {
    pub(crate) body: ChatSurfaceBody,
    pub(crate) body_height: u16,
}

pub(crate) fn build_chat_surface_model(
    app: &mut TuiApp,
    render_width: usize,
    max_body_height: usize,
) -> ChatSurfaceModel {
    if should_show_welcome(app) {
        ChatSurfaceModel {
            body: ChatSurfaceBody::Welcome,
            body_height: max_body_height.min(u16::MAX as usize) as u16,
        }
    } else {
        let lines = visible_transcript_lines(app, render_width, max_body_height);
        let body_height = transcript_container_height(lines.len(), max_body_height);
        ChatSurfaceModel {
            body: ChatSurfaceBody::ActiveCell(ActiveCellSurface { lines }),
            body_height,
        }
    }
}

fn transcript_container_height(line_count: usize, max_body_height: usize) -> u16 {
    if line_count == 0 {
        return 0;
    }
    let visible_lines = line_count.min(max_body_height);
    visible_lines.min(u16::MAX as usize) as u16
}

fn visible_transcript_lines(
    app: &mut TuiApp,
    render_width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let lines = wrap_transcript_lines(
        transcript_lines(app.transcript_owner.active_cell(), render_width),
        render_width,
    );
    if lines.len() > max_lines {
        lines[lines.len().saturating_sub(max_lines)..].to_vec()
    } else {
        lines
    }
}

fn transcript_lines(active_cell: Option<&HistoryCell>, render_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(cell) = active_cell {
        push_cell_lines(&mut lines, cell, render_width);
    }
    lines
}

fn push_cell_lines(lines: &mut Vec<Line<'static>>, cell: &HistoryCell, render_width: usize) {
    if !cell.body().trim().is_empty() {
        lines.extend(cell.to_transcript_lines(render_width));
    }
}

fn wrap_transcript_lines(lines: Vec<Line<'static>>, wrap_width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .flat_map(|line| wrap_transcript_line(line, wrap_width.max(1)))
        .collect()
}

fn wrap_transcript_line(line: Line<'static>, wrap_width: usize) -> Vec<Line<'static>> {
    if line.width() <= wrap_width {
        return vec![line];
    }

    let line_style = line.style;
    let mut rows = Vec::new();
    let mut row_spans = Vec::new();
    let mut row_width = 0usize;

    for span in line.spans {
        let span_style = span.style;
        let mut chunk = String::new();
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if row_width > 0 && row_width + ch_width > wrap_width {
                if !chunk.is_empty() {
                    row_spans.push(Span::styled(std::mem::take(&mut chunk), span_style));
                }
                rows.push(line_from_spans(std::mem::take(&mut row_spans), line_style));
                row_width = 0;
            }
            chunk.push(ch);
            row_width = row_width.saturating_add(ch_width);
        }
        if !chunk.is_empty() {
            row_spans.push(Span::styled(chunk, span_style));
        }
    }

    if !row_spans.is_empty() {
        rows.push(line_from_spans(row_spans, line_style));
    }
    if rows.is_empty() {
        rows.push(line_from_spans(Vec::new(), line_style));
    }
    rows
}

fn line_from_spans(spans: Vec<Span<'static>>, style: ratatui::style::Style) -> Line<'static> {
    let mut line = Line::from(spans);
    line.style = style;
    line
}
