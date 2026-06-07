use crate::projection::transcript_item_projection::{
    fallback_started_item, projected_item_from_transcript_item, projected_item_to_transcript_item,
    projected_transcript_item_is_empty, turn_item_kind_for_transcript_item,
};
use crate::projection::turn_projection_state::{
    ActiveLifecycle, ProjectedItemState, ProjectedItemStatus, TurnProjectionState,
};
use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::projection::{CoreTranscriptEvent, core_transcript_event_from_event_msg};
use agent_core::{EventMsg, TurnItemDeltaKind, TurnItemKind, TurnState};
use agent_protocol::AppServerNotification;
use std::collections::HashMap;

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
        let conversation_id = conversation_id.into();
        Self {
            conversation_id,
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
                preserved_user_count,
            } => {
                vec![AppServerNotification::ContextCompacted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: Some(turn_id.clone()),
                    continuation: *continuation,
                    pre_context_tokens_estimate: *pre_context_tokens_estimate,
                    post_context_tokens_estimate: *post_context_tokens_estimate,
                    pre_message_count: *pre_message_count,
                    post_message_count: *post_message_count,
                    preserved_user_count: *preserved_user_count,
                }]
            }
            EventMsg::ContextCompactionStarted {
                turn_id,
                continuation,
                estimated_tokens,
            } => {
                vec![AppServerNotification::ContextCompactionStarted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: Some(turn_id.clone()),
                    continuation: *continuation,
                    estimated_tokens: *estimated_tokens,
                }]
            }
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
        let notifications = std::mem::take(&mut self.deferred_terminal_notifications);
        let _ = turn_state;
        self.active_turn_id = None;
        notifications
    }

    pub(crate) fn project_system_error(&mut self, message: String) -> Vec<AppServerNotification> {
        vec![AppServerNotification::Error {
            conversation_id: self.conversation_id.clone(),
            message,
        }]
    }
}

#[cfg(test)]
#[path = "conversation_notifications_tests.rs"]
mod conversation_notifications_tests;
