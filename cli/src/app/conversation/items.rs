use crate::app::TuiApp;
use crate::state::{
    ActiveAssistantState, ActiveExecCall, ActiveExecMode, ActiveExecPresentation,
    ActiveExecRouteKey, ActiveExecSession, ActiveExecViewState, ActiveReasoningState,
    LiveOverlayEntry, LiveOverlayKind,
};
use crate::ui::widgets::history_cell::{
    ExplorationAggregate, HistoryCell, HistoryFormat, HistoryKind,
    render_active_control_placeholder,
};
use agent_protocol::TurnItemKind;

impl TuiApp {
    fn alloc_live_block_id(&mut self) -> u64 {
        self.transcript_state.next_live_block_id =
            self.transcript_state.next_live_block_id.saturating_add(1);
        self.transcript_state.next_live_block_id
    }

    fn bump_live_event_seq(&mut self) -> u64 {
        self.transcript_state.next_overlay_order =
            self.transcript_state.next_overlay_order.saturating_add(1);
        self.transcript_state.next_overlay_order
    }

    fn push_live_overlay(&mut self, id: u64, kind: LiveOverlayKind, cell: HistoryCell) {
        self.transcript_state.live_overlays.push(LiveOverlayEntry {
            id,
            kind,
            cell,
        });
    }

    fn update_live_overlay(&mut self, id: u64, cell: HistoryCell) {
        if let Some(entry) = self
            .transcript_state
            .live_overlays
            .iter_mut()
            .find(|entry| entry.id == id)
        {
            entry.cell = cell;
        }
    }

    fn upsert_live_overlay_snapshot(&mut self, id: u64, kind: LiveOverlayKind, cell: HistoryCell) {
        if self
            .transcript_state
            .live_overlays
            .iter()
            .any(|entry| entry.id == id)
        {
            self.update_live_overlay(id, cell);
        } else {
            self.push_live_overlay(id, kind, cell);
        }
    }

    fn remove_live_overlay(&mut self, id: u64) {
        self.transcript_state
            .live_overlays
            .retain(|entry| entry.id != id);
    }

    fn sync_assistant_overlay(&mut self) {
        let Some(active) = self.transcript_state.active_assistant.clone() else {
            return;
        };
        if active.cell.body().trim().is_empty() {
            self.remove_live_overlay(active.block_id);
            return;
        }
        self.upsert_live_overlay_snapshot(active.block_id, LiveOverlayKind::Assistant, active.cell);
    }

    fn sync_exec_overlay(&mut self) {
        let Some(active) = self.transcript_state.active_exec_view.clone() else {
            return;
        };
        let cell = active.to_history_cell();
        if cell.body().trim().is_empty() {
            self.remove_live_overlay(active.block_id);
            return;
        }
        self.upsert_live_overlay_snapshot(active.block_id, LiveOverlayKind::Exec, cell);
    }

    fn sync_reasoning_overlay(&mut self) {
        let Some(active) = self.transcript_state.active_reasoning.clone() else {
            return;
        };
        if active.text.trim().is_empty() {
            self.remove_live_overlay(active.block_id);
            return;
        }
        self.upsert_live_overlay_snapshot(
            active.block_id,
            LiveOverlayKind::Reasoning,
            active.to_history_cell(),
        );
    }

    pub(crate) fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        let _ = turn_id;
        self.flush_active_assistant_to_transcript();
        self.flush_active_exec_to_transcript();
        self.consolidate_exploration_stage();
        let block_id = self.alloc_live_block_id();
        let cell = HistoryCell::agent("", String::new(), HistoryFormat::Markdown);
        self.push_live_overlay(block_id, LiveOverlayKind::Assistant, cell.clone());
        self.transcript_state.active_assistant = Some(ActiveAssistantState {
            block_id,
            item_id: item_id.to_string(),
            cell,
            order: self.bump_live_event_seq(),
            completed: false,
        });
        self.sync_assistant_overlay();
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
        self.sync_assistant_overlay();
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
            self.push_live_overlay(block_id, LiveOverlayKind::Assistant, cell.clone());
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
        self.sync_assistant_overlay();
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
        } else {
            self.clear_assistant_live();
        }
    }

    pub(crate) fn handle_reasoning_item_started(&mut self, item_id: &str, title: &str) {
        let block_id = self.alloc_live_block_id();
        let state = ActiveReasoningState {
            block_id,
            item_id: item_id.to_string(),
            title: title.to_string(),
            text: String::new(),
            order: self.bump_live_event_seq(),
            completed: false,
        };
        self.push_live_overlay(block_id, LiveOverlayKind::Reasoning, state.to_history_cell());
        self.transcript_state.active_reasoning = Some(state);
        self.sync_reasoning_overlay();
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
            let state = ActiveReasoningState {
                block_id,
                item_id: item_id.to_string(),
                title: title.to_string(),
                text: String::new(),
                order: self.bump_live_event_seq(),
                completed: false,
            };
            self.push_live_overlay(block_id, LiveOverlayKind::Reasoning, state.to_history_cell());
            self.transcript_state.active_reasoning = Some(state);
        }
        let next_order = self.bump_live_event_seq();
        if let Some(active) = self.transcript_state.active_reasoning.as_mut() {
            active.text.clear();
            active.text.push_str(output);
            active.completed = true;
            active.order = next_order;
        }
        self.sync_reasoning_overlay();
        if let Some(active) = self.transcript_state.active_reasoning.as_ref() {
            let mut cell = active.to_history_cell();
            cell.expanded = self.run_state.expand_tool_details;
            if !cell.body().trim().is_empty() {
                self.push_cell(cell);
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
        self.sync_reasoning_overlay();
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
        if cell.kind() != crate::ui::widgets::history_cell::HistoryKind::Exploration {
            self.consolidate_exploration_stage();
        }
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        if matches!(
            self.transcript_state
                .active_exec_view
                .as_ref()
                .map(|active| active.to_history_cell().kind()),
            Some(crate::ui::widgets::history_cell::HistoryKind::Command)
                | Some(crate::ui::widgets::history_cell::HistoryKind::Exploration)
                | Some(crate::ui::widgets::history_cell::HistoryKind::Notice)
        ) {
            self.clear_exec_live();
        }
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
        if self.append_active_exec_delta(route_key, delta) {
            return;
        }
        if let Some(active) = self.transcript_state.active_exec_view.as_mut()
            && let ActiveExecPresentation::Command { detail, .. } = &mut active.presentation
        {
            match detail {
                Some(existing) => existing.push_str(delta),
                None => *detail = Some(delta.to_string()),
            }
            self.sync_exec_overlay();
        }
    }

    pub(crate) fn flush_reasoning_buffer_to_transcript(&mut self) {
        if let Some(active) = self.transcript_state.active_reasoning.as_ref()
            && !active.completed
        {
            let mut cell = active.to_history_cell();
            cell.expanded = self.run_state.expand_tool_details;
            if !cell.body().trim().is_empty() {
                self.push_cell(cell);
            }
        }
        self.clear_reasoning_buffer();
    }

    fn clear_assistant_live(&mut self) {
        if let Some(active) = self.transcript_state.active_assistant.take() {
            self.remove_live_overlay(active.block_id);
        }
    }

    fn clear_exec_live(&mut self) {
        self.transcript_state.active_exec = None;
        if let Some(active) = self.transcript_state.active_exec_view.take() {
            self.remove_live_overlay(active.block_id);
        }
    }

    fn clear_live_cells(&mut self) {
        self.clear_assistant_live();
        self.clear_exec_live();
    }

    fn clear_reasoning_buffer(&mut self) {
        if let Some(active) = self.transcript_state.active_reasoning.take() {
            self.remove_live_overlay(active.block_id);
        }
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
        self.transcript_state.active_exec_view.take().and_then(|active| {
            let cell = active.to_history_cell();
            (!active.committed && !cell.body().trim().is_empty()).then_some((active.order, cell))
        })
    }

    fn start_exploration_session(
        &mut self,
        route_key: ActiveExecRouteKey,
        detail: String,
    ) {
        let call = ActiveExecCall {
            route_key,
            label: "Exploring workspace".to_string(),
            summary: detail,
            detail: String::new(),
            completed: false,
        };
        self.transcript_state.active_exec = Some(ActiveExecSession::new_exploration(call));
        let block_id = self.alloc_live_block_id();
        self.transcript_state.active_exec_view = Some(ActiveExecViewState {
            block_id,
            presentation: ActiveExecPresentation::Command {
                label: String::new(),
                summary: String::new(),
                detail: None,
                expanded: self.run_state.expand_tool_details,
            },
            order: self.bump_live_event_seq(),
            committed: false,
        });
        self.push_live_overlay(
            block_id,
            LiveOverlayKind::Exec,
            HistoryCell::exec("", "", None, crate::ui::widgets::history_cell::HistoryTone::Control),
        );
        self.refresh_active_exec_cell();
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
        self.push_cell(cell.clone());
        self.clear_exec_live();
    }

    fn refresh_active_exec_cell(&mut self) {
        let existing_view = self.transcript_state.active_exec_view.clone();
        let Some(active) = self.transcript_state.active_exec.as_ref() else {
            return;
        };
        match &active.mode {
            ActiveExecMode::Exploration { .. } => {
                let Some(aggregate) = active.exploration_aggregate() else {
                    return;
                };
                let mut combined = aggregate.clone();
                for call in &active.calls {
                    if !call.summary.trim().is_empty() {
                        combined.push_detail(call.summary.clone());
                    }
                    if !call.detail.trim().is_empty() {
                        combined.push_detail(compact_inline(&call.detail.replace('\n', " "), 120));
                    }
                }
                let has_pending = active.has_pending_calls();
                let summary = format_active_exploration_summary(aggregate, has_pending);
                self.transcript_state.active_exec_view = Some(ActiveExecViewState {
                    block_id: existing_view
                        .as_ref()
                        .map(|active| active.block_id)
                        .unwrap_or(0),
                    presentation: ActiveExecPresentation::Exploration {
                        label: if has_pending {
                            "Exploring workspace".to_string()
                        } else {
                            "Explored workspace".to_string()
                        },
                        summary,
                        aggregate: combined,
                        expanded: self.run_state.expand_tool_details,
                    },
                    order: existing_view
                        .as_ref()
                        .map(|active| active.order)
                        .unwrap_or_else(|| self.bump_live_event_seq()),
                    committed: existing_view
                        .as_ref()
                        .is_some_and(|active| active.committed),
                });
                self.sync_exec_overlay();
            }
            ActiveExecMode::Command => {
                let Some(call) = active.last_call() else {
                    return;
                };
                let detail = (!call.detail.trim().is_empty()).then(|| call.detail.clone());
                self.transcript_state.active_exec_view = Some(ActiveExecViewState {
                    block_id: existing_view
                        .as_ref()
                        .map(|active| active.block_id)
                        .unwrap_or(0),
                    presentation: ActiveExecPresentation::Command {
                        label: call.label.clone(),
                        summary: call.summary.clone(),
                        detail: detail.or_else(|| Some("running".to_string())),
                        expanded: self.run_state.expand_tool_details,
                    },
                    order: existing_view
                        .as_ref()
                        .map(|active| active.order)
                        .unwrap_or_else(|| self.bump_live_event_seq()),
                    committed: existing_view
                        .as_ref()
                        .is_some_and(|active| active.committed),
                });
                self.sync_exec_overlay();
            }
        }
    }

    fn start_command_session(
        &mut self,
        route_key: ActiveExecRouteKey,
        kind: TurnItemKind,
        title: &str,
    ) {
        let placeholder = render_active_control_placeholder(kind, title);
        self.transcript_state.active_exec = Some(ActiveExecSession::new_command(ActiveExecCall {
            route_key,
            label: placeholder.label().to_string(),
            summary: placeholder.body().to_string(),
            detail: String::new(),
            completed: false,
        }));
        self.transcript_state.active_exec_view = Some(ActiveExecViewState {
            block_id: self.alloc_live_block_id(),
            presentation: ActiveExecPresentation::Command {
                label: placeholder.label().to_string(),
                summary: placeholder.body().to_string(),
                detail: None,
                expanded: self.run_state.expand_tool_details,
            },
            order: self.bump_live_event_seq(),
            committed: false,
        });
        if let Some(active) = self.transcript_state.active_exec_view.as_ref() {
            self.push_live_overlay(
                active.block_id,
                LiveOverlayKind::Exec,
                active.to_history_cell(),
            );
        }
        self.refresh_active_exec_cell();
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
        self.transcript_state.active_exec = None;
        if cell.kind() != crate::ui::widgets::history_cell::HistoryKind::Exploration {
            self.consolidate_exploration_stage();
        }
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        if let Some(active_view) = self.transcript_state.active_exec_view.as_mut() {
            active_view.presentation = match cell.kind() {
                HistoryKind::Exploration => active_view.presentation.clone(),
                _ => ActiveExecPresentation::Command {
                    label: cell.label().to_string(),
                    summary: cell.body().to_string(),
                    detail: cell.detail().map(ToOwned::to_owned),
                    expanded: cell.expanded,
                },
            };
            active_view.committed = true;
        }
        self.sync_exec_overlay();
        self.push_cell(cell.clone());
        self.clear_exec_live();
        true
    }

    fn append_active_exec_delta(&mut self, route_key: &ActiveExecRouteKey, delta: &str) -> bool {
        let Some(session) = self.transcript_state.active_exec.as_mut() else {
            return false;
        };
        if !session.append_delta(route_key, delta) {
            return false;
        }
        let next_order = self.bump_live_event_seq();
        if let Some(active_view) = self.transcript_state.active_exec_view.as_mut() {
            active_view.order = next_order;
        }
        self.refresh_active_exec_cell();
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
        TurnItemKind::ToolCall if is_exploration_tool(title) => Some(humanize_tool_title(title)),
        _ => None,
    }
}

fn is_exploration_tool(title: &str) -> bool {
    matches!(
        title,
        "read_file" | "search_workspace" | "read_directory" | "get_metadata"
    )
}

fn humanize_tool_title(tool_name: &str) -> String {
    match tool_name {
        "read_file" => "Read file".to_string(),
        "search_workspace" => "Search workspace".to_string(),
        "read_directory" => "Read directory".to_string(),
        "get_metadata" => "File info".to_string(),
        other => other.replace('_', " "),
    }
}

fn is_exploration_command(command: &str) -> bool {
    let normalized = command.trim().to_ascii_lowercase();
    normalized.starts_with("ls ")
        || normalized == "ls"
        || normalized.starts_with("dir ")
        || normalized == "dir"
        || normalized == "pwd"
        || normalized.starts_with("cat ")
        || normalized.starts_with("type ")
        || normalized.starts_with("rg ")
        || normalized.starts_with("grep ")
        || normalized.starts_with("findstr ")
        || normalized.starts_with("select-string ")
        || normalized.starts_with("git grep ")
}

fn summarize_exploration_command(command: &str) -> String {
    let compact = compact_inline(command.trim(), 72);
    if let Some((_, rhs)) = compact.rsplit_once("&&") {
        compact_inline(rhs.trim(), 56)
    } else {
        compact
    }
}

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

fn format_active_exploration_summary(
    aggregate: &ExplorationAggregate,
    has_pending: bool,
) -> String {
    let mut parts = Vec::new();
    if aggregate.searches > 0 {
        parts.push(format!(
            "searched {} time{}",
            aggregate.searches,
            if aggregate.searches == 1 { "" } else { "s" }
        ));
    }
    if aggregate.read_files > 0 {
        parts.push(format!(
            "read {} file{}",
            aggregate.read_files,
            if aggregate.read_files == 1 { "" } else { "s" }
        ));
    }
    if aggregate.listed_directories > 0 {
        parts.push(format!(
            "listed {} director{}",
            aggregate.listed_directories,
            if aggregate.listed_directories == 1 {
                "y"
            } else {
                "ies"
            }
        ));
    }
    if aggregate.metadata_reads > 0 {
        parts.push(format!(
            "checked {} path{}",
            aggregate.metadata_reads,
            if aggregate.metadata_reads == 1 { "" } else { "s" }
        ));
    }
    if aggregate.inspect_commands > 0 {
        parts.push(format!(
            "ran {} inspect command{}",
            aggregate.inspect_commands,
            if aggregate.inspect_commands == 1 { "" } else { "s" }
        ));
    }
    if has_pending {
        parts.push("running tool".to_string());
    }
    if parts.is_empty() {
        "exploring workspace".to_string()
    } else {
        parts.join(", ")
    }
}
