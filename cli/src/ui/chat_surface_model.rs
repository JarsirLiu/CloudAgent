use crate::app::TuiApp;
use crate::app::runtime::display::should_show_welcome;

pub(crate) enum ChatSurfaceBody {
    Welcome,
    Transcript(TranscriptSurface),
}

pub(crate) struct TranscriptSurface {
    pub(crate) lines: Vec<ratatui::text::Line<'static>>,
    pub(crate) rendered_rows: usize,
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
        let transcript = transcript_for_width(app, render_width);
        let body_height = transcript.rendered_rows.min(u16::MAX as usize) as u16;
        ChatSurfaceModel {
            body: ChatSurfaceBody::Transcript(transcript),
            body_height,
        }
    }
}

fn transcript_for_width(app: &mut TuiApp, render_width: usize) -> TranscriptSurface {
    let key = app.transcript_owner.render_cache_key(render_width);
    if !app.transcript_render_cache.is_fresh(key) {
        let cells = app.transcript_owner.transcript_cells_for_render();
        let lines = build_transcript_lines(cells, render_width);
        let rendered_rows = if lines.is_empty() {
            0
        } else {
            crate::ui::transcript_render_cache::build_rendered_rows(&lines, render_width)
        };
        app.transcript_render_cache.store(key, lines, rendered_rows);
    }
    TranscriptSurface {
        lines: app.transcript_render_cache.lines().to_vec(),
        rendered_rows: app.transcript_render_cache.rendered_rows(),
    }
}

fn build_transcript_lines(
    cells: Vec<crate::ui::widgets::history_cell::HistoryCell>,
    render_width: usize,
) -> Vec<ratatui::text::Line<'static>> {
    let mut lines = Vec::new();
    let mut has_emitted = false;
    let mut last_kind: Option<crate::ui::widgets::history_cell::HistoryKind> = None;
    for cell in cells {
        if cell.body().trim().is_empty() {
            continue;
        }
        if has_emitted && !cell.is_stream_continuation() {
            lines.push(ratatui::text::Line::from(""));
            if should_add_tool_gap(last_kind, cell.kind()) {
                lines.push(ratatui::text::Line::from(""));
            }
        }
        push_cell_lines(&mut lines, &cell, render_width);
        has_emitted = true;
        last_kind = Some(cell.kind());
    }
    trim_trailing_blank_lines(&mut lines);
    lines
}

fn push_cell_lines(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    cell: &crate::ui::widgets::history_cell::HistoryCell,
    render_width: usize,
) {
    if !cell.body().trim().is_empty() {
        lines.extend(cell.to_live_transcript_lines(render_width));
    }
}

fn trim_trailing_blank_lines(lines: &mut Vec<ratatui::text::Line<'static>>) {
    while lines
        .last()
        .is_some_and(|line| line.to_string().trim().is_empty())
    {
        lines.pop();
    }
}

fn should_add_tool_gap(
    previous_kind: Option<crate::ui::widgets::history_cell::HistoryKind>,
    current_kind: crate::ui::widgets::history_cell::HistoryKind,
) -> bool {
    matches!(
        (previous_kind, current_kind),
        (
            Some(
                crate::ui::widgets::history_cell::HistoryKind::Message
                    | crate::ui::widgets::history_cell::HistoryKind::Reasoning
                    | crate::ui::widgets::history_cell::HistoryKind::Exploration
            ),
            crate::ui::widgets::history_cell::HistoryKind::Command
        ) | (
            Some(
                crate::ui::widgets::history_cell::HistoryKind::Message
                    | crate::ui::widgets::history_cell::HistoryKind::Reasoning
                    | crate::ui::widgets::history_cell::HistoryKind::Exploration
            ),
            crate::ui::widgets::history_cell::HistoryKind::Tool
        )
    )
}
