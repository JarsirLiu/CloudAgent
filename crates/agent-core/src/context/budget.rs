use crate::context::facade::ContextFacade;
use crate::conversation::ResponseItem;
use crate::tool::ToolSpec;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct MemoryBudgetSource {
    pub memory: Option<String>,
    pub skills: Option<String>,
    pub mcp: Option<String>,
    pub enable_skills_bucket: bool,
    pub enable_mcp_bucket: bool,
    pub post_compact_budget_tokens: usize,
    pub post_compact_memory_floor_tokens: usize,
    pub post_compact_skills_budget_tokens: usize,
    pub post_compact_mcp_budget_tokens: usize,
    pub post_compact_max_tokens_per_memory: usize,
    pub post_compact_max_tokens_per_skill: usize,
    pub post_compact_max_tokens_per_mcp: usize,
    pub safety_buffer_tokens: usize,
}

#[derive(Clone, Debug, Default)]
pub struct BucketAudit {
    pub memory_before: usize,
    pub memory_after: usize,
    pub skills_before: usize,
    pub skills_after: usize,
    pub mcp_before: usize,
    pub mcp_after: usize,
    pub hard_cap_triggered: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BudgetedFragments {
    pub fragments: Vec<ResponseItem>,
    pub audit: BucketAudit,
}

pub fn build_memory_budgeted_fragments(
    facade: &ContextFacade,
    history: &[ResponseItem],
    environment_fragment: ResponseItem,
    tool_specs: &[ToolSpec],
    workspace_root: &Path,
    model_context_window: u64,
    trigger_ratio: f32,
    configured_overhead_tokens: usize,
    source: MemoryBudgetSource,
) -> BudgetedFragments {
    let mut fragments = vec![environment_fragment.clone()];
    let mut audit = BucketAudit {
        memory_before: estimate_text_tokens(source.memory.as_deref().unwrap_or("")),
        skills_before: estimate_text_tokens(source.skills.as_deref().unwrap_or("")),
        mcp_before: estimate_text_tokens(source.mcp.as_deref().unwrap_or("")),
        ..BucketAudit::default()
    };
    let history_tokens = facade.estimate_history_tokens_for_compaction(history, workspace_root);
    let overhead_tokens = facade.estimate_request_overhead_tokens(
        history,
        &environment_fragment,
        tool_specs,
        configured_overhead_tokens,
    );
    let trigger_tokens = ((model_context_window as f32) * trigger_ratio) as usize;
    let available_tokens = trigger_tokens
        .saturating_sub(history_tokens)
        .saturating_sub(overhead_tokens)
        .saturating_sub(source.safety_buffer_tokens)
        .saturating_sub(512);
    if available_tokens < 64 {
        return BudgetedFragments { fragments, audit };
    }

    let mut remaining = available_tokens.min(source.post_compact_budget_tokens.max(1));
    let memory_floor_cap = source
        .post_compact_max_tokens_per_memory
        .min(remaining)
        .max(32);
    if let Some(memory) = fit_bucket(source.memory.as_deref(), memory_floor_cap) {
        fragments.push(ResponseItem::User {
            content: format!("<long_term_memory>\n{}\n</long_term_memory>", memory.0),
        });
        let used = estimate_text_tokens(&memory.0).max(1);
        audit.memory_after = used;
        remaining = remaining.saturating_sub(used);
    } else if let Some(memory) = fit_bucket(
        source.memory.as_deref(),
        source.post_compact_memory_floor_tokens.min(remaining).max(32),
    ) {
        fragments.push(ResponseItem::User {
            content: format!("<long_term_memory>\n{}\n</long_term_memory>", memory.0),
        });
        let used = estimate_text_tokens(&memory.0).max(1);
        audit.memory_after = used;
        remaining = remaining.saturating_sub(used);
    }
    if source.enable_skills_bucket {
        let skill_budget = remaining
            .min(source.post_compact_skills_budget_tokens)
            .min(source.post_compact_max_tokens_per_skill);
        if let Some(skills) = fit_bucket(source.skills.as_deref(), skill_budget) {
            fragments.push(ResponseItem::User {
                content: format!("<skills_context>\n{}\n</skills_context>", skills.0),
            });
            let used = estimate_text_tokens(&skills.0).max(1);
            audit.skills_after = used;
            remaining = remaining.saturating_sub(used);
        }
    }
    if source.enable_mcp_bucket {
        let mcp_budget = remaining
            .min(source.post_compact_mcp_budget_tokens)
            .min(source.post_compact_max_tokens_per_mcp);
        if let Some(mcp) = fit_bucket(source.mcp.as_deref(), mcp_budget) {
            fragments.push(ResponseItem::User {
                content: format!("<mcp_context>\n{}\n</mcp_context>", mcp.0),
            });
            audit.mcp_after = estimate_text_tokens(&mcp.0).max(1);
        }
    }
    audit.hard_cap_triggered = audit.memory_after < audit.memory_before
        || audit.skills_after < audit.skills_before
        || audit.mcp_after < audit.mcp_before;
    BudgetedFragments { fragments, audit }
}

fn fit_bucket(text: Option<&str>, remaining_tokens: usize) -> Option<(String, usize)> {
    let text = text?.trim();
    if text.is_empty() || remaining_tokens < 32 {
        return None;
    }
    let token_budget = remaining_tokens;
    let char_budget = token_budget.saturating_mul(3).min(text.len());
    let trimmed = text.chars().take(char_budget).collect::<String>();
    if trimmed.trim().is_empty() {
        None
    } else {
        Some((trimmed, token_budget))
    }
}

fn estimate_text_tokens(text: &str) -> usize {
    text.chars().count().saturating_div(3).max(1)
}
