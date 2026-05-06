use crate::app::TuiApp;
use crate::app::runtime::display::should_show_welcome;
use crate::ui::widgets::history_cell::HistoryCell;
use ratatui::text::Line;

pub(crate) enum ChatSurfaceBody {
    Welcome,
    ActiveCell(ActiveCellSurface),
}

pub(crate) struct ActiveCellSurface {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) height: u16,
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
        let lines = visible_transcript_lines(
            app,
            render_width,
            max_body_height,
            None,
        );
        let body_height = transcript_container_height(lines.len(), max_body_height);
        ChatSurfaceModel {
            body: ChatSurfaceBody::ActiveCell(ActiveCellSurface {
                lines,
                height: body_height,
            }),
            body_height,
        }
    }
}

fn transcript_container_height(line_count: usize, max_body_height: usize) -> u16 {
    if line_count == 0 {
        return 0;
    }
    let visible_lines = line_count.min(max_body_height);
    let vertical_margin = 2usize;
    (visible_lines + vertical_margin)
        .min(max_body_height.max(1))
        .min(u16::MAX as usize) as u16
}

fn visible_transcript_lines(
    app: &mut TuiApp,
    render_width: usize,
    max_lines: usize,
    status_line: Option<Line<'static>>,
) -> Vec<Line<'static>> {
    let lines = transcript_lines(app.live_cells(), render_width, status_line);
    app.transcript_state.note_total_lines(lines.len());
    app.transcript_state.set_viewport_height(max_lines);
    app.transcript_state.clamp_scroll();
    if lines.len() > max_lines {
        let offset = app.transcript_state.scroll_offset_lines;
        let start = lines.len().saturating_sub(max_lines).saturating_sub(offset);
        lines[start..start + max_lines].to_vec()
    } else {
        app.transcript_state.jump_to_bottom();
        lines
    }
}

fn transcript_lines(
    live_cells: &[HistoryCell],
    render_width: usize,
    status_line: Option<Line<'static>>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, cell) in live_cells.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        push_cell_lines(&mut lines, cell, render_width);
    }
    if let Some(line) = status_line {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.push(line);
    }
    lines
}

fn push_cell_lines(lines: &mut Vec<Line<'static>>, cell: &HistoryCell, render_width: usize) {
    if !cell.body().trim().is_empty() {
        lines.extend(cell.to_lines_with_mode(render_width));
    }
}
