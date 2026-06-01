use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::projection::{CoreTranscriptEvent, core_transcript_event_from_event_msg};
use agent_core::{
    CommandExecutionStatus, EventMsg, TurnItemDeltaKind, TurnItemKind, TurnState, WriteFileStatus,
};
use agent_protocol::{AppServerNotification, FrontendMode};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct ActiveLifecycle {
    turn_id: String,
    item_id: String,
    call_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProjectedItemStatus {
    Started,
    Completed,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ProjectedItemState {
    turn_id: String,
    item_id: String,
    call_id: Option<String>,
    kind: TurnItemKind,
    title: Option<String>,
    status: ProjectedItemStatus,
    last_delta_kind: Option<TurnItemDeltaKind>,
    user_content: Vec<InputItem>,
    text_buffer: String,
    reasoning_buffer: String,
    tool_output_buffer: String,
    reasoning_summary_part_opened: bool,
    order_hint: u64,
}

impl ProjectedItemState {
    fn new(
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

    fn apply_delta(&mut self, kind: TurnItemDeltaKind, delta: &str) {
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
struct TurnProjectionState {
    state: TurnState,
    items_in_order: Vec<String>,
    rollout_start_index: usize,
    rollout_end_index: usize,
}

impl TurnProjectionState {
    fn new(rollout_index: usize) -> Self {
        Self {
            state: TurnState::Running,
            items_in_order: Vec::new(),
            rollout_start_index: rollout_index,
            rollout_end_index: rollout_index,
        }
    }

    fn push_item(&mut self, item_id: &str) {
        if !self
            .items_in_order
            .iter()
            .any(|existing| existing == item_id)
        {
            self.items_in_order.push(item_id.to_string());
        }
    }

    fn retain_item_ids(&mut self, keep: impl Fn(&String) -> bool) {
        self.items_in_order.retain(keep);
    }

    fn touch(&mut self, rollout_index: usize) {
        self.rollout_end_index = rollout_index;
    }
}

pub(crate) struct ConversationNotificationProjector {
    conversation_id: String,
    deferred_terminal_notifications: Vec<AppServerNotification>,
    active_items_by_item_id: HashMap<String, ActiveLifecycle>,
    active_item_id_by_call_id: HashMap<String, String>,
    turns_by_turn_id: HashMap<String, TurnProjectionState>,
    items_by_item_id: HashMap<String, ProjectedItemState>,
    next_item_order_hint: u64,
    next_rollout_index: usize,
    active_turn_id: Option<String>,
}

impl ConversationNotificationProjector {
    pub(crate) fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            deferred_terminal_notifications: Vec::new(),
            active_items_by_item_id: HashMap::new(),
            active_item_id_by_call_id: HashMap::new(),
            turns_by_turn_id: HashMap::new(),
            items_by_item_id: HashMap::new(),
            next_item_order_hint: 0,
            next_rollout_index: 0,
            active_turn_id: None,
        }
    }

    pub(crate) fn project_turn_event(&mut self, event: &EventMsg) -> Vec<AppServerNotification> {
        let rollout_index = self.bump_rollout_index();
        if let Some(core_event) = core_transcript_event_from_event_msg(event) {
            return self.project_core_transcript_event(core_event, rollout_index);
        }

        match event {
            EventMsg::TurnStarted {
                turn_id,
                user_input,
                ..
            } => {
                self.observe_turn_started(turn_id.clone(), user_input.clone(), rollout_index);
                vec![AppServerNotification::TurnStarted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                }]
            }
            EventMsg::ItemStarted {
                turn_id,
                item_id,
                call_id,
                kind,
                title,
            } => {
                let lifecycle = ActiveLifecycle {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                };
                self.register_active_item(lifecycle);
                self.observe_item_started(
                    turn_id.clone(),
                    item_id.clone(),
                    call_id.clone(),
                    kind.clone(),
                    title.clone(),
                    rollout_index,
                );
                let started_item = self
                    .items_by_item_id
                    .get(item_id)
                    .and_then(projected_item_to_transcript_item)
                    .unwrap_or_else(|| fallback_started_item(item_id, kind, title.as_deref()));
                vec![AppServerNotification::ItemStarted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    call_id: call_id.clone(),
                    item: started_item,
                }]
            }
            EventMsg::ItemDelta {
                turn_id,
                item_id,
                call_id,
                kind,
                delta,
                ..
            } => match kind {
                TurnItemDeltaKind::Text
                | TurnItemDeltaKind::ReasoningSummary
                | TurnItemDeltaKind::ReasoningText => {
                    self.observe_item_delta(turn_id, item_id, kind.clone(), delta, rollout_index);
                    Vec::new()
                }
                TurnItemDeltaKind::CommandExecutionOutput => {
                    self.observe_item_delta(turn_id, item_id, kind.clone(), delta, rollout_index);
                    if let Some(error) = self.validate_active_lifecycle(turn_id, item_id, call_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::CommandExecutionOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id
                            .clone()
                            .or_else(|| self.call_id_for_item(item_id).map(str::to_owned)),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::ToolOutput => {
                    self.observe_item_delta(turn_id, item_id, kind.clone(), delta, rollout_index);
                    if let Some(error) = self.validate_active_lifecycle(turn_id, item_id, call_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::ToolOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id
                            .clone()
                            .or_else(|| self.call_id_for_item(item_id).map(str::to_owned)),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::FileChangeOutput => {
                    self.observe_item_delta(turn_id, item_id, kind.clone(), delta, rollout_index);
                    if let Some(error) = self.validate_active_lifecycle(turn_id, item_id, call_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::FileChangeOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id
                            .clone()
                            .or_else(|| self.call_id_for_item(item_id).map(str::to_owned)),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::JsonPatch => Vec::new(),
            },
            EventMsg::ItemCompleted { .. } => Vec::new(),
            EventMsg::TokenUsageUpdated {
                turn_id,
                last_usage,
                total_usage,
                model_context_window,
                ..
            } => vec![AppServerNotification::TokenUsageUpdated {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                last_usage: last_usage.clone(),
                total_usage: total_usage.clone(),
                model_context_window: *model_context_window,
            }],
            EventMsg::ModelRetrying {
                turn_id,
                stage,
                attempt,
                next_delay_ms,
            } => vec![AppServerNotification::ModelRetrying {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                stage: stage.clone(),
                attempt: *attempt,
                next_delay_ms: *next_delay_ms,
            }],
            EventMsg::ContextCompacted {
                turn_id,
                continuation,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                pre_message_count,
                post_message_count,
                preserved_tail_count,
            } => vec![AppServerNotification::ContextCompacted {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                continuation: *continuation,
                pre_context_tokens_estimate: *pre_context_tokens_estimate,
                post_context_tokens_estimate: *post_context_tokens_estimate,
                pre_message_count: *pre_message_count,
                post_message_count: *post_message_count,
                preserved_tail_count: *preserved_tail_count,
            }],
            EventMsg::ContextCompactionStarted {
                turn_id,
                continuation,
                estimated_tokens,
            } => vec![AppServerNotification::ContextCompactionStarted {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                continuation: *continuation,
                estimated_tokens: *estimated_tokens,
            }],
            EventMsg::ServerRequestRequested { turn_id, request } => {
                vec![AppServerNotification::ServerRequestRequested {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    request: request.clone(),
                }]
            }
            EventMsg::TurnCompleted { turn_id } => {
                self.observe_turn_state(turn_id, TurnState::Completed, rollout_index);
                Vec::new()
            }
            EventMsg::TurnFailed { turn_id, error } => {
                self.observe_turn_state(turn_id, TurnState::Failed, rollout_index);
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnFailed {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        error: error.clone(),
                    });
                Vec::new()
            }
            EventMsg::TurnCancelled { turn_id, reason } => {
                self.observe_turn_state(turn_id, TurnState::Cancelled, rollout_index);
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCancelled {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        reason: reason.clone(),
                    });
                Vec::new()
            }
            EventMsg::ServerRequestResolved { .. }
            | EventMsg::ModelRequestStarted { .. }
            | EventMsg::ModelResponseReceived { .. } => Vec::new(),
        }
    }

    fn project_core_transcript_event(
        &mut self,
        event: CoreTranscriptEvent,
        rollout_index: usize,
    ) -> Vec<AppServerNotification> {
        match event {
            CoreTranscriptEvent::ReasoningSummaryPartAdded {
                turn_id,
                item_id,
                summary_index,
            } => vec![AppServerNotification::ReasoningSummaryPartAdded {
                conversation_id: self.conversation_id.clone(),
                turn_id,
                item_id,
                summary_index,
            }],
            CoreTranscriptEvent::TurnCompleted { turn_id } => {
                if let Some(error) = self.active_items_not_closed_error(&turn_id) {
                    self.deferred_terminal_notifications.push(error);
                }
                self.retain_active_items_for_other_turns(&turn_id);
                self.clear_turn_projection_state(&turn_id);
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCompleted {
                        conversation_id: self.conversation_id.clone(),
                        turn_id,
                    });
                Vec::new()
            }
            CoreTranscriptEvent::ItemCompleted {
                turn_id,
                call_id,
                item,
            } => {
                let item_id = item.id().to_string();
                let lifecycle_error = self.validate_active_lifecycle(&turn_id, &item_id, &call_id);
                let lifecycle = self.remove_active_item(&item_id);
                self.observe_item_completed(&turn_id, &item_id, &item, rollout_index);
                let mut notifications = Vec::new();
                if let Some(error) = lifecycle_error {
                    notifications.push(error);
                }
                notifications.push(AppServerNotification::ItemCompleted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id,
                    call_id: call_id.or_else(|| lifecycle.and_then(|it| it.call_id)),
                    item,
                });
                notifications
            }
            CoreTranscriptEvent::AgentMessageDelta {
                turn_id,
                item_id,
                delta,
            } => {
                self.observe_item_delta(
                    &turn_id,
                    &item_id,
                    TurnItemDeltaKind::Text,
                    &delta,
                    rollout_index,
                );
                self.project_core_delta(
                    turn_id,
                    item_id,
                    delta,
                    |conversation_id, turn_id, item_id, delta| {
                        AppServerNotification::AgentMessageDelta {
                            conversation_id,
                            turn_id,
                            item_id,
                            delta,
                        }
                    },
                )
            }
            CoreTranscriptEvent::PlanDelta {
                turn_id,
                item_id,
                delta,
            } => self.project_core_delta(
                turn_id,
                item_id,
                delta,
                |conversation_id, turn_id, item_id, delta| AppServerNotification::PlanDelta {
                    conversation_id,
                    turn_id,
                    item_id,
                    delta,
                },
            ),
            CoreTranscriptEvent::ReasoningSummaryTextDelta {
                turn_id,
                item_id,
                summary_index,
                delta,
            } => {
                let mut notifications = Vec::new();
                if self.mark_reasoning_summary_part_opened(&item_id) {
                    notifications.push(AppServerNotification::ReasoningSummaryPartAdded {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        summary_index,
                    });
                }
                self.observe_item_delta(
                    &turn_id,
                    &item_id,
                    TurnItemDeltaKind::ReasoningSummary,
                    &delta,
                    rollout_index,
                );
                notifications.extend(self.project_core_delta(
                    turn_id,
                    item_id,
                    delta,
                    |conversation_id, turn_id, item_id, delta| {
                        AppServerNotification::ReasoningSummaryTextDelta {
                            conversation_id,
                            turn_id,
                            item_id,
                            summary_index,
                            delta,
                        }
                    },
                ));
                notifications
            }
            CoreTranscriptEvent::ReasoningTextDelta {
                turn_id,
                item_id,
                content_index,
                delta,
            } => {
                self.observe_item_delta(
                    &turn_id,
                    &item_id,
                    TurnItemDeltaKind::ReasoningText,
                    &delta,
                    rollout_index,
                );
                self.project_core_delta(
                    turn_id,
                    item_id,
                    delta,
                    |conversation_id, turn_id, item_id, delta| {
                        AppServerNotification::ReasoningTextDelta {
                            conversation_id,
                            turn_id,
                            item_id,
                            content_index,
                            delta,
                        }
                    },
                )
            }
        }
    }

    fn project_core_delta(
        &self,
        turn_id: String,
        item_id: String,
        delta: String,
        build: impl FnOnce(String, String, String, String) -> AppServerNotification,
    ) -> Vec<AppServerNotification> {
        if let Some(error) = self.validate_active_item(&turn_id, &item_id) {
            return vec![error];
        }
        vec![build(self.conversation_id.clone(), turn_id, item_id, delta)]
    }

    fn mark_reasoning_summary_part_opened(&mut self, item_id: &str) -> bool {
        let Some(item) = self.items_by_item_id.get_mut(item_id) else {
            return false;
        };
        if item.reasoning_summary_part_opened {
            return false;
        }
        item.reasoning_summary_part_opened = true;
        true
    }

    fn validate_active_item(&self, turn_id: &str, item_id: &str) -> Option<AppServerNotification> {
        match self.active_items_by_item_id.get(item_id) {
            Some(active) if active.turn_id == turn_id => None,
            Some(active) => Some(self.lifecycle_error(format!(
                "item `{item_id}` belongs to turn `{}` but received event for turn `{turn_id}`",
                active.turn_id
            ))),
            None => Some(self.lifecycle_error(format!(
                "item `{item_id}` received lifecycle event before item start"
            ))),
        }
    }

    fn validate_active_call(&self, turn_id: &str, call_id: &str) -> Option<AppServerNotification> {
        let Some(item_id) = self.active_item_id_by_call_id.get(call_id) else {
            return Some(self.lifecycle_error(format!(
                "call `{call_id}` received lifecycle event before item start"
            )));
        };
        match self.active_items_by_item_id.get(item_id) {
            Some(active) if active.turn_id == turn_id => None,
            Some(active) => Some(self.lifecycle_error(format!(
                "call `{call_id}` belongs to turn `{}` but received event for turn `{turn_id}`",
                active.turn_id
            ))),
            None => Some(self.lifecycle_error(format!(
                "call `{call_id}` points to missing active item `{item_id}`"
            ))),
        }
    }

    fn validate_active_lifecycle(
        &self,
        turn_id: &str,
        item_id: &str,
        call_id: &Option<String>,
    ) -> Option<AppServerNotification> {
        if let Some(call_id) = call_id.as_deref() {
            if let Some(error) = self.validate_active_call(turn_id, call_id) {
                return Some(error);
            }
            if let Some(active) = self.active_items_by_item_id.get(item_id)
                && active.call_id.as_deref() != Some(call_id)
            {
                return Some(self.lifecycle_error(format!(
                    "item `{item_id}` is associated with call `{}` but received lifecycle event for call `{call_id}`",
                    active.call_id.as_deref().unwrap_or("<missing>")
                )));
            }
            return None;
        }
        self.validate_active_item(turn_id, item_id)
    }

    fn active_items_not_closed_error(&self, turn_id: &str) -> Option<AppServerNotification> {
        let dangling = self
            .active_items_by_item_id
            .iter()
            .filter(|(_, active)| active.turn_id == turn_id)
            .map(|(_, active)| active.item_id.as_str())
            .collect::<Vec<_>>();
        (!dangling.is_empty()).then(|| {
            self.lifecycle_error(format!(
                "turn `{turn_id}` completed with active items: {}",
                dangling.join(", ")
            ))
        })
    }

    fn lifecycle_error(&self, message: String) -> AppServerNotification {
        AppServerNotification::Error {
            conversation_id: self.conversation_id.clone(),
            message,
        }
    }

    fn register_active_item(&mut self, lifecycle: ActiveLifecycle) {
        if let Some(call_id) = lifecycle.call_id.as_ref() {
            self.active_item_id_by_call_id
                .insert(call_id.clone(), lifecycle.item_id.clone());
        }
        self.active_items_by_item_id
            .insert(lifecycle.item_id.clone(), lifecycle);
    }

    fn remove_active_item(&mut self, item_id: &str) -> Option<ActiveLifecycle> {
        let lifecycle = self.active_items_by_item_id.remove(item_id)?;
        if let Some(call_id) = lifecycle.call_id.as_ref() {
            self.active_item_id_by_call_id.remove(call_id);
        }
        Some(lifecycle)
    }

    fn call_id_for_item(&self, item_id: &str) -> Option<&str> {
        self.active_items_by_item_id
            .get(item_id)
            .and_then(|lifecycle| lifecycle.call_id.as_deref())
    }

    fn observe_item_started(
        &mut self,
        turn_id: String,
        item_id: String,
        call_id: Option<String>,
        kind: TurnItemKind,
        title: Option<String>,
        rollout_index: usize,
    ) {
        let order_hint = self.bump_item_order_hint();
        self.turns_by_turn_id
            .entry(turn_id.clone())
            .or_insert_with(|| TurnProjectionState::new(rollout_index))
            .push_item(&item_id);
        self.touch_turn(&turn_id, rollout_index);
        self.items_by_item_id.insert(
            item_id.clone(),
            ProjectedItemState::new(turn_id, item_id, call_id, kind, title, order_hint),
        );
    }

    fn observe_item_delta(
        &mut self,
        turn_id: &str,
        item_id: &str,
        kind: TurnItemDeltaKind,
        delta: &str,
        rollout_index: usize,
    ) {
        if let Some(item) = self.items_by_item_id.get_mut(item_id) {
            item.apply_delta(kind, delta);
        }
        self.touch_turn(turn_id, rollout_index);
    }

    fn observe_item_completed(
        &mut self,
        turn_id: &str,
        item_id: &str,
        completed_item: &TranscriptItem,
        rollout_index: usize,
    ) {
        if let Some(item_state) = self.items_by_item_id.get_mut(item_id) {
            item_state.status = ProjectedItemStatus::Completed;
            item_state.kind = turn_item_kind_for_transcript_item(completed_item);
        }
        self.touch_turn(turn_id, rollout_index);
        self.items_by_item_id.insert(
            item_id.to_string(),
            projected_item_from_transcript_item(
                turn_id.to_string(),
                completed_item.clone(),
                rollout_index,
            ),
        );
        if let Some(turn) = self.turns_by_turn_id.get_mut(turn_id) {
            turn.push_item(item_id);
        }
    }

    fn clear_turn_projection_state(&mut self, turn_id: &str) {
        self.turns_by_turn_id.remove(turn_id);
        self.items_by_item_id
            .retain(|_, item| item.turn_id != turn_id);
    }

    fn bump_item_order_hint(&mut self) -> u64 {
        self.next_item_order_hint = self.next_item_order_hint.saturating_add(1);
        self.next_item_order_hint
    }

    fn bump_rollout_index(&mut self) -> usize {
        let current = self.next_rollout_index;
        self.next_rollout_index = self.next_rollout_index.saturating_add(1);
        current
    }

    fn observe_turn_started(
        &mut self,
        turn_id: String,
        user_input: Vec<InputItem>,
        rollout_index: usize,
    ) {
        self.active_turn_id = Some(turn_id.clone());
        self.turns_by_turn_id
            .insert(turn_id.clone(), TurnProjectionState::new(rollout_index));
        let user_item_id = format!("user:{turn_id}");
        let order_hint = self.bump_item_order_hint();
        self.turns_by_turn_id
            .get_mut(&turn_id)
            .expect("turn inserted above")
            .push_item(&user_item_id);
        self.items_by_item_id.insert(
            user_item_id.clone(),
            ProjectedItemState {
                turn_id,
                item_id: user_item_id,
                call_id: None,
                kind: TurnItemKind::UserMessage,
                title: None,
                status: ProjectedItemStatus::Completed,
                last_delta_kind: None,
                user_content: user_input.clone(),
                text_buffer: String::new(),
                reasoning_buffer: String::new(),
                tool_output_buffer: String::new(),
                reasoning_summary_part_opened: false,
                order_hint,
            },
        );
    }

    fn observe_turn_state(&mut self, turn_id: &str, state: TurnState, rollout_index: usize) {
        if let Some(turn) = self.turns_by_turn_id.get_mut(turn_id) {
            turn.state = state;
            turn.touch(rollout_index);
        }
    }

    fn touch_turn(&mut self, turn_id: &str, rollout_index: usize) {
        if let Some(turn) = self.turns_by_turn_id.get_mut(turn_id) {
            turn.touch(rollout_index);
        }
    }

    fn stable_item_ids_for_turn(&self, turn_id: &str) -> Vec<String> {
        let Some(turn) = self.turns_by_turn_id.get(turn_id) else {
            return Vec::new();
        };

        let mut output = Vec::with_capacity(turn.items_in_order.len());
        let mut emitted = HashMap::<String, ()>::new();

        for item_id in &turn.items_in_order {
            if emitted.contains_key(item_id) {
                continue;
            }
            if !self.items_by_item_id.contains_key(item_id) {
                continue;
            }
            output.push(item_id.clone());
            emitted.insert(item_id.clone(), ());
        }

        output
    }

    pub(crate) fn active_turn_snapshot(&self) -> Option<ConversationTurn> {
        let turn_id = self.active_turn_id.as_deref()?;
        self.turn_snapshot(turn_id)
    }

    pub(crate) fn turn_snapshot(&self, turn_id: &str) -> Option<ConversationTurn> {
        let turn = self.turns_by_turn_id.get(turn_id)?;
        let items = self
            .stable_item_ids_for_turn(turn_id)
            .into_iter()
            .filter_map(|item_id| self.items_by_item_id.get(&item_id))
            .filter_map(projected_item_to_transcript_item)
            .filter(|item| !projected_transcript_item_is_empty(item))
            .collect::<Vec<_>>();
        Some(ConversationTurn {
            id: turn_id.to_string(),
            state: turn.state.clone(),
            items,
            rollout_start_index: turn.rollout_start_index,
            rollout_end_index: turn.rollout_end_index,
        })
    }

    fn retain_active_items_for_other_turns(&mut self, turn_id: &str) {
        let stale_item_ids = self
            .active_items_by_item_id
            .iter()
            .filter(|(_, active)| active.turn_id == turn_id)
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        for item_id in stale_item_ids {
            let _ = self.remove_active_item(&item_id);
        }
        for turn in self.turns_by_turn_id.values_mut() {
            turn.retain_item_ids(|item_id| {
                self.items_by_item_id
                    .get(item_id)
                    .is_some_and(|item| item.turn_id != turn_id)
            });
        }
    }

    pub(crate) fn finish_turn(&mut self, turn_state: TurnState) -> Vec<AppServerNotification> {
        let mut notifications = std::mem::take(&mut self.deferred_terminal_notifications);
        if matches!(
            turn_state,
            TurnState::Completed | TurnState::Failed | TurnState::Cancelled
        ) {
            notifications.push(AppServerNotification::FrontendStateChanged {
                conversation_id: self.conversation_id.clone(),
                mode: FrontendMode::Idle,
            });
        }
        self.active_turn_id = None;
        notifications
    }
}

fn projected_item_from_transcript_item(
    turn_id: String,
    item: TranscriptItem,
    order_hint: usize,
) -> ProjectedItemState {
    let kind = turn_item_kind_for_transcript_item(&item);
    match item {
        TranscriptItem::SystemMessage { id, text } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: None,
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: text,
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::UserMessage { id, content } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: None,
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: content.clone(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::AgentMessage { id, text } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: None,
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: text,
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::Reasoning { id, title, text } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(title),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: text,
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: true,
            order_hint: order_hint as u64,
        },
        TranscriptItem::CommandExecution {
            id,
            tool_name: _,
            command,
            current_directory: _,
            status: _,
            exit_code: _,
            output: _,
            duration_ms: _,
            summary,
        } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(command),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: summary,
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::FileChange {
            id,
            tool_name: _,
            path,
            status: _,
            files_changed: _,
            summary,
        } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(path),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: summary,
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::ToolResult {
            id,
            tool_name,
            content,
            summary,
            structured: _,
        } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(tool_name),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: if summary.trim().is_empty() {
                content
            } else {
                summary
            },
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
    }
}

fn projected_item_to_transcript_item(item: &ProjectedItemState) -> Option<TranscriptItem> {
    match item.kind {
        TurnItemKind::UserMessage => Some(TranscriptItem::UserMessage {
            id: item.item_id.clone(),
            content: item.user_content.clone(),
        }),
        TurnItemKind::AssistantMessage => Some(TranscriptItem::AgentMessage {
            id: item.item_id.clone(),
            text: item.text_buffer.clone(),
        }),
        TurnItemKind::Reasoning => Some(TranscriptItem::Reasoning {
            id: item.item_id.clone(),
            title: item
                .title
                .clone()
                .unwrap_or_else(|| "reasoning".to_string()),
            text: item.reasoning_buffer.clone(),
        }),
        TurnItemKind::CommandExecution => Some(TranscriptItem::CommandExecution {
            id: item.item_id.clone(),
            tool_name: "exec_command".to_string(),
            command: item.title.clone().unwrap_or_default(),
            current_directory: String::new(),
            status: CommandExecutionStatus::InProgress,
            exit_code: None,
            output: Some(item.tool_output_buffer.clone()),
            duration_ms: None,
            summary: item.tool_output_buffer.clone(),
        }),
        TurnItemKind::FileChange => Some(TranscriptItem::FileChange {
            id: item.item_id.clone(),
            tool_name: "edit_file".to_string(),
            path: item.title.clone().unwrap_or_default(),
            status: WriteFileStatus::InProgress,
            files_changed: 0,
            summary: item.tool_output_buffer.clone(),
        }),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => Some(TranscriptItem::ToolResult {
            id: item.item_id.clone(),
            tool_name: item.title.clone().unwrap_or_else(|| "tool".to_string()),
            content: item.tool_output_buffer.clone(),
            summary: item.tool_output_buffer.clone(),
            structured: None,
        }),
        TurnItemKind::SystemNote => Some(TranscriptItem::SystemMessage {
            id: item.item_id.clone(),
            text: item.text_buffer.clone(),
        }),
    }
}

fn fallback_started_item(
    item_id: &str,
    kind: &TurnItemKind,
    title: Option<&str>,
) -> TranscriptItem {
    match projected_item_to_transcript_item(&ProjectedItemState::new(
        String::new(),
        item_id.to_string(),
        None,
        kind.clone(),
        title.map(str::to_string),
        0,
    )) {
        Some(item) => item,
        None => TranscriptItem::SystemMessage {
            id: item_id.to_string(),
            text: String::new(),
        },
    }
}

fn turn_item_kind_for_transcript_item(item: &TranscriptItem) -> TurnItemKind {
    match item {
        TranscriptItem::SystemMessage { .. } => TurnItemKind::SystemNote,
        TranscriptItem::UserMessage { .. } => TurnItemKind::UserMessage,
        TranscriptItem::AgentMessage { .. } => TurnItemKind::AssistantMessage,
        TranscriptItem::CommandExecution { .. } => TurnItemKind::CommandExecution,
        TranscriptItem::FileChange { .. } => TurnItemKind::FileChange,
        TranscriptItem::ToolResult { .. } => TurnItemKind::ToolResult,
        TranscriptItem::Reasoning { .. } => TurnItemKind::Reasoning,
    }
}

fn projected_transcript_item_is_empty(item: &TranscriptItem) -> bool {
    match item {
        TranscriptItem::SystemMessage { text, .. }
        | TranscriptItem::AgentMessage { text, .. }
        | TranscriptItem::Reasoning { text, .. } => text.trim().is_empty(),
        TranscriptItem::UserMessage { content, .. } => {
            agent_core::input_items_to_plain_text(content)
                .trim()
                .is_empty()
        }
        TranscriptItem::CommandExecution { summary, .. }
        | TranscriptItem::FileChange { summary, .. }
        | TranscriptItem::ToolResult { summary, .. } => summary.trim().is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::ConversationNotificationProjector;
    use agent_core::{
        CommandExecutionStatus, CompactionContinuation, EventMsg, ModelRetryStage, ModelUsage,
        TranscriptItem, TurnItemDeltaKind, TurnItemKind, TurnState,
    };
    use agent_protocol::{AppServerNotification, FrontendMode};

    #[test]
    fn terminal_notifications_are_deferred_until_finish() {
        let mut projector = ConversationNotificationProjector::new("default");

        let immediate = projector.project_turn_event(&EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        });
        assert!(immediate.is_empty());

        let flushed = projector.finish_turn(TurnState::Completed);
        assert_eq!(flushed.len(), 2);
        assert!(matches!(
            &flushed[0],
            AppServerNotification::TurnCompleted { turn_id, .. } if turn_id == "turn-1"
        ));
        assert!(matches!(
            &flushed[1],
            AppServerNotification::FrontendStateChanged {
                mode: FrontendMode::Idle,
                ..
            }
        ));
    }

    #[test]
    fn file_change_output_delta_projects_to_file_change_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:write".to_string(),
            call_id: Some("call-write".to_string()),
            kind: TurnItemKind::FileChange,
            title: Some("edit_file".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:write".to_string(),
            call_id: Some("call-write".to_string()),
            kind: TurnItemDeltaKind::FileChangeOutput,
            segment_index: None,
            delta: "wrote note.txt".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::FileChangeOutputDelta {
                item_id,
                call_id,
                delta,
                ..
            }
                if item_id == "tool:write"
                    && call_id.as_deref() == Some("call-write")
                    && delta == "wrote note.txt"
        ));
    }

    #[test]
    fn command_execution_output_delta_projects_to_command_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "D:\\work".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::CommandExecutionOutputDelta {
                item_id,
                call_id,
                delta,
                ..
            }
                if item_id == "tool:shell"
                    && call_id.as_deref() == Some("call-shell")
                    && delta == "D:\\work"
        ));
    }

    #[test]
    fn generic_tool_output_delta_projects_to_tool_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:custom".to_string(),
            call_id: Some("call-custom".to_string()),
            kind: TurnItemKind::ToolCall,
            title: Some("custom_tool".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:custom".to_string(),
            call_id: Some("call-custom".to_string()),
            kind: TurnItemDeltaKind::ToolOutput,
            segment_index: None,
            delta: "custom output".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ToolOutputDelta {
                item_id,
                call_id,
                delta,
                ..
            }
                if item_id == "tool:custom"
                    && call_id.as_deref() == Some("call-custom")
                    && delta == "custom output"
        ));
    }

    #[test]
    fn token_usage_projects_to_conversation_notification() {
        let mut projector = ConversationNotificationProjector::new("default");
        let usage = ModelUsage {
            input_tokens: 100,
            cached_input_tokens: 25,
            output_tokens: 40,
            reasoning_output_tokens: 5,
            total_tokens: 140,
        };

        let notifications = projector.project_turn_event(&EventMsg::TokenUsageUpdated {
            turn_id: "turn-1".to_string(),
            last_usage: usage.clone(),
            total_usage: usage.clone(),
            model_context_window: Some(1000),
            request_estimated_tokens: 130,
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::TokenUsageUpdated {
                conversation_id,
                turn_id,
                last_usage,
                total_usage,
                model_context_window,
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && last_usage.total_tokens == 140
                && total_usage.cached_input_tokens == 25
                && *model_context_window == Some(1000)
        ));
    }

    #[test]
    fn model_retrying_projects_to_conversation_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        let notifications = projector.project_turn_event(&EventMsg::ModelRetrying {
            turn_id: "turn-1".to_string(),
            stage: ModelRetryStage::Streaming,
            attempt: 2,
            next_delay_ms: 500,
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ModelRetrying {
                conversation_id,
                turn_id,
                stage,
                attempt,
                next_delay_ms,
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && *stage == ModelRetryStage::Streaming
                && *attempt == 2
                && *next_delay_ms == 500
        ));
    }

    #[test]
    fn context_compaction_notifications_preserve_continuation() {
        let mut projector = ConversationNotificationProjector::new("default");

        let started = projector.project_turn_event(&EventMsg::ContextCompactionStarted {
            turn_id: "turn-1".to_string(),
            continuation: CompactionContinuation::MidTurn,
            estimated_tokens: 12_345,
        });
        let compacted = projector.project_turn_event(&EventMsg::ContextCompacted {
            turn_id: "turn-1".to_string(),
            continuation: CompactionContinuation::MidTurn,
            pre_context_tokens_estimate: 12_345,
            post_context_tokens_estimate: 4_321,
            pre_message_count: 20,
            post_message_count: 6,
            preserved_tail_count: 4,
        });

        assert!(matches!(
            &started[0],
            AppServerNotification::ContextCompactionStarted {
                continuation,
                estimated_tokens,
                ..
            } if *continuation == CompactionContinuation::MidTurn
                && *estimated_tokens == 12_345
        ));
        assert!(matches!(
            &compacted[0],
            AppServerNotification::ContextCompacted {
                continuation,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                ..
            } if *continuation == CompactionContinuation::MidTurn
                && *pre_context_tokens_estimate == 12_345
                && *post_context_tokens_estimate == 4_321
        ));
    }

    #[test]
    fn assistant_text_delta_projects_to_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "hello".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::AgentMessageDelta {
                conversation_id,
                turn_id,
                item_id,
                delta,
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && item_id == "assistant:1"
                && delta == "hello"
        ));
    }

    #[test]
    fn reasoning_text_delta_projects_to_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("reasoning".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningText,
            segment_index: None,
            delta: "step".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ReasoningTextDelta {
                conversation_id,
                turn_id,
                item_id,
                delta,
                ..
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && item_id == "reasoning:1"
                && delta == "step"
        ));
    }

    #[test]
    fn reasoning_summary_delta_projects_to_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("reasoning".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningSummary,
            segment_index: None,
            delta: "summary".to_string(),
        });

        assert_eq!(notifications.len(), 2);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ReasoningSummaryPartAdded {
                conversation_id,
                turn_id,
                item_id,
                summary_index,
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && item_id == "reasoning:1"
                && *summary_index == 0
        ));
        assert!(matches!(
            &notifications[1],
            AppServerNotification::ReasoningSummaryTextDelta {
                conversation_id,
                turn_id,
                item_id,
                summary_index,
                delta,
                ..
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && item_id == "reasoning:1"
                && *summary_index == 0
                && delta == "summary"
        ));
    }

    #[test]
    fn item_completed_clears_active_lifecycle() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            item: TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "done".to_string(),
            },
        });
        let terminal = projector.project_turn_event(&EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        });
        let flushed = projector.finish_turn(TurnState::Completed);

        assert!(matches!(
            completed.as_slice(),
            [AppServerNotification::ItemCompleted { item, .. }]
                if matches!(
                    item,
                    TranscriptItem::AgentMessage { id, text }
                        if id == "assistant:1" && text == "done"
                )
        ));
        assert!(terminal.is_empty());
        assert_eq!(flushed.len(), 2);
        assert!(
            !flushed
                .iter()
                .any(|notification| matches!(notification, AppServerNotification::Error { .. }))
        );
    }

    #[test]
    fn assistant_item_completed_projects_final_source() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            item: TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "done".to_string(),
            },
        });

        assert!(matches!(
            completed.as_slice(),
            [AppServerNotification::ItemCompleted {
                conversation_id,
                turn_id,
                call_id,
                item,
            }] if conversation_id == "default"
                && turn_id == "turn-1"
                && call_id.is_none()
                && matches!(
                    item,
                    TranscriptItem::AgentMessage { id, text }
                        if id == "assistant:1" && text == "done"
                )
        ));
    }

    #[test]
    fn reasoning_item_completed_projects_final_source() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("thinking".to_string()),
        });
        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            item: TranscriptItem::Reasoning {
                id: "reasoning:1".to_string(),
                title: "thinking".to_string(),
                text: "final reasoning".to_string(),
            },
        });

        assert!(matches!(
            completed.as_slice(),
            [AppServerNotification::ItemCompleted {
                conversation_id,
                turn_id,
                call_id,
                item,
            }] if conversation_id == "default"
                && turn_id == "turn-1"
                && call_id.is_none()
                && matches!(
                    item,
                    TranscriptItem::Reasoning { id, title, text }
                        if id == "reasoning:1"
                            && title == "thinking"
                            && text == "final reasoning"
                )
        ));
    }

    #[test]
    fn mismatched_call_id_reports_lifecycle_error() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-other".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "D:\\work".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::Error { message, .. }
                if message.contains("call `call-other` received lifecycle event before item start")
        ));
    }

    #[test]
    fn item_completed_prefers_event_call_id_when_lifecycle_is_missing() {
        let mut projector = ConversationNotificationProjector::new("default");

        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            item: TranscriptItem::ToolResult {
                id: "tool:shell".to_string(),
                tool_name: "exec_command".to_string(),
                content: "done".to_string(),
                summary: "done".to_string(),
                structured: None,
            },
        });

        assert!(matches!(
            completed.last(),
            Some(AppServerNotification::ItemCompleted { call_id, .. })
                if call_id.as_deref() == Some("call-shell")
        ));
    }

    #[test]
    fn completed_item_removes_call_id_index() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let _ = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            item: TranscriptItem::CommandExecution {
                id: "tool:shell".to_string(),
                tool_name: "exec_command".to_string(),
                command: "pwd".to_string(),
                current_directory: "D:\\work".to_string(),
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                output: Some("D:\\work".to_string()),
                duration_ms: Some(1),
                summary: "D:\\work".to_string(),
            },
        });

        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:other".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "should fail".to_string(),
        });

        assert!(matches!(
            notifications.first(),
            Some(AppServerNotification::Error { message, .. })
                if message.contains("call `call-shell` received lifecycle event before item start")
        ));
    }

    #[test]
    fn call_id_turn_mismatch_reports_lifecycle_error() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-2".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "D:\\other".to_string(),
        });

        assert!(matches!(
            notifications.first(),
            Some(AppServerNotification::Error { message, .. })
                if message.contains("call `call-shell` belongs to turn `turn-1`")
        ));
    }

    #[test]
    fn turn_completed_reports_dangling_active_items() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        projector.project_turn_event(&EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        });
        let flushed = projector.finish_turn(TurnState::Completed);

        assert!(flushed.iter().any(|notification| matches!(
            notification,
            AppServerNotification::Error { message, .. }
                if message.contains("completed with active items")
        )));
    }

    #[test]
    fn stable_item_ids_preserve_arrival_order_when_reasoning_starts_late() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("reasoning".to_string()),
        });

        let ordered = projector.stable_item_ids_for_turn("turn-1");
        assert_eq!(ordered, vec!["assistant:1", "reasoning:1"]);
    }

    #[test]
    fn tool_items_preserve_arrival_order_relative_to_assistant() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("reasoning".to_string()),
        });

        let ordered = projector.stable_item_ids_for_turn("turn-1");
        assert_eq!(ordered, vec!["assistant:1", "tool:1", "reasoning:1"]);
    }

    #[test]
    fn active_turn_snapshot_preserves_late_reasoning_after_assistant_when_it_arrives_late() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: agent_core::text_input_items("hi"),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("reasoning".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningSummary,
            segment_index: None,
            delta: "first".to_string(),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("pwd".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "D:\\work".to_string(),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "answer".to_string(),
        });
        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:2".to_string(),
            call_id: None,
            kind: TurnItemKind::Reasoning,
            title: Some("reasoning".to_string()),
        });
        projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:2".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningSummary,
            segment_index: None,
            delta: "second".to_string(),
        });

        let snapshot = projector
            .active_turn_snapshot()
            .expect("active turn snapshot should exist");

        let ids = snapshot
            .items
            .iter()
            .map(|item| item.id().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "user:turn-1",
                "reasoning:1",
                "tool:1",
                "assistant:1",
                "reasoning:2",
            ]
        );
    }
}
