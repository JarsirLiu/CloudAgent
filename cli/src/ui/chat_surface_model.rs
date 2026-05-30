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
        let lines = transcript_lines_for_width(app, render_width);
        let body_height = transcript_container_height(&lines, render_width);
        ChatSurfaceModel {
            body: ChatSurfaceBody::ActiveCell(ActiveCellSurface { lines }),
            body_height,
        }
    }
}

fn transcript_container_height(lines: &[Line<'static>], render_width: usize) -> u16 {
    if lines.is_empty() {
        return 0;
    }
    HistoryCell::rendered_line_count(lines, render_width).min(u16::MAX as usize) as u16
}

fn transcript_lines_for_width(app: &mut TuiApp, render_width: usize) -> Vec<Line<'static>> {
    transcript_lines(app.transcript_owner.active_cell(), render_width)
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
        lines.extend(cell.to_live_transcript_lines(render_width));
    }
}
