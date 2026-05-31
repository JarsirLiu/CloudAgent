use std::time::Instant;

use crate::state::NoticeLevel;
use agent_core::{ModelRetryStage, TurnItemKind};

const TRANSIENT_NOTICE_TTL_SECS: u64 = 4;

#[derive(Clone, Debug)]
pub(crate) struct TransientNotice {
    pub(crate) message: String,
    pub(crate) level: NoticeLevel,
    pub(crate) expires_at: Instant,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BottomPaneRuntimeState {
    pub(crate) active_tool_title: Option<String>,
    pub(crate) active_command: Option<CommandRuntimeState>,
    pub(crate) live_label: Option<String>,
    pub(crate) transient_notice: Option<TransientNotice>,
    pub(crate) turn_active: bool,
    pub(crate) turn_started_at: Option<Instant>,
}

impl BottomPaneRuntimeState {
    pub(crate) fn reset(&mut self) {
        self.active_tool_title = None;
        self.active_command = None;
        self.live_label = None;
        self.transient_notice = None;
        self.turn_active = false;
        self.turn_started_at = None;
    }

    pub(crate) fn handle_tick(&mut self) -> bool {
        if self
            .transient_notice
            .as_ref()
            .is_some_and(|notice| Instant::now() >= notice.expires_at)
        {
            self.transient_notice = None;
            return true;
        }
        false
    }

    pub(crate) fn show_transient_notice(&mut self, level: NoticeLevel, message: String) {
        self.transient_notice = Some(TransientNotice {
            message,
            level,
            expires_at: Instant::now() + std::time::Duration::from_secs(TRANSIENT_NOTICE_TTL_SECS),
        });
    }

    pub(crate) fn on_turn_started(&mut self) {
        self.turn_active = true;
        self.turn_started_at = Some(Instant::now());
        self.live_label = Some("Working".to_string());
        self.active_tool_title = None;
    }

    pub(crate) fn on_tool_finished(&mut self) {
        self.active_tool_title = None;
        self.active_command = None;
        if self.live_label.is_none() {
            self.live_label = Some("Working".to_string());
        }
    }

    pub(crate) fn on_context_compaction_started(&mut self, estimated_tokens: u64) {
        self.active_tool_title = None;
        self.active_command = None;
        self.live_label = Some(format!(
            "Compacting context (~{} tokens)",
            compact_number(estimated_tokens)
        ));
    }

    pub(crate) fn on_context_compaction_finished(&mut self) {
        self.active_tool_title = None;
        self.active_command = None;
        if self.turn_active {
            self.live_label = Some("Working".to_string());
        } else {
            self.live_label = None;
        }
    }

    pub(crate) fn on_turn_finished(&mut self) {
        self.reset();
    }

    pub(crate) fn sync_frontend_mode(&mut self, mode: agent_protocol::FrontendMode) {
        match mode {
            agent_protocol::FrontendMode::Idle => self.reset(),
            agent_protocol::FrontendMode::Running => {
                if !self.turn_active {
                    self.turn_active = true;
                    self.live_label.get_or_insert_with(|| "Working".to_string());
                }
            }
            agent_protocol::FrontendMode::WaitingForServerRequest => {
                if !self.turn_active {
                    self.turn_active = true;
                }
                self.live_label.get_or_insert_with(|| "Working".to_string());
            }
        }
    }

    pub(crate) fn on_model_retrying(
        &mut self,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    ) {
        let seconds = (next_delay_ms as f64) / 1000.0;
        let stage_label = match stage {
            ModelRetryStage::Request => "request",
            ModelRetryStage::Streaming => "stream",
        };
        self.live_label = Some(format!(
            "reconnecting ({stage_label} retry {attempt}, next in {seconds:.1}s)"
        ));
    }

    pub(crate) fn on_active_item_started(
        &mut self,
        item_id: &str,
        kind: &TurnItemKind,
        title: Option<&str>,
    ) {
        match kind {
            TurnItemKind::AssistantMessage => {
                self.active_tool_title = None;
                self.active_command = None;
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::Reasoning => {
                self.active_tool_title = None;
                self.active_command = None;
                self.live_label = Some("Thinking".to_string());
            }
            TurnItemKind::CommandExecution => {
                self.active_tool_title = None;
                self.active_command = Some(CommandRuntimeState::started(item_id, title));
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::ToolCall => {
                self.active_tool_title =
                    Some(match title.map(str::trim).filter(|s| !s.is_empty()) {
                        Some(tool) => format!("executing tool: {}", humanize_runtime_title(tool)),
                        None => "executing tool".to_string(),
                    });
                self.active_command = None;
                self.live_label = Some("Working".to_string());
            }
            _ => {
                self.active_tool_title =
                    title.map(humanize_runtime_title).filter(|s| !s.is_empty());
                self.active_command = None;
            }
        }
    }

    pub(crate) fn on_command_output_delta(&mut self, item_id: Option<&str>, delta: &str) {
        if let Some(command) = self.active_command.as_mut() {
            command.append_output(item_id, delta);
        }
    }

    pub(crate) fn on_command_finished(&mut self, item_id: &str) {
        if self
            .active_command
            .as_ref()
            .is_some_and(|command| command.item_id == item_id)
        {
            self.active_command = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn set_live_label_for_test(&mut self, label: Option<String>) {
        self.live_label = label;
    }

    #[cfg(test)]
    pub(crate) fn set_active_tool_title_for_test(&mut self, title: Option<String>) {
        self.active_tool_title = title;
    }

    #[cfg(test)]
    pub(crate) fn expire_transient_notice_for_test(&mut self) {
        if let Some(notice) = self.transient_notice.as_mut() {
            notice.expires_at = Instant::now();
        }
    }
}

fn humanize_runtime_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return String::new();
    }
    title
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CommandRuntimeState {
    pub(crate) item_id: String,
    pub(crate) title: String,
    pub(crate) recent_output: Option<String>,
}

impl CommandRuntimeState {
    fn started(item_id: &str, title: Option<&str>) -> Self {
        let title = match title.map(str::trim).filter(|value| !value.is_empty()) {
            Some(command) => format!("running command: {command}"),
            None => "running command".to_string(),
        };
        Self {
            item_id: item_id.to_string(),
            title,
            recent_output: None,
        }
    }

    fn append_output(&mut self, item_id: Option<&str>, delta: &str) {
        if let Some(item_id) = item_id
            && item_id != self.item_id
        {
            return;
        }
        let delta = delta.trim();
        if delta.is_empty() {
            return;
        }
        let compact = compact_recent_output(delta, 120);
        self.recent_output = Some(match self.recent_output.take() {
            Some(previous) if !previous.trim().is_empty() => {
                compact_recent_output(&format!("{previous} {compact}"), 120)
            }
            _ => compact,
        });
    }

    pub(crate) fn banner_text(&self) -> String {
        match self
            .recent_output
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(output) => format!("{} · {output}", self.title),
            None => self.title.clone(),
        }
    }
}

fn compact_recent_output(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let keep = max_chars.saturating_sub(1);
    let mut out = normalized.chars().rev().take(keep).collect::<Vec<_>>();
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}
