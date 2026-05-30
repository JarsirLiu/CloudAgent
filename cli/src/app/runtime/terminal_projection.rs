use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::terminal::HistoryProjection;
use crate::terminal::HistoryUpdate;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::TerminalGuard;
use crate::terminal::prepare_history_lines;
use crate::terminal::prepare_history_tail_lines;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::Rect;

#[derive(Default)]
pub(crate) struct TerminalProjectionController {
    reflow: TranscriptReflowState,
    reflow_policy: ReflowPolicy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReflowPolicy {
    pub(crate) max_rows: Option<usize>,
}

#[derive(Default)]
struct TranscriptReflowState {
    replay_requested: bool,
    replay_requested_during_stream: bool,
    pending_replay_reason: ReplayReason,
    last_replay_reason: ReplayReason,
    last_observed_width: Option<u16>,
    last_observed_viewport_height: Option<u16>,
    last_replayed_width: Option<u16>,
    last_replayed_viewport_height: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ReplayReason {
    #[default]
    None,
    Explicit,
    Metrics,
}

impl TerminalProjectionController {
    pub(crate) fn set_reflow_policy(&mut self, policy: ReflowPolicy) {
        self.reflow_policy = policy;
    }

    pub(crate) fn request_history_replay(&mut self) {
        self.reflow.request_replay();
    }

    pub(crate) fn reset(&mut self) {
        self.reflow.reset();
    }

    pub(crate) fn on_stream_boundary(&mut self) {
        self.reflow.on_stream_boundary();
    }

    pub(crate) fn build_plan(
        &mut self,
        transcript_owner: &mut TranscriptOwner,
        viewport_height: u16,
        terminal_width: u16,
        has_active_stream: bool,
        _history_capacity_rows: u16,
    ) -> HistoryProjection {
        let should_replay =
            self.reflow
                .begin_frame(terminal_width, viewport_height, has_active_stream);

        let history_update = if should_replay {
            let committed = transcript_owner.committed_history_cells();
            transcript_owner.mark_committed_history_replayed();
            HistoryUpdate::ReplayAll(committed)
        } else {
            HistoryUpdate::AppendTail(transcript_owner.drain_pending_history_cells())
        };

        HistoryProjection {
            viewport_height,
            history_render_width: ChatSurface::render_width_for_area(Rect::new(
                0,
                0,
                terminal_width,
                viewport_height.max(1),
            )),
            history_update,
        }
    }

    pub(crate) fn draw_frame(
        &mut self,
        app: &mut TuiApp,
        terminal: &mut TerminalGuard,
    ) -> Result<()> {
        let size = terminal.terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        let viewport_height = ChatSurface::desired_viewport_height(app, area);
        let history_capacity_rows = area.height.saturating_sub(viewport_height);
        let has_active_stream = app.transcript_owner.active_turn_id().is_some();
        let plan = self.build_plan(
            &mut app.transcript_owner,
            viewport_height,
            area.width,
            has_active_stream,
            history_capacity_rows,
        );
        let prepared = self.prepare_projection(plan, terminal.terminal.visible_history_rows() > 0);
        terminal.draw_projection(prepared, |frame| app.render(frame))?;
        Ok(())
    }

    fn prepare_projection(
        &self,
        projection: HistoryProjection,
        has_existing_history: bool,
    ) -> PreparedHistoryProjection {
        let HistoryProjection {
            viewport_height,
            history_render_width,
            history_update,
        } = projection;

        let history_update = match history_update {
            HistoryUpdate::ReplayAll(cells) => {
                let lines = if let Some(max_rows) = self.reflow_policy.max_rows {
                    prepare_history_tail_lines(cells, history_render_width, max_rows)
                } else {
                    prepare_history_lines(cells, history_render_width, false)
                };
                PreparedHistoryUpdate::ReplayAll(lines)
            }
            HistoryUpdate::AppendTail(cells) => PreparedHistoryUpdate::AppendTail(
                prepare_history_lines(cells, history_render_width, has_existing_history),
            ),
        };

        PreparedHistoryProjection {
            viewport_height,
            history_update,
        }
    }
}

pub(crate) fn draw_with_terminal_projection(
    app: &mut TuiApp,
    terminal: &mut TerminalGuard,
) -> Result<()> {
    let mut projection = std::mem::take(&mut app.terminal_projection);
    let result = projection.draw_frame(app, terminal);
    app.terminal_projection = projection;
    result
}

impl TranscriptReflowState {
    fn request_replay(&mut self) {
        self.replay_requested = true;
        self.pending_replay_reason = ReplayReason::Explicit;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn on_stream_boundary(&mut self) {
        if self.replay_requested_during_stream {
            self.replay_requested = true;
            self.replay_requested_during_stream = false;
            self.pending_replay_reason = ReplayReason::Metrics;
        }
    }

    fn begin_frame(&mut self, width: u16, viewport_height: u16, has_active_stream: bool) -> bool {
        let replay_metrics_changed = self.last_replayed_width != Some(width)
            || self.last_replayed_viewport_height != Some(viewport_height);
        let observed_metrics_changed = self.last_observed_width != Some(width)
            || self.last_observed_viewport_height != Some(viewport_height);

        self.last_observed_width = Some(width);
        self.last_observed_viewport_height = Some(viewport_height);

        if has_active_stream && observed_metrics_changed {
            self.replay_requested_during_stream = true;
        }

        if self.replay_requested || replay_metrics_changed {
            self.last_replay_reason = if self.replay_requested {
                self.pending_replay_reason
            } else {
                ReplayReason::Metrics
            };
            self.replay_requested = false;
            self.pending_replay_reason = ReplayReason::None;
            self.last_replayed_width = Some(width);
            self.last_replayed_viewport_height = Some(viewport_height);
            true
        } else {
            self.last_replay_reason = ReplayReason::None;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReflowPolicy, TerminalProjectionController};
    use crate::app::core::transcript_owner::TranscriptOwner;
    use crate::terminal::{HistoryProjection, HistoryUpdate, PreparedHistoryUpdate};
    use crate::ui::widgets::history_cell::HistoryCell;

    #[test]
    fn explicit_replay_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();
        controller.request_history_replay();

        let plan = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);

        match plan.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn metrics_change_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }

        let second = controller.build_plan(&mut transcript_owner, 6, 80, false, 0);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn metrics_change_replays_all_history_even_with_visible_capacity() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 6, 80, false, 4);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay after metrics changed"),
        }
    }

    #[test]
    fn resize_during_stream_replays_now_and_repairs_at_stream_boundary() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80, true, 0);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("streaming metrics change should replay now"),
        }

        controller.on_stream_boundary();
        let second = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected final repair after stream boundary"),
        }
    }

    #[test]
    fn reflow_policy_caps_prepared_replay_lines_without_changing_plan() {
        let mut controller = TerminalProjectionController::default();
        controller.set_reflow_policy(ReflowPolicy { max_rows: Some(3) });

        let prepared = controller.prepare_projection(
            HistoryProjection {
                viewport_height: 5,
                history_render_width: 80,
                history_update: HistoryUpdate::ReplayAll(vec![
                    HistoryCell::user("oldest message"),
                    HistoryCell::user("middle message"),
                    HistoryCell::user("latest message"),
                ]),
            },
            false,
        );

        match prepared.history_update {
            PreparedHistoryUpdate::ReplayAll(lines) => {
                let rendered = lines
                    .iter()
                    .map(|line| {
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                assert!(lines.len() <= 3);
                assert!(rendered.contains("latest message"));
                assert!(!rendered.contains("oldest message"));
            }
            PreparedHistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }
}
