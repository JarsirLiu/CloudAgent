use std::time::Instant;

use crate::runtime_metrics_display::format_runtime_metrics;
use crate::ui::history_cell::humanize_tool_label;
use agent_core::{
    ModelRetryStage, RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress, TurnItemKind,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct BottomPaneRuntimeState {
    active_runtime: Option<ActiveRuntimeState>,
    pub(crate) live_label: Option<String>,
    pub(crate) turn_active: bool,
    pub(crate) turn_started_at: Option<Instant>,
}

impl BottomPaneRuntimeState {
    pub(crate) fn reset(&mut self) {
        self.active_runtime = None;
        self.live_label = None;
        self.turn_active = false;
        self.turn_started_at = None;
    }

    pub(crate) fn handle_tick(&mut self) -> bool {
        false
    }

    pub(crate) fn on_turn_started(&mut self) {
        self.turn_active = true;
        self.turn_started_at = Some(Instant::now());
        self.live_label = Some("Working".to_string());
        self.active_runtime = None;
    }

    pub(crate) fn on_context_compaction_started(&mut self, estimated_tokens: u64) {
        self.active_runtime = None;
        self.live_label = Some(format!(
            "Compacting context (~{} tokens)",
            compact_number(estimated_tokens)
        ));
    }

    pub(crate) fn on_context_compaction_finished(&mut self) {
        self.active_runtime = None;
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
        let started = StartedItemState::from_item(item);
        self.active_runtime = started.active_runtime;
        self.live_label = started.live_label;
    }

    pub(crate) fn on_active_runtime_output_delta(&mut self, item_id: Option<&str>, delta: &str) {
        self.update_active_runtime(item_id, |runtime| runtime.append_output(item_id, delta));
    }

    pub(crate) fn on_item_progress(
        &mut self,
        item_id: Option<&str>,
        progress: &RuntimeItemProgress,
    ) {
        self.update_active_runtime(item_id, |runtime| {
            runtime.update_progress(item_id, progress)
        });
    }

    pub(crate) fn on_item_metrics_updated(
        &mut self,
        item_id: Option<&str>,
        metrics: &RuntimeItemMetrics,
    ) {
        self.update_active_runtime(item_id, |runtime| runtime.update_metrics(item_id, metrics));
    }

    pub(crate) fn display_banner_text(&self) -> Option<String> {
        if let Some(text) = self
            .active_runtime
            .as_ref()
            .map(ActiveRuntimeState::banner_text)
        {
            return Some(text);
        }
        let live_label = self.live_label.as_deref()?;
        let live_label = live_label.trim();
        if live_label.is_empty() || live_label.eq_ignore_ascii_case("working") {
            return None;
        }
        Some(live_label.to_string())
    }

    #[cfg(test)]
    pub(crate) fn set_live_label_for_test(&mut self, label: Option<String>) {
        self.live_label = label;
    }

    #[cfg(test)]
    pub(crate) fn set_active_runtime_banner_for_test(&mut self, title: Option<String>) {
        self.active_runtime = title.map(ActiveRuntimeState::static_banner);
    }

    pub(crate) fn on_active_runtime_finished(&mut self, item_id: Option<&str>) {
        let should_clear = self.active_runtime_matches(item_id) || item_id.is_none();
        if should_clear {
            self.active_runtime = None;
        }
    }

    fn active_runtime_matches(&self, item_id: Option<&str>) -> bool {
        self.active_runtime
            .as_ref()
            .is_some_and(|runtime| runtime.matches_item(item_id))
    }

    fn update_active_runtime(
        &mut self,
        item_id: Option<&str>,
        update: impl FnOnce(&mut ActiveRuntimeState),
    ) {
        let Some(runtime) = self.active_runtime.as_mut() else {
            return;
        };
        if runtime.matches_item(item_id) {
            update(runtime);
        }
    }
}

#[derive(Clone, Debug)]
struct ActiveRuntimeState {
    banner: RuntimeBannerState,
}

impl ActiveRuntimeState {
    fn from_started_item(item: &RuntimeItem) -> Option<Self> {
        let banner = match item.kind {
            TurnItemKind::CommandExecution => match item
                .title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(command) => format!("running command: {command}"),
                None => "running command".to_string(),
            },
            TurnItemKind::ToolCall | TurnItemKind::ToolResult => match item
                .title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(tool) => format!("executing tool: {}", humanize_tool_label(tool)),
                None => "executing tool".to_string(),
            },
            _ => return None,
        };
        let mut state = Self {
            banner: RuntimeBannerState::new(Some(item.id.clone()), banner),
        };
        let progress = item
            .progress
            .as_ref()
            .cloned()
            .unwrap_or_else(|| RuntimeItemProgress {
                message: item.summary.clone(),
                completed: None,
                total: None,
                unit: None,
            });
        state.update_progress(Some(&item.id), &progress);
        if let Some(metrics) = item.metrics.as_ref() {
            state.update_metrics(Some(&item.id), metrics);
        }
        Some(state)
    }

    #[cfg(test)]
    fn static_banner(banner: String) -> Self {
        Self {
            banner: RuntimeBannerState::new(None, banner),
        }
    }

    fn banner_text(&self) -> String {
        self.banner.banner_text()
    }

    fn matches_item(&self, item_id: Option<&str>) -> bool {
        self.banner.matches_item(item_id)
    }

    fn append_output(&mut self, item_id: Option<&str>, delta: &str) {
        self.banner.append_output(item_id, delta);
    }

    fn update_progress(&mut self, item_id: Option<&str>, progress: &RuntimeItemProgress) {
        self.banner.update_progress(item_id, progress);
    }

    fn update_metrics(&mut self, item_id: Option<&str>, metrics: &RuntimeItemMetrics) {
        self.banner.update_metrics(item_id, metrics);
    }
}

#[derive(Clone, Debug)]
struct StartedItemState {
    active_runtime: Option<ActiveRuntimeState>,
    live_label: Option<String>,
}

impl StartedItemState {
    fn from_item(item: &RuntimeItem) -> Self {
        match item.kind {
            TurnItemKind::AssistantMessage => Self {
                active_runtime: None,
                live_label: Some("Working".to_string()),
            },
            TurnItemKind::Reasoning => Self {
                active_runtime: None,
                live_label: Some("Thinking".to_string()),
            },
            TurnItemKind::CommandExecution | TurnItemKind::ToolCall | TurnItemKind::ToolResult => {
                Self {
                    active_runtime: ActiveRuntimeState::from_started_item(item),
                    live_label: Some("Working".to_string()),
                }
            }
            _ => Self {
                active_runtime: None,
                live_label: None,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct RuntimeBannerState {
    item_id: Option<String>,
    banner: String,
    output: Option<String>,
    metrics: Option<String>,
}

impl RuntimeBannerState {
    fn new(item_id: Option<String>, banner: String) -> Self {
        Self {
            item_id,
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
