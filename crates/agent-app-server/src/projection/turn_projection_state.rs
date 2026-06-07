use agent_core::conversation::InputItem;
use agent_core::{TurnItemDeltaKind, TurnItemKind, TurnState};

#[derive(Clone, Debug)]
pub(super) struct ActiveLifecycle {
    pub(super) turn_id: String,
    pub(super) item_id: String,
    pub(super) call_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ProjectedItemStatus {
    Started,
    Completed,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(super) struct ProjectedItemState {
    pub(super) turn_id: String,
    pub(super) item_id: String,
    pub(super) call_id: Option<String>,
    pub(super) kind: TurnItemKind,
    pub(super) title: Option<String>,
    pub(super) status: ProjectedItemStatus,
    pub(super) last_delta_kind: Option<TurnItemDeltaKind>,
    pub(super) user_content: Vec<InputItem>,
    pub(super) text_buffer: String,
    pub(super) reasoning_buffer: String,
    pub(super) tool_output_buffer: String,
    pub(super) reasoning_summary_part_opened: bool,
    pub(super) order_hint: u64,
}

impl ProjectedItemState {
    pub(super) fn new(
        turn_id: String,
        item_id: String,
        call_id: Option<String>,
        kind: TurnItemKind,
        title: Option<String>,
        order_hint: u64,
    ) -> Self {
        Self {
            turn_id,
            item_id,
            call_id,
            kind,
            title,
            status: ProjectedItemStatus::Started,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint,
        }
    }

    pub(super) fn apply_delta(&mut self, kind: TurnItemDeltaKind, delta: &str) {
        self.last_delta_kind = Some(kind.clone());
        match kind {
            TurnItemDeltaKind::Text => self.text_buffer.push_str(delta),
            TurnItemDeltaKind::ReasoningSummary | TurnItemDeltaKind::ReasoningText => {
                self.reasoning_buffer.push_str(delta)
            }
            TurnItemDeltaKind::CommandExecutionOutput
            | TurnItemDeltaKind::ToolOutput
            | TurnItemDeltaKind::FileChangeOutput => self.tool_output_buffer.push_str(delta),
            TurnItemDeltaKind::JsonPatch => {}
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TurnProjectionState {
    pub(super) state: TurnState,
    pub(super) items_in_order: Vec<String>,
    pub(super) rollout_start_index: usize,
    pub(super) rollout_end_index: usize,
}

impl TurnProjectionState {
    pub(super) fn new(rollout_index: usize) -> Self {
        Self {
            state: TurnState::Running,
            items_in_order: Vec::new(),
            rollout_start_index: rollout_index,
            rollout_end_index: rollout_index,
        }
    }

    pub(super) fn push_item(&mut self, item_id: &str) {
        if !self
            .items_in_order
            .iter()
            .any(|existing| existing == item_id)
        {
            self.items_in_order.push(item_id.to_string());
        }
    }

    pub(super) fn retain_item_ids(&mut self, keep: impl Fn(&String) -> bool) {
        self.items_in_order.retain(keep);
    }

    pub(super) fn touch(&mut self, rollout_index: usize) {
        self.rollout_end_index = rollout_index;
    }
}
