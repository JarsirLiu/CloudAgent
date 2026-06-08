use crate::app::core::active_turn::{
    ActiveTurnAction, ActiveTurnEffects, ActiveTurnState, ConsolidateAgentMessage,
};
use crate::ui::widgets::history_cell::{
    HistoryCell, HistoryTone, RenderContext, Transcript, render_history_entry,
};
use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::turn::{TurnId, TurnItemKind};

#[derive(Default)]
pub(crate) struct ActiveCellController {
    live_transcript: Transcript,
    active_turn: ActiveTurnState,
    last_copyable_output: Option<String>,
    revision: u64,
}

pub(crate) struct AppliedActiveTurnEffects {
    pub(crate) replay_cells: Vec<HistoryCell>,
    pub(crate) consolidate_agent_message: Option<ConsolidateAgentMessage>,
}

impl ActiveCellController {
    pub(crate) fn clear(&mut self) {
        self.live_transcript = Transcript::default();
        let _ = self.active_turn.clear();
        self.last_copyable_output = None;
        self.bump_revision();
    }

    pub(crate) fn replace_notice_cell(&mut self, cell: HistoryCell) {
        self.live_transcript.replace_cells(vec![cell]);
        self.bump_revision();
    }

    pub(crate) fn replace_live_cells(&mut self, cells: Vec<HistoryCell>, expand_details: bool) {
        let mut cells = cells;
        for cell in &mut cells {
            if matches!(
                cell.tone,
                HistoryTone::Reasoning
                    | HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.set_expanded(expand_details);
            }
        }
        self.live_transcript.replace_cells(cells);
        self.live_transcript.set_tool_cells_expanded(expand_details);
        self.bump_revision();
    }

    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.live_transcript.cells()
    }

    #[cfg(test)]
    pub(crate) fn active_cell(&self) -> Option<&HistoryCell> {
        self.live_transcript.cells().last()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.live_transcript.is_empty()
    }

    pub(crate) fn set_expand_details(&mut self, expand_details: bool) {
        self.live_transcript.set_tool_cells_expanded(expand_details);
        self.bump_revision();
    }

    pub(crate) fn last_copyable_output(&self) -> Option<&str> {
        self.last_copyable_output.as_deref()
    }

    pub(crate) fn set_last_copyable_output(&mut self, text: Option<String>) {
        self.last_copyable_output = text;
    }

    pub(crate) fn active_turn_id(&self) -> Option<&str> {
        self.active_turn.turn_id()
    }

    pub(crate) fn clear_active_turn(&mut self, expand_details: bool) -> AppliedActiveTurnEffects {
        self.apply_active_turn(ActiveTurnAction::Clear, expand_details)
    }

    pub(crate) fn start_local_user(
        &mut self,
        user_input: Vec<InputItem>,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(
            ActiveTurnAction::StartLocalUser { user_input },
            expand_details,
        )
    }

    pub(crate) fn bind_turn_id(
        &mut self,
        turn_id: TurnId,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(ActiveTurnAction::BindTurnId { turn_id }, expand_details)
    }

    pub(crate) fn start_item(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(
            ActiveTurnAction::StartItem {
                turn_id,
                item_id,
                kind,
                title,
            },
            expand_details,
        )
    }

    pub(crate) fn append_agent_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(
            ActiveTurnAction::AppendAgentDelta {
                turn_id,
                item_id,
                delta,
            },
            expand_details,
        )
    }

    pub(crate) fn append_reasoning_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(
            ActiveTurnAction::AppendReasoningDelta {
                turn_id,
                item_id,
                delta,
            },
            expand_details,
        )
    }

    pub(crate) fn complete_item(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        item: TranscriptItem,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(
            ActiveTurnAction::CompleteItem {
                turn_id,
                item_id,
                item,
            },
            expand_details,
        )
    }

    pub(crate) fn complete_turn(
        &mut self,
        turn_id: TurnId,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.apply_active_turn(ActiveTurnAction::CompleteTurn { turn_id }, expand_details)
    }

    pub(crate) fn restore_running_turn_snapshot(
        &mut self,
        turn: ConversationTurn,
        expand_details: bool,
    ) -> Vec<HistoryCell> {
        let turn_id = turn.id.clone();
        let _ = self.clear_active_turn(expand_details);
        let _ = self.bind_turn_id(turn_id, expand_details);

        let mut replay_cells = Vec::new();
        let mut live_cells = Vec::new();
        let mut last_copyable_output = turn.items.iter().rev().find_map(|item| {
            if let TranscriptItem::AgentMessage { text, .. } = item {
                (!text.trim().is_empty()).then(|| text.clone())
            } else {
                None
            }
        });
        let mut last_live_cell: Option<HistoryCell> = None;
        let mut context = RenderContext;

        for item in turn.items {
            let cell = render_history_entry(&item, &mut context);
            if cell.is_empty() {
                continue;
            }

            if is_live_tail_candidate(&item) {
                if let Some(previous_cell) = last_live_cell.replace(cell) {
                    replay_cells.push(previous_cell);
                }
            } else {
                replay_cells.push(cell);
            }
        }

        if let Some(live_cell) = last_live_cell {
            live_cells.push(live_cell);
        }

        self.replace_live_cells(live_cells, expand_details);
        self.set_last_copyable_output(last_copyable_output.take());
        replay_cells
    }

    fn apply_active_turn(
        &mut self,
        action: ActiveTurnAction,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        let effects = self.active_turn.apply(action);
        self.apply_active_turn_effects(effects, expand_details)
    }

    fn apply_active_turn_effects(
        &mut self,
        effects: ActiveTurnEffects,
        expand_details: bool,
    ) -> AppliedActiveTurnEffects {
        self.replace_live_cells(effects.active_cell.into_iter().collect(), expand_details);
        self.last_copyable_output = effects.last_copyable_output;
        self.bump_revision();
        AppliedActiveTurnEffects {
            replay_cells: effects.replay_cells,
            consolidate_agent_message: effects.consolidate_agent_message,
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

fn is_live_tail_candidate(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::AgentMessage { .. }
            | TranscriptItem::Reasoning { .. }
            | TranscriptItem::CommandExecution { .. }
            | TranscriptItem::ToolResult { .. }
            | TranscriptItem::FileChange { .. }
    )
}
