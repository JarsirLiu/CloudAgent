use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{
    HistoryCell, HistoryFormat, render_active_control_placeholder,
};
use agent_protocol::TurnItemKind;

impl TuiApp {
    pub(crate) fn handle_assistant_item_started(&mut self, turn_id: &str, item_id: &str) {
        let _ = turn_id;
        self.flush_reasoning_buffer_to_transcript();
        self.consolidate_exploration_stage();
        self.flush_active_cell_to_transcript();
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(TurnItemKind::AssistantMessage);
        self.transcript_state.active_cell =
            Some(HistoryCell::agent("", String::new(), HistoryFormat::Markdown));
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
            self.transcript_state.active_cell =
                Some(HistoryCell::agent("", String::new(), HistoryFormat::Markdown));
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
        self.consolidate_exploration_stage();
        self.flush_active_cell_to_transcript();
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
        if matches!(
            self.transcript_state.active_item_kind,
            Some(TurnItemKind::CommandExecution | TurnItemKind::ToolCall | TurnItemKind::FileChange)
        ) {
            self.clear_active_cell();
        } else {
            self.flush_active_cell_to_transcript();
        }
        self.transcript_state.active_item_id = Some(item_id.to_string());
        self.transcript_state.active_item_kind = Some(kind.clone());
        self.transcript_state.active_cell = Some(render_active_control_placeholder(kind, title));
        if let Some(cell) = self.transcript_state.active_cell.as_mut() {
            cell.expanded = self.run_state.expand_tool_details;
        }
    }

    pub(crate) fn handle_control_item_completed(&mut self, item_id: &str, cell: HistoryCell) {
        self.transcript_state.active_item_id = None;
        self.transcript_state.active_item_kind = None;
        if cell.kind() != crate::ui::widgets::history_cell::HistoryKind::Exploration {
            self.consolidate_exploration_stage();
        }
        self.run_state.set_system_notice(
            format!("Completed {}", compact_activity(cell.label())),
            Some(std::time::Duration::from_secs(4)),
        );
        if matches!(
            self.transcript_state.active_cell.as_ref().map(HistoryCell::kind),
            Some(crate::ui::widgets::history_cell::HistoryKind::Command)
                | Some(crate::ui::widgets::history_cell::HistoryKind::Exploration)
                | Some(crate::ui::widgets::history_cell::HistoryKind::Notice)
        ) {
            self.clear_active_cell();
        }
        self.push_cell(cell);
        let _ = item_id;
    }

    pub(crate) fn handle_control_item_delta(&mut self, _item_id: &str, _delta: &str) {}

    pub(crate) fn flush_reasoning_buffer_to_transcript(&mut self) {
        if self.transcript_state.active_reasoning_text.trim().is_empty() {
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
