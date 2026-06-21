use std::time::Instant;

use crate::runtime_metrics_display::format_runtime_metrics;
use crate::state::NoticeLevel;
use crate::ui::history_cell::humanize_tool_label;
use agent_core::{
    ModelRetryStage, RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress, TurnItemKind,
};

const TRANSIENT_NOTICE_TTL_SECS: u64 = 4;

#[derive(Clone, Debug)]
pub(crate) struct TransientNotice {
    pub(crate) message: String,
    pub(crate) level: NoticeLevel,
    pub(crate) expires_at: Instant,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BottomPaneRuntimeState {
    pub(crate) active_tool: Option<ActiveToolRuntimeState>,
    pub(crate) live_label: Option<String>,
    pub(crate) transient_notice: Option<TransientNotice>,
    pub(crate) turn_active: bool,
    pub(crate) turn_started_at: Option<Instant>,
}

impl BottomPaneRuntimeState {
    pub(crate) fn reset(&mut self) {
        self.active_tool = None;
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
        self.active_tool = None;
    }

    pub(crate) fn on_tool_finished_for_item(&mut self, item_id: Option<&str>) {
        let should_clear = match self.active_tool.as_ref() {
            Some(ActiveToolRuntimeState::Tool(tool)) => tool.matches_item(item_id),
            _ => false,
        };
        if should_clear {
            self.active_tool = None;
        }
        if self.live_label.is_none() {
            self.live_label = Some("Working".to_string());
        }
    }

    pub(crate) fn on_context_compaction_started(&mut self, estimated_tokens: u64) {
        self.active_tool = None;
        self.live_label = Some(format!(
            "Compacting context (~{} tokens)",
            compact_number(estimated_tokens)
        ));
    }

    pub(crate) fn on_context_compaction_finished(&mut self) {
        self.active_tool = None;
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

    pub(crate) fn on_active_item_started(&mut self, item: &RuntimeItem) {
        match item.kind {
            TurnItemKind::AssistantMessage => {
                self.active_tool = None;
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::Reasoning => {
                self.active_tool = None;
                self.live_label = Some("Thinking".to_string());
            }
            TurnItemKind::CommandExecution => {
                self.active_tool = Some(ActiveToolRuntimeState::Command(
                    CommandRuntimeState::started(item),
                ));
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::ToolCall => {
                self.active_tool = Some(ActiveToolRuntimeState::Tool(
                    ToolRuntimeState::tool_call_started(item),
                ));
                self.live_label = Some("Working".to_string());
            }
            TurnItemKind::ToolResult => {
                self.active_tool = Some(ActiveToolRuntimeState::Tool(
                    ToolRuntimeState::tool_result_started(item),
                ));
                self.live_label = Some("Working".to_string());
            }
            _ => {
                self.active_tool = None;
            }
        }
    }

    pub(crate) fn on_command_output_delta(&mut self, item_id: Option<&str>, delta: &str) {
        if let Some(ActiveToolRuntimeState::Command(command)) = self.active_tool.as_mut() {
            command.append_output(item_id, delta);
        }
    }

    pub(crate) fn on_command_finished(&mut self, item_id: &str) {
        if self.active_tool.as_ref().is_some_and(
            |tool| matches!(tool, ActiveToolRuntimeState::Command(command) if command.item_id == item_id),
        ) {
            self.active_tool = None;
        }
    }

    pub(crate) fn on_tool_output_delta(&mut self, item_id: Option<&str>, delta: &str) {
        if let Some(ActiveToolRuntimeState::Tool(tool)) = self.active_tool.as_mut() {
            tool.append_output(item_id, delta);
        }
    }

    pub(crate) fn on_item_progress(
        &mut self,
        item_id: Option<&str>,
        progress: &RuntimeItemProgress,
    ) {
        match self.active_tool.as_mut() {
            Some(ActiveToolRuntimeState::Command(command)) => {
                command.update_progress(item_id, progress)
            }
            Some(ActiveToolRuntimeState::Tool(tool)) => tool.update_progress(item_id, progress),
            None => {}
        }
    }

    pub(crate) fn on_item_metrics_updated(
        &mut self,
        item_id: Option<&str>,
        metrics: &RuntimeItemMetrics,
    ) {
        match self.active_tool.as_mut() {
            Some(ActiveToolRuntimeState::Command(command)) => {
                command.update_metrics(item_id, metrics)
            }
            Some(ActiveToolRuntimeState::Tool(tool)) => tool.update_metrics(item_id, metrics),
            None => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn set_live_label_for_test(&mut self, label: Option<String>) {
        self.live_label = label;
    }

    #[cfg(test)]
    pub(crate) fn set_active_tool_title_for_test(&mut self, title: Option<String>) {
        self.active_tool = title
            .map(ToolRuntimeState::static_banner)
            .map(ActiveToolRuntimeState::Tool);
    }

    #[cfg(test)]
    pub(crate) fn expire_transient_notice_for_test(&mut self) {
        if let Some(notice) = self.transient_notice.as_mut() {
            notice.expires_at = Instant::now();
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ActiveToolRuntimeState {
    Command(CommandRuntimeState),
    Tool(ToolRuntimeState),
}

impl ActiveToolRuntimeState {
    pub(crate) fn banner_text(&self) -> String {
        match self {
            Self::Command(command) => command.banner_text(),
            Self::Tool(tool) => tool.banner_text(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ToolRuntimeState {
    pub(crate) item_id: Option<String>,
    banner: String,
    output: Option<String>,
    metrics: Option<String>,
}

impl ToolRuntimeState {
    fn tool_call_started(item: &RuntimeItem) -> Self {
        let banner = match item
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(tool) => format!("executing tool: {}", humanize_tool_label(tool)),
            None => "executing tool".to_string(),
        };
        let mut state = Self {
            item_id: Some(item.id.clone()),
            banner,
            output: None,
            metrics: None,
        };
        state.update_progress(
            Some(&item.id),
            &item
                .progress
                .clone()
                .unwrap_or_else(|| RuntimeItemProgress {
                    message: item.summary.clone(),
                    completed: None,
                    total: None,
                    unit: None,
                }),
        );
        if let Some(metrics) = item.metrics.as_ref() {
            state.update_metrics(Some(&item.id), metrics);
        }
        state
    }

    fn tool_result_started(item: &RuntimeItem) -> Self {
        let banner = item
            .title
            .as_deref()
            .map(humanize_tool_label)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Tool".to_string());
        let mut state = Self {
            item_id: Some(item.id.clone()),
            banner,
            output: None,
            metrics: None,
        };
        state.update_progress(
            Some(&item.id),
            &RuntimeItemProgress {
                message: item
                    .progress
                    .as_ref()
                    .and_then(|progress| progress.message.clone())
                    .or_else(|| item.summary.clone()),
                completed: None,
                total: None,
                unit: None,
            },
        );
        if let Some(metrics) = item.metrics.as_ref() {
            state.update_metrics(Some(&item.id), metrics);
        }
        state
    }

    #[cfg(test)]
    fn static_banner(banner: String) -> Self {
        Self {
            item_id: None,
            banner,
            output: None,
            metrics: None,
        }
    }

    fn matches_item(&self, item_id: Option<&str>) -> bool {
        match (self.item_id.as_deref(), item_id) {
            (_, None) => true,
            (Some(active), Some(item_id)) => active == item_id,
            (None, Some(_)) => false,
        }
    }

    fn append_output(&mut self, item_id: Option<&str>, delta: &str) {
        if !self.matches_item(item_id) {
            return;
        }
        let delta = delta.trim();
        if delta.is_empty() {
            return;
        }
        let compact = compact_recent_output(delta, 120);
        if self.output.as_deref() == Some(compact.as_str()) {
            return;
        }
        self.output = Some(match self.output.take() {
            Some(previous) if !previous.trim().is_empty() => {
                compact_recent_output(&format!("{previous} {compact}"), 120)
            }
            _ => compact,
        });
    }

    fn update_progress(&mut self, item_id: Option<&str>, progress: &RuntimeItemProgress) {
        if !self.matches_item(item_id) {
            return;
        }
        let Some(message) = progress.message.as_deref() else {
            return;
        };
        let message = message.trim();
        if message.is_empty() {
            return;
        }
        self.output = Some(compact_recent_output(message, 120));
    }

    fn update_metrics(&mut self, item_id: Option<&str>, metrics: &RuntimeItemMetrics) {
        if !self.matches_item(item_id) {
            return;
        }
        self.metrics = format_runtime_metrics(metrics);
    }

    fn banner_text(&self) -> String {
        let mut parts = vec![self.banner.clone()];
        if let Some(output) = self
            .output
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(output.to_string());
        }
        if let Some(metrics) = self
            .metrics
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(metrics.to_string());
        }
        parts.join(" · ")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CommandRuntimeState {
    pub(crate) item_id: String,
    pub(crate) title: String,
    pub(crate) recent_output: Option<String>,
    pub(crate) metrics: Option<String>,
}

impl CommandRuntimeState {
    fn started(item: &RuntimeItem) -> Self {
        let title = match item
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(command) => format!("running command: {command}"),
            None => "running command".to_string(),
        };
        let mut state = Self {
            item_id: item.id.clone(),
            title,
            recent_output: None,
            metrics: None,
        };
        state.update_progress(
            Some(&item.id),
            &RuntimeItemProgress {
                message: item.summary.clone(),
                completed: None,
                total: None,
                unit: None,
            },
        );
        if let Some(metrics) = item.metrics.as_ref() {
            state.update_metrics(Some(&item.id), metrics);
        }
        state
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
        if self.recent_output.as_deref() == Some(compact.as_str()) {
            return;
        }
        self.recent_output = Some(match self.recent_output.take() {
            Some(previous) if !previous.trim().is_empty() => {
                compact_recent_output(&format!("{previous} {compact}"), 120)
            }
            _ => compact,
        });
    }

    pub(crate) fn banner_text(&self) -> String {
        let mut parts = vec![self.title.clone()];
        if let Some(output) = self
            .recent_output
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(output.to_string());
        }
        if let Some(metrics) = self
            .metrics
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(metrics.to_string());
        }
        parts.join(" · ")
    }

    fn update_progress(&mut self, item_id: Option<&str>, progress: &RuntimeItemProgress) {
        if let Some(item_id) = item_id
            && item_id != self.item_id
        {
            return;
        }
        let Some(message) = progress.message.as_deref() else {
            return;
        };
        let message = message.trim();
        if message.is_empty() {
            return;
        }
        self.recent_output = Some(compact_recent_output(message, 120));
    }

    fn update_metrics(&mut self, item_id: Option<&str>, metrics: &RuntimeItemMetrics) {
        if let Some(item_id) = item_id
            && item_id != self.item_id
        {
            return;
        }
        self.metrics = format_runtime_metrics(metrics);
    }
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
