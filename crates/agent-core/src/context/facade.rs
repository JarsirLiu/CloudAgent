use crate::context::{
    CompactionSummary, ContextCompactionConfig, ContextCompactionPlan, ContextCompactionResult,
    ContextInputFilterService, ContextManager, FilterPolicy, apply_history_compaction,
    build_compaction_summary_request, plan_manual_history_compaction,
};
use crate::conversation::ResponseItem;
use crate::model::ModelRequest;
use crate::tool::ToolSpec;
use anyhow::Result;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ContextFacade {
    input_filter: ContextInputFilterService,
}

#[derive(Clone, Debug)]
pub struct PreparedCompaction {
    pub summary: CompactionSummary,
    pub rendered_summary: String,
    pub replacement_history: Vec<ResponseItem>,
    pub pre_context_tokens_estimate: u64,
    pub post_context_tokens_estimate: u64,
    pub pre_message_count: usize,
    pub post_message_count: usize,
}

#[derive(Clone, Debug)]
pub struct PreparedModelRequest {
    pub model_request: ModelRequest,
    pub compaction_requested: bool,
    pub compaction: Option<PreparedCompaction>,
}

impl ContextFacade {
    pub fn new() -> Self {
        Self {
            input_filter: ContextInputFilterService::new(),
        }
    }

    pub fn apply_pre_llm_filter(
        &self,
        messages: Vec<ResponseItem>,
        workspace_root: &Path,
    ) -> Vec<ResponseItem> {
        let enabled = Self::read_pre_llm_filter_enabled(workspace_root);
        self.input_filter
            .filter_for_model(messages, FilterPolicy { enabled })
    }

    pub fn read_pre_llm_filter_enabled(workspace_root: &Path) -> bool {
        let path = workspace_root.join("data").join("ui-settings.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("pre_llm_filter_enabled").and_then(|b| b.as_bool()))
            .unwrap_or(false)
    }

    pub fn plan_manual_compaction(
        &self,
        messages: &[ResponseItem],
        config: ContextCompactionConfig,
        minimum_history_tokens: usize,
    ) -> Option<ContextCompactionPlan> {
        plan_manual_history_compaction(messages, config, minimum_history_tokens)
    }

    pub fn build_compaction_summary_request(
        &self,
        plan: &ContextCompactionPlan,
        config: ContextCompactionConfig,
        temperature: f32,
    ) -> ModelRequest {
        build_compaction_summary_request(plan, config, temperature)
    }

    pub fn apply_compaction(
        &self,
        messages: &mut Vec<ResponseItem>,
        plan: &ContextCompactionPlan,
        summary: CompactionSummary,
    ) -> ContextCompactionResult {
        apply_history_compaction(messages, plan, summary)
    }

    pub async fn prepare_model_request<F, Fut>(
        &self,
        context_manager: &mut ContextManager,
        workspace_root: &Path,
        fragments: Vec<ResponseItem>,
        tools: Vec<ToolSpec>,
        temperature: f32,
        compaction_config: ContextCompactionConfig,
        estimated_total_tokens: usize,
        summarize_compaction: F,
    ) -> Result<PreparedModelRequest>
    where
        F: FnOnce(ModelRequest) -> Fut,
        Fut: std::future::Future<Output = Result<Option<String>>>,
    {
        let trigger_tokens =
            ((compaction_config.model_context_window as f32) * compaction_config.trigger_ratio)
                as usize;
        let available_history_tokens = trigger_tokens
            .saturating_sub(compaction_config.request_overhead_tokens)
            .max(1);
        let compaction_requested = estimated_total_tokens > available_history_tokens;

        let compaction = if compaction_requested {
            if let Some(compaction_plan) =
                self.plan_manual_compaction(&context_manager.history().messages, compaction_config, 1)
            {
                let pre_message_count = context_manager.history().messages.len();
                let pre_context_tokens_estimate =
                    estimate_history_tokens(&context_manager.history().messages) as u64;
                let summary_request = self.build_compaction_summary_request(
                    &compaction_plan,
                    compaction_config,
                    temperature,
                );
                let summary_text = summarize_compaction(summary_request)
                    .await?
                    .unwrap_or_default();
                let summary = CompactionSummary::from_model_output(&summary_text)
                    .ensure_defaults();
                let compacted = self.apply_compaction(
                    &mut context_manager.history_mut().messages,
                    &compaction_plan,
                    summary,
                );
                let post_message_count = compacted.replacement_history.len();
                let post_context_tokens_estimate =
                    estimate_history_tokens(&compacted.replacement_history) as u64;
                let rendered_summary = compacted.summary.rendered();
                Some(PreparedCompaction {
                    summary: compacted.summary,
                    rendered_summary,
                    replacement_history: compacted.replacement_history,
                    pre_context_tokens_estimate,
                    post_context_tokens_estimate,
                    pre_message_count,
                    post_message_count,
                })
            } else {
                None
            }
        } else {
            None
        };

        let mut model_request = context_manager.build_current_model_request_with_rendered_fragments(
            &fragments,
            tools,
            temperature,
        );
        model_request.messages = self.apply_pre_llm_filter(model_request.messages, workspace_root);

        Ok(PreparedModelRequest {
            model_request,
            compaction_requested,
            compaction,
        })
    }
}

fn estimate_history_tokens(messages: &[ResponseItem]) -> usize {
    messages
        .iter()
        .map(|item| match item {
            ResponseItem::System { content } | ResponseItem::User { content } => content.chars().count(),
            ResponseItem::Assistant { content, tool_calls } => {
                let text_len = content.as_ref().map_or(0, |text| text.chars().count());
                let tool_len: usize = tool_calls
                    .iter()
                    .map(|call| call.name.chars().count() + call.arguments.to_string().chars().count())
                    .sum();
                text_len + tool_len
            }
            ResponseItem::Tool { name, content, .. } => {
                name.chars().count() + content.chars().count()
            }
        })
        .sum::<usize>()
        .saturating_div(3)
        .max(1)
}
