use crate::app::TuiApp;
use crate::app::conversation::exploration::{
    humanize_exploration_tool_title, is_exploration_command, is_exploration_tool,
    summarize_exploration_command,
};
use crate::state::{
    ActiveAssistantState, ActiveExecCall, ActiveExecMode, ActiveExecRouteKey, ActiveExecSession,
    ActiveReasoningState,
};
use crate::ui::widgets::history_cell::{
    HistoryCell, HistoryFormat, HistoryKind, render_active_control_placeholder,
};
use agent_protocol::TurnItemKind;

impl TuiApp {
    fn alloc_live_block_id(&mut self) -> u64 {
        self.transcript_state.next_live_block_id =
            self.transcript_state.next_live_block_id.saturating_add(1);
        self.transcript_state.next_live_block_id
    }

    fn bump_live_event_seq(&mut self) -> u64 {
        self.transcript_state.next_live_block_id =
            self.transcript_state.next_live_block_id.saturating_add(1);
        self.transcript_state.next_live_block_id
    }

    pub(crate) fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        let _ = turn_id;
        self.flush_active_assistant_to_transcript();
        self.flush_active_exec_to_transcript();
        self.consolidate_exploration_stage();
        let block_id = self.alloc_live_block_id();
        let cell = HistoryCell::agent("", String::new(), HistoryFormat::Markdown);
        self.transcript_state.active_assistant = Some(ActiveAssistantState {
            block_id,
            item_id: item_id.to_string(),
            cell,
            order: self.bump_live_event_seq(),
            completed: false,
        });
    }

    pub(crate) fn handle_assistant_item_delta(&mut self, item_id: &str, delta: &str) {
        let next_order = self.bump_live_event_seq();
        let Some(active) = self.transcript_state.active_assistant.as_mut() else {
            return;
        };
        if active.item_id != item_id {
            return;
        }
        active.cell.append_body(delta);
        active.order = next_order;
        active.completed = false;
    }

    pub(crate) fn handle_assistant_item_completed(&mut self, item_id: &str, output: &str) {
        if self
            .transcript_state
            .active_assistant
            .as_ref()
            .is_none_or(|active| active.item_id != item_id)
        {
            self.flush_active_assistant_to_transcript();
            let block_id = self.alloc_live_block_id();
            let cell = HistoryCell::agent("", String::new(), HistoryFormat::Markdown);
            self.transcript_state.active_assistant = Some(ActiveAssistantState {
                block_id,
                item_id: item_id.to_string(),
                cell,
                order: self.bump_live_event_seq(),
                completed: false,
            });
        }
        let next_order = self.bump_live_event_seq();
        if let Some(active) = self.transcript_state.active_assistant.as_mut() {
            active.cell.replace_body(output);
            active.order = next_order;
            active.completed = true;
        }
        let has_text = self
            .transcript_state
            .active_assistant
            .as_ref()
            .is_some_and(|active| !active.cell.body().trim().is_empty());
        if has_text {
            self.transcript_state.last_copyable_output = self
                .transcript_state
                .active_assistant
                .as_ref()
                .map(|active| active.cell.body().to_string());
            if let Some(active) = self.transcript_state.active_assistant.as_ref() {
                self.push_cell(active.cell.clone());
            }
        }
        self.clear_assistant_live();
    }

    pub(crate) fn handle_reasoning_item_started(&mut self, item_id: &str, title: &str) {
        let block_id = self.alloc_live_block_id();
        self.transcript_state.active_reasoning = Some(ActiveReasoningState {
            block_id,
            item_id: item_id.to_string(),
            title: title.to_string(),
            text: String::new(),
            order: self.bump_live_event_seq(),
            completed: false,
        });
    }

    pub(crate) fn handle_reasoning_item_completed(
        &mut self,
        item_id: &str,
        title: &str,
        output: &str,
    ) {
        if self
            .transcript_state
            .active_reasoning
            .as_ref()
            .is_none_or(|active| active.item_id != item_id)
        {
            let block_id = self.alloc_live_block_id();
            self.transcript_state.active_reasoning = Some(ActiveReasoningState {
                block_id,
                item_id: item_id.to_string(),
                title: title.to_string(),
                text: String::new(),
                order: self.bump_live_event_seq(),
                completed: false,
            });
        }
        let next_order = self.bump_live_event_seq();
        if let Some(active) = self.transcript_state.active_reasoning.as_mut() {
            active.text.clear();
            active.text.push_str(output);
            active.completed = true;
            active.order = next_order;
        }
        if let Some(active) = self.transcript_state.active_reasoning.as_ref() {
            let mut cell = active.to_history_cell();
            cell.expanded = self.run_state.expand_tool_details;
            if !cell.body().trim().is_empty() {
                if self.transcript_state.active_assistant.is_none() {
                    let inserted = self.insert_cell_before_trailing_agent(cell.clone());
                    if !inserted {
                        self.push_cell(cell);
                    }
                } else {
                    self.push_cell(cell);
                }
            }
        }
        self.clear_reasoning_buffer();
    }

    pub(crate) fn handle_reasoning_item_delta(&mut self, item_id: &str, delta: &str) {
        let next_order = self.bump_live_event_seq();
        let Some(active) = self.transcript_state.active_reasoning.as_mut() else {
            return;
        };
        if active.item_id != item_id {
            return;
        }
        active.text.push_str(delta);
        active.order = next_order;
        active.completed = false;
    }

    pub(crate) fn handle_control_item_started_with_route_key(
        &mut self,
        _item_id: &str,
        route_key: ActiveExecRouteKey,
        kind: TurnItemKind,
        title: &str,
    ) {
        if let Some(detail) = classify_exploration_start(&kind, title) {
            self.clear_exec_live();
            self.start_exploration_session(route_key, detail);
            return;
        }
        if self.transcript_state.active_exec.is_some() {
            self.clear_exec_live();
        } else {
            self.flush_active_exec_to_transcript();
        }
        self.start_command_session(route_key, kind, title);
    }

    pub(crate) fn handle_control_item_completed_with_route_key(
        &mut self,
        _item_id: &str,
        route_key: ActiveExecRouteKey,
        cell: HistoryCell,
    ) {
        if cell.kind() == HistoryKind::Exploration {
            self.complete_exploration_session(&route_key, &cell);
            return;
        }
        if self.complete_command_session(&route_key, &cell) {
            return;
        }
        if cell.kind() != HistoryKind::Exploration {
            self.consolidate_exploration_stage();
        }
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        self.push_cell(cell);
    }

    pub(crate) fn handle_control_item_delta_with_route_key(
        &mut self,
        _item_id: &str,
        route_key: &ActiveExecRouteKey,
        delta: &str,
    ) {
        let route_is_active = self
            .transcript_state
            .active_exec
            .as_ref()
            .is_some_and(|session| session.contains_call(route_key));
        if !route_is_active {
            return;
        }
        let _ = self.append_active_exec_delta(route_key, delta);
    }

    pub(crate) fn flush_reasoning_buffer_to_transcript(&mut self) {
        if let Some(active) = self.transcript_state.active_reasoning.as_ref()
            && !active.completed
        {
            let mut cell = active.to_history_cell();
            cell.expanded = self.run_state.expand_tool_details;
            if !cell.body().trim().is_empty() {
                if self.transcript_state.active_assistant.is_none() {
                    let inserted = self.insert_cell_before_trailing_agent(cell.clone());
                    if !inserted {
                        self.push_cell(cell);
                    }
                } else {
                    self.push_cell(cell);
                }
            }
        }
        self.clear_reasoning_buffer();
    }

    fn clear_assistant_live(&mut self) {
        self.transcript_state.active_assistant = None;
    }

    fn clear_exec_live(&mut self) {
        self.transcript_state.active_exec = None;
    }

    fn clear_live_cells(&mut self) {
        self.clear_assistant_live();
        self.clear_exec_live();
    }

    fn clear_reasoning_buffer(&mut self) {
        self.transcript_state.active_reasoning = None;
    }

    pub(crate) fn flush_live_cells_to_transcript(&mut self) {
        let mut cells = Vec::new();
        if let Some(cell) = self.take_active_assistant_history_cell() {
            cells.push(cell);
        }
        if let Some(cell) = self.take_active_exec_history_cell() {
            cells.push(cell);
        }
        if cells.is_empty() {
            self.clear_live_cells();
            return;
        }
        cells.sort_by_key(|(order, _)| *order);
        for (_, cell) in cells {
            self.push_cell(cell);
        }
        self.clear_live_cells();
    }

    fn flush_active_assistant_to_transcript(&mut self) {
        let Some((_, cell)) = self.take_active_assistant_history_cell() else {
            self.clear_assistant_live();
            return;
        };
        self.push_cell(cell);
        self.clear_assistant_live();
    }

    fn flush_active_exec_to_transcript(&mut self) {
        let Some((_, cell)) = self.take_active_exec_history_cell() else {
            self.clear_exec_live();
            return;
        };
        self.push_cell(cell);
        self.clear_exec_live();
    }

    fn take_active_assistant_history_cell(&mut self) -> Option<(u64, HistoryCell)> {
        self.transcript_state.active_assistant.take().and_then(|active| {
            (!active.completed && !active.cell.body().trim().is_empty())
                .then_some((active.order, active.cell))
        })
    }

    fn take_active_exec_history_cell(&mut self) -> Option<(u64, HistoryCell)> {
        self.transcript_state.active_exec.take().and_then(|active| {
            let mut cell = active.to_history_cell(self.run_state.expand_tool_details);
            cell.expanded = false;
            (!cell.body().trim().is_empty()).then_some((active.order, cell))
        })
    }

    fn start_exploration_session(&mut self, route_key: ActiveExecRouteKey, detail: String) {
        let call = ActiveExecCall {
            route_key,
            label: "Exploring workspace".to_string(),
            summary: detail,
            detail: String::new(),
            completed: false,
        };
        let block_id = self.alloc_live_block_id();
        let order = self.bump_live_event_seq();
        self.transcript_state.active_exec =
            Some(ActiveExecSession::new_exploration(block_id, order, call));
    }

    fn complete_exploration_session(&mut self, route_key: &ActiveExecRouteKey, cell: &HistoryCell) {
        let Some(active) = self.transcript_state.active_exec.as_ref() else {
            self.push_cell(cell.clone());
            return;
        };
        if !matches!(active.mode, ActiveExecMode::Exploration { .. }) {
            self.push_cell(cell.clone());
            return;
        }
        if !active.contains_call(route_key) {
            self.push_cell(cell.clone());
            return;
        }
        if let Some(session) = self.transcript_state.active_exec.as_mut() {
            let _ = session.complete_call_or_only_pending(route_key);
        }
        if let Some(session) = self.transcript_state.active_exec.as_ref() {
            let mut block = session.to_history_cell(false);
            block.expanded = false;
            self.push_cell(block);
        }
        self.clear_exec_live();
    }

    fn start_command_session(
        &mut self,
        route_key: ActiveExecRouteKey,
        kind: TurnItemKind,
        title: &str,
    ) {
        let placeholder = render_active_control_placeholder(kind, title);
        let block_id = self.alloc_live_block_id();
        let order = self.bump_live_event_seq();
        self.transcript_state.active_exec = Some(ActiveExecSession::new_command(
            block_id,
            order,
            ActiveExecCall {
                route_key,
                label: placeholder.label().to_string(),
                summary: placeholder.body().to_string(),
                detail: String::new(),
                completed: false,
            },
        ));
    }

    fn complete_command_session(&mut self, route_key: &ActiveExecRouteKey, cell: &HistoryCell) -> bool {
        let Some(active) = self.transcript_state.active_exec.as_ref() else {
            return false;
        };
        if !matches!(active.mode, ActiveExecMode::Command) {
            return false;
        }
        if !active.contains_call(route_key) && active.has_pending_calls() {
            let mut session = active.clone();
            if !session.complete_call_or_only_pending(route_key) {
                return false;
            }
        } else if !active.contains_call(route_key) {
            return false;
        }
        if let Some(session) = self.transcript_state.active_exec.as_mut() {
            let _ = session.complete_call_or_only_pending(route_key);
        }
        if cell.kind() != HistoryKind::Exploration {
            self.consolidate_exploration_stage();
        }
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        if let Some(session) = self.transcript_state.active_exec.as_ref() {
            let mut block = session.to_history_cell(false);
            block.expanded = false;
            self.push_cell(block);
        }
        self.clear_exec_live();
        true
    }

    fn append_active_exec_delta(&mut self, route_key: &ActiveExecRouteKey, delta: &str) -> bool {
        let next_order = self.bump_live_event_seq();
        let Some(session) = self.transcript_state.active_exec.as_mut() else {
            return false;
        };
        if !session.append_delta(route_key, delta) {
            return false;
        }
        session.order = next_order;
        true
    }
}

fn compact_activity(title: &str) -> String {
    let single_line = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (index, ch) in single_line.chars().enumerate() {
        if index >= 48 {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn classify_exploration_start(kind: &TurnItemKind, title: &str) -> Option<String> {
    match kind {
        TurnItemKind::CommandExecution if is_exploration_command(title) => {
            Some(summarize_exploration_command(title))
        }
        TurnItemKind::ToolCall if is_exploration_tool(title) => {
            Some(humanize_exploration_tool_title(title))
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn compact_inline(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            return out;
        }
        out.push(if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        });
    }
    out
}
