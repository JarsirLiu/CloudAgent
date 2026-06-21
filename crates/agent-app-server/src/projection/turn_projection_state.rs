use agent_core::conversation::InputItem;
use agent_core::{
    RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress, RuntimeItemSnapshot, RuntimeItemStatus,
    StructuredToolResult, ToolIdentity, TurnItemDeltaKind, TurnItemKind, TurnState,
};

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
    pub(super) summary: Option<String>,
    pub(super) tool_identity: Option<ToolIdentity>,
    pub(super) structured: Option<StructuredToolResult>,
    pub(super) progress: Option<RuntimeItemProgress>,
    pub(super) metrics: Option<RuntimeItemMetrics>,
    pub(super) status: ProjectedItemStatus,
    pub(super) last_delta_kind: Option<TurnItemDeltaKind>,
    pub(super) user_content: Vec<InputItem>,
    pub(super) text_buffer: String,
    pub(super) reasoning_buffer: String,
    pub(super) tool_output_buffer: String,
    pub(super) patch_buffer: String,
    pub(super) reasoning_summary_part_opened: bool,
    pub(super) order_hint: u64,
}

impl ProjectedItemState {
    pub(super) fn from_runtime_item(turn_id: String, item: RuntimeItem, order_hint: u64) -> Self {
        Self {
            turn_id,
            item_id: item.id,
            call_id: item.call_id,
            kind: item.kind,
            title: item.title,
            summary: item.summary,
            tool_identity: item.tool_identity,
            structured: item.structured,
            progress: item.progress,
            metrics: item.metrics,
            status: match item.status {
                RuntimeItemStatus::InProgress => ProjectedItemStatus::Started,
                RuntimeItemStatus::Completed => ProjectedItemStatus::Completed,
            },
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            patch_buffer: String::new(),
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
            TurnItemDeltaKind::CommandExecutionOutput | TurnItemDeltaKind::ToolOutput => {
                self.tool_output_buffer.push_str(delta)
            }
            TurnItemDeltaKind::JsonPatch => self.patch_buffer.push_str(delta),
        }
    }

    pub(super) fn update_progress(&mut self, progress: RuntimeItemProgress) {
        self.progress = Some(progress);
    }

    pub(super) fn update_metrics(&mut self, metrics: RuntimeItemMetrics) {
        self.metrics = Some(metrics);
    }

    pub(super) fn runtime_snapshot(&self) -> RuntimeItemSnapshot {
        RuntimeItemSnapshot {
            item: RuntimeItem {
                id: self.item_id.clone(),
                call_id: self.call_id.clone(),
                kind: self.kind.clone(),
                title: self.title.clone(),
                status: match self.status {
                    ProjectedItemStatus::Started => RuntimeItemStatus::InProgress,
                    ProjectedItemStatus::Completed => RuntimeItemStatus::Completed,
                },
                summary: self.summary.clone(),
                tool_identity: self.tool_identity.clone(),
                structured: self.structured.clone(),
                progress: self.progress.clone(),
                metrics: self.metrics.clone(),
            },
            text_buffer: self.text_buffer.clone(),
            reasoning_buffer: self.reasoning_buffer.clone(),
            tool_output_buffer: self.tool_output_buffer.clone(),
            patch_buffer: self.patch_buffer.clone(),
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
