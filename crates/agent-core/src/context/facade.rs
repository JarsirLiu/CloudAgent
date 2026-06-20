use crate::context::{
    BudgetedFragments, CompactionSummary, ContextBudgetSource, ContextCompactionConfig,
    ContextCompactionPlan, ContextCompactionResult, ContextInputFilterService, ContextManager,
    FilterPolicy, apply_history_compaction, build_compaction_summary_request,
    build_context_budgeted_fragments, plan_manual_history_compaction,
};
use crate::conversation::ResponseItem;
use crate::model::ModelRequest;
use crate::tool::ToolSpec;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ContextFacade {
    input_filter: ContextInputFilterService,
}

#[derive(Clone, Debug)]
pub struct PreparedModelRequest {
    pub model_request: ModelRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalModelRequestBudget {
    pub estimated_tokens: usize,
    pub limit_tokens: usize,
    pub exceeded: bool,
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
        let filtered = self
            .input_filter
            .filter_for_model(messages.to_vec(), policy);
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

    pub fn estimate_model_request_tokens(&self, request: &ModelRequest) -> usize {
        let message_tokens = estimate_history_tokens(&request.messages);
        let tool_tokens = estimate_tool_spec_tokens(&request.tools);
        message_tokens
            .saturating_add(tool_tokens)
            .saturating_add(estimate_protocol_overhead_tokens())
    }

    pub fn check_final_model_request_budget(
        &self,
        request: &ModelRequest,
        model_context_window: usize,
        safety_buffer_tokens: usize,
    ) -> FinalModelRequestBudget {
        let estimated_tokens = self.estimate_model_request_tokens(request);
        let limit_tokens = model_context_window
            .saturating_sub(safety_buffer_tokens)
            .max(1);
        FinalModelRequestBudget {
            estimated_tokens,
            limit_tokens,
            exceeded: estimated_tokens > limit_tokens,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_context_budgeted_fragments(
        &self,
        history: &[ResponseItem],
        filter_policy: FilterPolicy,
        environment_fragment: ResponseItem,
        workspace_root: &Path,
        model_context_window: u64,
        trigger_ratio: f32,
        source: ContextBudgetSource,
    ) -> BudgetedFragments {
        build_context_budgeted_fragments(
            self,
            history,
            filter_policy,
            environment_fragment,
            workspace_root,
            model_context_window,
            trigger_ratio,
            source,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_model_request(
        &self,
        context_manager: &ContextManager,
        workspace_root: &Path,
        filter_policy: FilterPolicy,
        fragments: Vec<ResponseItem>,
        tools: Vec<ToolSpec>,
        temperature: f32,
    ) -> PreparedModelRequest {
        let mut model_request = context_manager
            .build_current_model_request_with_rendered_fragments(&fragments, tools, temperature);
        model_request.messages =
            self.apply_pre_llm_filter(model_request.messages, filter_policy, workspace_root);

        PreparedModelRequest { model_request }
    }

    fn canonical_compaction_filter_policy() -> FilterPolicy {
        FilterPolicy { enabled: true }
    }
}

fn estimate_history_tokens(messages: &[ResponseItem]) -> usize {
    messages
        .iter()
        .map(|item| match item {
            ResponseItem::System { content } => content.chars().count(),
            ResponseItem::User { content } => crate::input_items_text_len(content),
            ResponseItem::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let text_len = content.as_ref().map_or(0, |text| text.chars().count());
                let tool_len: usize = tool_calls
                    .iter()
                    .map(|call| {
                        call.name.chars().count() + call.arguments.to_string().chars().count()
                    })
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

fn estimate_tool_spec_tokens(tools: &[ToolSpec]) -> usize {
    tools
        .iter()
        .map(|tool| {
            tool.name.chars().count()
                + tool.description.chars().count()
                + tool.parameters.to_string().chars().count()
                + 64
        })
        .sum::<usize>()
        .saturating_div(3)
        .max(1)
}

fn estimate_protocol_overhead_tokens() -> usize {
    256
}

#[cfg(test)]
#[path = "facade_tests.rs"]
mod tests;
