use crate::context::{
    BudgetedFragments, MemoryBudgetSource, build_memory_budgeted_fragments,
    CompactionSummary, ContextCompactionConfig, ContextCompactionPlan, ContextCompactionResult,
    ContextInputFilterService, ContextManager, FilterPolicy, apply_history_compaction,
    build_compaction_summary_request, plan_manual_history_compaction,
};
use crate::conversation::{ConversationHistory, ResponseItem};
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
    pub ephemeral: bool,
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
        policy: FilterPolicy,
        _workspace_root: &Path,
    ) -> Vec<ResponseItem> {
        self.input_filter.filter_for_model(messages, policy)
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

    pub fn estimate_history_tokens(&self, messages: &[ResponseItem]) -> usize {
        estimate_history_tokens(messages)
    }

    pub fn estimate_history_tokens_for_compaction(
        &self,
        messages: &[ResponseItem],
        policy: FilterPolicy,
        _workspace_root: &Path,
    ) -> usize {
        let filtered = self.input_filter.filter_for_model(messages.to_vec(), policy);
        estimate_history_tokens(&filtered)
    }

    pub fn estimate_history_tokens_for_canonical_compaction(
        &self,
        messages: &[ResponseItem],
        workspace_root: &Path,
    ) -> usize {
        self.estimate_history_tokens_for_compaction(
            messages,
            Self::canonical_compaction_filter_policy(),
            workspace_root,
        )
    }

    pub fn filtered_messages_for_canonical_compaction(
        &self,
        messages: &[ResponseItem],
    ) -> Vec<ResponseItem> {
        self.input_filter.filter_for_model(
            messages.to_vec(),
            Self::canonical_compaction_filter_policy(),
        )
    }

    pub fn estimate_request_overhead_tokens(
        &self,
        history_messages: &[ResponseItem],
        environment_fragment: &ResponseItem,
        tool_specs: &[ToolSpec],
        minimum_overhead_tokens: usize,
    ) -> usize {
        let system_tokens = history_messages
            .first()
            .map(|item| estimate_history_tokens(std::slice::from_ref(item)))
            .unwrap_or(0);
        let environment_tokens =
            estimate_history_tokens(std::slice::from_ref(environment_fragment));
        let tool_tokens = tool_specs
            .iter()
            .map(|tool| {
                tool.name.chars().count()
                    + tool.description.chars().count()
                    + tool.parameters.to_string().chars().count()
                    + 64
            })
            .sum::<usize>()
            .saturating_div(3)
            .max(1);

        minimum_overhead_tokens.max(
            system_tokens
                .saturating_add(environment_tokens)
                .saturating_add(tool_tokens)
                .saturating_add(2_000),
        )
    }

    pub fn build_memory_budgeted_fragments(
        &self,
        history: &[ResponseItem],
        filter_policy: FilterPolicy,
        environment_fragment: ResponseItem,
        tool_specs: &[ToolSpec],
        workspace_root: &Path,
        model_context_window: u64,
        trigger_ratio: f32,
        configured_overhead_tokens: usize,
        source: MemoryBudgetSource,
    ) -> BudgetedFragments {
        build_memory_budgeted_fragments(
            self,
            history,
            filter_policy,
            environment_fragment,
            tool_specs,
            workspace_root,
            model_context_window,
            trigger_ratio,
            configured_overhead_tokens,
            source,
        )
    }

    pub async fn prepare_model_request<F, Fut>(
        &self,
        context_manager: &mut ContextManager,
        workspace_root: &Path,
        filter_policy: FilterPolicy,
        fragments: Vec<ResponseItem>,
        tools: Vec<ToolSpec>,
        temperature: f32,
        compaction_config: ContextCompactionConfig,
        estimated_total_tokens: usize,
        memory_floor_tokens: usize,
        safety_buffer_tokens: usize,
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
            .saturating_sub(memory_floor_tokens)
            .saturating_sub(safety_buffer_tokens)
            .max(1);
        let compaction_requested = estimated_total_tokens > available_history_tokens;

        let mut prepared_messages_override: Option<Vec<ResponseItem>> = None;
        let compaction = if compaction_requested {
            let raw_messages = &context_manager.history().messages;
            let filtered_messages = self.filtered_messages_for_canonical_compaction(raw_messages);
            let filtered_plan =
                self.plan_manual_compaction(&filtered_messages, compaction_config, 1);
            let raw_plan = self.plan_manual_compaction(raw_messages, compaction_config, 1);
            if let (Some(filtered_plan), Some(raw_plan)) = (filtered_plan, raw_plan)
            {
                let pre_message_count = context_manager.history().messages.len();
                let pre_context_tokens_estimate =
                    estimate_history_tokens(&context_manager.history().messages) as u64;
                let summary_request = self.build_compaction_summary_request(
                    &filtered_plan,
                    compaction_config,
                    temperature,
                );
                let summary_text = summarize_compaction(summary_request)
                    .await?
                    .unwrap_or_default();
                let summary = CompactionSummary::from_model_output(&summary_text)
                    .ensure_defaults();
                if filter_policy.enabled {
                    let mut filtered_history = filtered_messages.clone();
                    let compacted =
                        self.apply_compaction(&mut filtered_history, &filtered_plan, summary);
                    let post_message_count = compacted.replacement_history.len();
                    let post_context_tokens_estimate =
                        estimate_history_tokens(&compacted.replacement_history) as u64;
                    let rendered_summary = compacted.summary.rendered();
                    let temp_history = ConversationHistory {
                        id: context_manager.history().id.clone(),
                        turn_count: context_manager.history().turn_count,
                        messages: compacted.replacement_history.clone(),
                    };
                    let temp_manager = ContextManager::from_history(temp_history);
                    prepared_messages_override = Some(
                        temp_manager
                            .build_current_model_request_with_rendered_fragments(
                                &fragments,
                                tools.clone(),
                                temperature,
                            )
                            .messages,
                    );
                    Some(PreparedCompaction {
                        summary: compacted.summary,
                        rendered_summary,
                        replacement_history: compacted.replacement_history,
                        pre_context_tokens_estimate,
                        post_context_tokens_estimate,
                        pre_message_count,
                        post_message_count,
                        ephemeral: true,
                    })
                } else {
                    let compacted = self.apply_compaction(
                        &mut context_manager.history_mut().messages,
                        &raw_plan,
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
                        ephemeral: false,
                    })
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut model_request = if let Some(messages) = prepared_messages_override {
            ModelRequest {
                messages,
                tools,
                temperature,
            }
        } else {
            context_manager.build_current_model_request_with_rendered_fragments(
                &fragments,
                tools,
                temperature,
            )
        };
        if !compaction
            .as_ref()
            .is_some_and(|compaction| compaction.ephemeral)
        {
            model_request.messages =
                self.apply_pre_llm_filter(model_request.messages, filter_policy, workspace_root);
        }

        Ok(PreparedModelRequest {
            model_request,
            compaction_requested,
            compaction,
        })
    }

    fn canonical_compaction_filter_policy() -> FilterPolicy {
        FilterPolicy { enabled: true }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{CommandExecutionStatus, StructuredToolResult};

    #[test]
    fn apply_pre_llm_filter_respects_policy_flag() {
        let facade = ContextFacade::new();
        let messages = vec![ResponseItem::Tool {
            tool_call_id: "call-1".to_string(),
            name: "shell_command".to_string(),
            content: "raw".to_string(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "git status".to_string(),
                current_directory: "D:\\repo".to_string(),
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                stdout: Some("modified: a.rs\nnew file: b.rs".to_string()),
                stderr: None,
                aggregated_output: None,
                duration_ms: Some(1),
            }),
        }];

        let filtered = facade.apply_pre_llm_filter(
            messages.clone(),
            FilterPolicy { enabled: true },
            Path::new("D:\\repo"),
        );
        let unfiltered = facade.apply_pre_llm_filter(
            messages,
            FilterPolicy { enabled: false },
            Path::new("D:\\repo"),
        );

        match &filtered[0] {
            ResponseItem::Tool { content, .. } => assert!(content.starts_with("[rtk:git]")),
            _ => panic!("expected tool message"),
        }
        match &unfiltered[0] {
            ResponseItem::Tool { content, .. } => assert_eq!(content, "raw"),
            _ => panic!("expected tool message"),
        }
    }

    #[test]
    fn estimate_history_tokens_for_compaction_respects_policy_flag() {
        let facade = ContextFacade::new();
        let messages = vec![ResponseItem::Tool {
            tool_call_id: "call-1".to_string(),
            name: "shell_command".to_string(),
            content: "raw".to_string(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "git status".to_string(),
                current_directory: "D:\\repo".to_string(),
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                stdout: Some("modified: a.rs\nnew file: b.rs".to_string()),
                stderr: None,
                aggregated_output: None,
                duration_ms: Some(1),
            }),
        }];

        let filtered = facade.estimate_history_tokens_for_compaction(
            &messages,
            FilterPolicy { enabled: true },
            Path::new("D:\\repo"),
        );
        let unfiltered = facade.estimate_history_tokens_for_compaction(
            &messages,
            FilterPolicy { enabled: false },
            Path::new("D:\\repo"),
        );

        assert!(filtered > 0);
        assert!(unfiltered > 0);
        assert_ne!(filtered, unfiltered);
    }
}
