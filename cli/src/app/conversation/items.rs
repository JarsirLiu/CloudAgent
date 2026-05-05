use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat, HistoryTone};
use agent_protocol::TurnItemKind;

impl TuiApp {
    pub(crate) fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        let _ = turn_id;
        self.consolidate_exploration_stage();
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(TurnItemKind::AssistantMessage);
        self.transcript_state.active_cell = Some(HistoryCell::agent(
            next_response_label(self),
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
                next_response_label(self),
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
        self.handle_secondary_item_started(
            item_id,
            TurnItemKind::Reasoning,
            title,
            HistoryTone::Reasoning,
        );
    }

    pub(crate) fn handle_reasoning_item_completed(
        &mut self,
        item_id: &str,
        title: &str,
        output: &str,
    ) {
        self.handle_secondary_item_completed(
            item_id,
            TurnItemKind::Reasoning,
            title,
            output,
            HistoryTone::Reasoning,
        );
    }

    pub(crate) fn handle_reasoning_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_item_kind == Some(TurnItemKind::Reasoning) {
            self.append_active_secondary_item_delta(item_id, delta);
        }
    }

    pub(crate) fn handle_control_item_started(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        _title: &str,
    ) {
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(kind.clone());
    }

    pub(crate) fn handle_control_item_completed(&mut self, _item_id: &str, cell: HistoryCell) {
        self.transcript_state.active_item_id = None;
        self.transcript_state.active_item_kind = None;
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        self.push_cell(cell);
    }

    pub(crate) fn handle_control_item_delta(&mut self, _item_id: &str, _delta: &str) {}

    fn handle_secondary_item_started(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
        tone: HistoryTone,
    ) {
        self.consolidate_exploration_stage();
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(kind.clone());
        self.transcript_state.active_cell = Some(make_secondary_cell(kind, title, tone));
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.expanded = self.run_state.expand_tool_details;
        }
    }

    fn handle_secondary_item_completed(
        &mut self,
        item_id: &str,
        kind: TurnItemKind,
        title: &str,
        output: &str,
        tone: HistoryTone,
    ) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id) {
            self.flush_active_cell_to_transcript();
            self.transcript_state.active_item_id = Some(item_id.to_string());
            self.transcript_state.active_item_kind = Some(kind.clone());
            self.transcript_state.active_cell = Some(make_secondary_cell(kind, title, tone));
            if let Some(cell) = self.transcript_state.active_cell.as_mut() {
                cell.expanded = self.run_state.expand_tool_details;
            }
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.replace_body(output);
        }
        if self.transcript_state.active_item_id.as_deref() == Some(item_id) {
            self.flush_active_cell_to_transcript();
        }
    }

    fn append_active_secondary_item_delta(&mut self, item_id: &str, delta: &str) {
        if self.transcript_state.active_item_id.as_deref() != Some(item_id) {
            return;
        }
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            if !cell.body().is_empty() {
                cell.append_body("\n");
            }
            cell.append_body(delta);
        }
    }

    fn clear_active_cell(&mut self) {
        self.transcript_state.active_item_id = None;
        self.transcript_state.active_item_kind = None;
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

fn next_response_label(app: &TuiApp) -> String {
    let completed = app
        .history_cells()
        .iter()
        .filter(|cell| cell.tone == HistoryTone::Agent)
        .count();
    format!("Response {}", completed + 1)
}

fn make_secondary_cell(kind: TurnItemKind, title: &str, tone: HistoryTone) -> HistoryCell {
    match kind {
        TurnItemKind::Reasoning => HistoryCell::reasoning(title.to_string(), String::new()),
        _ => HistoryCell::info(title.to_string(), String::new(), tone),
    }
}
