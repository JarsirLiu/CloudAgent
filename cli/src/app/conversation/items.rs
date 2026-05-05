use crate::app::TuiApp;
use crate::state::{ActiveExecCall, ActiveExecMode, ActiveExecRouteKey, ActiveExecSession};
use crate::ui::widgets::history_cell::{
    ExplorationAggregate, HistoryCell, HistoryFormat, HistoryKind, HistoryTone,
    render_active_control_placeholder,
};
use agent_protocol::TurnItemKind;

impl TuiApp {
    pub(crate) fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        let _ = turn_id;
        self.flush_reasoning_buffer_to_transcript();
        self.flush_active_cell_to_transcript();
        self.consolidate_exploration_stage();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(TurnItemKind::AssistantMessage);
        self.transcript_state.active_cell = Some(HistoryCell::agent(
            "",
            String::new(),
            HistoryFormat::Markdown,
        ));
    }

    pub(crate) fn handle_assistant_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id)
            || self.transcript_state.active_item_kind != Some(TurnItemKind::AssistantMessage)
        {
            return;
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.append_body(delta);
        }
    }

    pub(crate) fn handle_assistant_item_completed(&mut self, item_id: &str, output: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id)
            || self.transcript_state.active_item_kind != Some(TurnItemKind::AssistantMessage)
        {
            self.flush_active_cell_to_transcript();
            self.transcript_state.active_item_id = Some(item_id.to_string());
            self.transcript_state.active_item_kind = Some(TurnItemKind::AssistantMessage);
            self.transcript_state.active_cell = Some(HistoryCell::agent(
                "",
                String::new(),
                HistoryFormat::Markdown,
            ));
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.replace_body(output);
        }
        let has_text = self
            .transcript_state
            .active_cell
            .as_ref()
            .is_some_and(|cell| !cell.body().trim().is_empty());
        if has_text {
            self.transcript_state.last_copyable_output = self
                .transcript_state
                .active_cell
                .as_ref()
                .map(|cell| cell.body().to_string());
            self.flush_active_cell_to_transcript();
        } else {
            self.clear_active_cell();
        }
    }

    pub(crate) fn handle_reasoning_item_started(&mut self, item_id: &str, title: &str) {
        self.flush_active_cell_to_transcript();
        self.consolidate_exploration_stage();
        self.transcript_state.active_reasoning_item_id = Some(item_id.to_string());
        self.transcript_state.active_reasoning_title = Some(title.to_string());
        self.transcript_state.active_reasoning_text.clear();
    }

    pub(crate) fn handle_reasoning_item_completed(
        &mut self,
        item_id: &str,
        title: &str,
        output: &str,
    ) {
        if self.transcript_state.active_reasoning_item_id.as_deref() != Some(item_id) {
            self.transcript_state.active_reasoning_item_id = Some(item_id.to_string());
            self.transcript_state.active_reasoning_title = Some(title.to_string());
            self.transcript_state.active_reasoning_text.clear();
        }
        self.transcript_state.active_reasoning_text.clear();
        self.transcript_state.active_reasoning_text.push_str(output);
        self.flush_reasoning_buffer_to_transcript();
    }

    pub(crate) fn handle_reasoning_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_reasoning_item_id.as_deref() == Some(item_id) {
            self.transcript_state.active_reasoning_text.push_str(delta);
        }
    }

    pub(crate) fn handle_control_item_started(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
    ) {
        self.flush_reasoning_buffer_to_transcript();
        if let Some(detail) = classify_exploration_start(&kind, title) {
            self.transcript_state.active_item_id = Some(item_id.to_string());
            self.transcript_state.active_item_kind = Some(kind);
            self.start_or_update_exploration_session(item_id, detail);
            return;
        }
        if self.transcript_state.active_exec.is_some() {
            self.clear_active_cell();
        } else {
            self.flush_active_cell_to_transcript();
        }
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(kind.clone());
        self.start_command_session(item_id, kind, title);
    }

    pub(crate) fn handle_control_item_completed(&mut self, item_id: &str, cell: HistoryCell) {
        self.transcript_state.active_item_id = None;
        self.transcript_state.active_item_kind = None;
        if cell.kind() == HistoryKind::Exploration {
            self.absorb_exploration_completion(item_id, cell);
            return;
        }
        if self.complete_command_session(item_id, &cell) {
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
                .active_cell
                .as_ref()
                .map(HistoryCell::kind),
            Some(crate::ui::widgets::history_cell::HistoryKind::Command)
                | Some(crate::ui::widgets::history_cell::HistoryKind::Exploration)
                | Some(crate::ui::widgets::history_cell::HistoryKind::Notice)
        ) {
            self.clear_active_cell();
        }
        self.push_cell(cell);
        let _ = item_id;
    }

    pub(crate) fn handle_control_item_delta(&mut self, item_id: &str, delta: &str) {
        let Some(active_id) = self.transcript_state.active_item_id.as_deref() else {
            return;
        };
        if active_id != item_id {
            return;
        }
        if self.append_active_exec_delta(item_id, delta) {
            return;
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.append_body(delta);
        }
    }

    pub(crate) fn flush_reasoning_buffer_to_transcript(&mut self) {
        if self
            .transcript_state
            .active_reasoning_text
            .trim()
            .is_empty()
        {
            self.transcript_state.active_reasoning_item_id = None;
            self.transcript_state.active_reasoning_title = None;
            self.transcript_state.active_reasoning_text.clear();
            return;
        }
        let title = self
            .transcript_state
            .active_reasoning_title
            .clone()
            .unwrap_or_else(|| "Reasoning".to_string());
        self.push_cell(HistoryCell::reasoning(
            title,
            self.transcript_state.active_reasoning_text.clone(),
        ));
        self.transcript_state.active_reasoning_item_id = None;
        self.transcript_state.active_reasoning_title = None;
        self.transcript_state.active_reasoning_text.clear();
    }

    fn clear_active_cell(&mut self) {
        self.transcript_state.active_item_id = None;
        self.transcript_state.active_item_kind = None;
        self.transcript_state.active_exec = None;
        self.transcript_state.active_cell = None;
    }

    pub(crate) fn flush_active_cell_to_transcript(&mut self) {
        let Some(cell) = self.transcript_state.active_cell.take() else {
            self.clear_active_cell();
            return;
        };
        if !cell.body().trim().is_empty() {
            self.push_cell(cell);
        }
        self.clear_active_cell();
    }

    fn start_or_update_exploration_session(&mut self, item_id: &str, detail: String) {
        let call = ActiveExecCall {
            route_key: route_key_for_item(item_id),
            label: "Exploring workspace".to_string(),
            summary: detail,
            detail: String::new(),
            completed: false,
        };
        match self.transcript_state.active_exec.as_mut() {
            Some(session) if session.is_exploration() => {
                session.append_call(call);
            }
            _ => {
                self.transcript_state.active_exec = Some(ActiveExecSession::new_exploration(call));
            }
        }
        self.refresh_active_exec_cell();
    }

    fn absorb_exploration_completion(&mut self, item_id: &str, cell: HistoryCell) {
        let Some(completed) = cell.aggregate().cloned() else {
            return;
        };
        match self.transcript_state.active_exec.as_mut() {
            Some(session) => {
                if let Some(aggregate) = session.exploration_aggregate_mut() {
                    *aggregate = merge_aggregate(aggregate, &completed);
                    let route_key = route_key_for_item(item_id);
                    let _ = session.complete_call(&route_key);
                } else {
                    self.transcript_state.active_exec = Some(ActiveExecSession {
                        mode: ActiveExecMode::Exploration { aggregate: completed },
                        calls: Vec::new(),
                    });
                }
            }
            None => {
                self.transcript_state.active_exec = Some(ActiveExecSession {
                    mode: ActiveExecMode::Exploration {
                        aggregate: completed,
                    },
                    calls: Vec::new(),
                });
            }
        }
        self.refresh_active_exec_cell();
    }

    fn refresh_active_exec_cell(&mut self) {
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
                let mut cell = HistoryCell::exploration(
                    if has_pending {
                        "Exploring workspace"
                    } else {
                        "Explored workspace"
                    },
                    summary,
                    combined,
                    HistoryTone::Control,
                );
                cell.expanded = self.run_state.expand_tool_details;
                self.transcript_state.active_cell = Some(cell);
            }
            ActiveExecMode::Command => {
                let Some(call) = active.last_call() else {
                    return;
                };
                let detail = (!call.detail.trim().is_empty()).then(|| call.detail.clone());
                let mut cell = HistoryCell::exec(
                    call.label.clone(),
                    call.summary.clone(),
                    detail.or_else(|| Some("running".to_string())),
                    HistoryTone::Control,
                );
                cell.expanded = self.run_state.expand_tool_details;
                self.transcript_state.active_cell = Some(cell);
            }
        }
    }

    fn start_command_session(&mut self, item_id: &str, kind: TurnItemKind, title: &str) {
        let placeholder = render_active_control_placeholder(kind, title);
        self.transcript_state.active_exec = Some(ActiveExecSession::new_command(ActiveExecCall {
                route_key: route_key_for_item(item_id),
                label: placeholder.label().to_string(),
                summary: placeholder.body().to_string(),
                detail: String::new(),
                completed: false,
            }));
        self.refresh_active_exec_cell();
    }

    fn complete_command_session(&mut self, item_id: &str, cell: &HistoryCell) -> bool {
        let Some(active) = self.transcript_state.active_exec.as_ref() else {
            return false;
        };
        if !matches!(active.mode, ActiveExecMode::Command) {
            return false;
        }
        let route_key = route_key_for_item(item_id);
        if !active.contains_call(&route_key) {
            return false;
        }
        self.transcript_state.active_exec = None;
        if cell.kind() != crate::ui::widgets::history_cell::HistoryKind::Exploration {
            self.consolidate_exploration_stage();
        }
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        self.transcript_state.active_cell = None;
        self.push_cell(cell.clone());
        true
    }

    fn append_active_exec_delta(&mut self, item_id: &str, delta: &str) -> bool {
        let Some(session) = self.transcript_state.active_exec.as_mut() else {
            return false;
        };
        let route_key = route_key_for_item(item_id);
        if !session.append_delta(&route_key, delta) {
            return false;
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

fn route_key_for_item(item_id: &str) -> ActiveExecRouteKey {
    ActiveExecRouteKey::ItemId(item_id.to_string())
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

fn merge_aggregate(
    left: &ExplorationAggregate,
    right: &ExplorationAggregate,
) -> ExplorationAggregate {
    let mut details = left.details.clone();
    details.extend(right.details.clone());
    ExplorationAggregate {
        read_files: left.read_files + right.read_files,
        searches: left.searches + right.searches,
        inspect_commands: left.inspect_commands + right.inspect_commands,
        listed_directories: left.listed_directories + right.listed_directories,
        metadata_reads: left.metadata_reads + right.metadata_reads,
        details,
    }
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
