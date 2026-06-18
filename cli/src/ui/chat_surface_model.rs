use crate::app::TuiApp;
use crate::app::runtime::display::should_show_welcome;
use crate::ui::transcript_line_builder::{TranscriptLineOptions, build_transcript_lines};

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
        let snapshot = app.transcript_owner.viewport_snapshot();
        let _revision = snapshot.revision;
        let lines =
            build_transcript_lines(&snapshot.cells, TranscriptLineOptions::live(render_width))
                .lines;
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
