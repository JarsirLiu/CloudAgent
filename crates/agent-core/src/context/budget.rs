use crate::context::{FilterPolicy, facade::ContextFacade};
use crate::conversation::{ResponseItem, input_items_to_plain_text, text_input_items};
use crate::skill::{SkillDocument, render_skill_injection, render_truncated_skill_injection};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct SkillBudgetSource {
    pub summary: Option<String>,
    pub explicit_documents: Vec<SkillDocument>,
    pub enable_summary_bucket: bool,
    pub post_compact_budget_tokens: usize,
    pub max_tokens_per_item: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ContextBudgetSource {
    pub memory: Option<String>,
    pub skills: SkillBudgetSource,
    pub mcp: Option<String>,
    pub enable_mcp_bucket: bool,
    pub post_compact_budget_tokens: usize,
    pub post_compact_memory_floor_tokens: usize,
    pub post_compact_mcp_budget_tokens: usize,
    pub post_compact_max_tokens_per_memory: usize,
    pub post_compact_max_tokens_per_mcp: usize,
    pub safety_buffer_tokens: usize,
}

#[derive(Clone, Debug, Default)]
pub struct BucketAudit {
    pub memory_before: usize,
    pub memory_after: usize,
    pub skills_before: usize,
    pub skills_after: usize,
    pub skill_bucket: SkillBucketAudit,
    pub mcp_before: usize,
    pub mcp_after: usize,
    pub hard_cap_triggered: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SkillBucketAudit {
    pub before: usize,
    pub after: usize,
    pub truncated: bool,
    pub kept_items: usize,
}

#[derive(Clone, Debug, Default)]
pub struct BudgetedFragments {
    pub fragments: Vec<ResponseItem>,
    pub audit: BucketAudit,
}

#[allow(clippy::too_many_arguments)]
pub fn build_context_budgeted_fragments(
    facade: &ContextFacade,
    history: &[ResponseItem],
    filter_policy: FilterPolicy,
    environment_fragment: ResponseItem,
    workspace_root: &Path,
    model_context_window: u64,
    trigger_ratio: f32,
    source: ContextBudgetSource,
) -> BudgetedFragments {
    let mut fragments = vec![environment_fragment.clone()];
    let skill_before = estimate_skill_context_tokens(&source.skills);
    let mut audit = BucketAudit {
        memory_before: estimate_text_tokens(source.memory.as_deref().unwrap_or("")),
        skills_before: skill_before,
        skill_bucket: SkillBucketAudit {
            before: skill_before,
            ..SkillBucketAudit::default()
        },
        mcp_before: estimate_text_tokens(source.mcp.as_deref().unwrap_or("")),
        ..BucketAudit::default()
    };
    let history_tokens =
        facade.estimate_history_tokens_for_compaction(history, filter_policy, workspace_root);
    let trigger_tokens = ((model_context_window as f32) * trigger_ratio) as usize;
    let available_tokens = trigger_tokens
        .saturating_sub(history_tokens)
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
            content: text_input_items(format!(
                "<long_term_memory>\n{}\n</long_term_memory>",
                memory.0
            )),
        });
        let used = estimate_text_tokens(&memory.0).max(1);
        audit.memory_after = used;
        remaining = remaining.saturating_sub(used);
    } else if let Some(memory) = fit_bucket(
        source.memory.as_deref(),
        source
            .post_compact_memory_floor_tokens
            .min(remaining)
            .max(32),
    ) {
        fragments.push(ResponseItem::User {
            content: text_input_items(format!(
                "<long_term_memory>\n{}\n</long_term_memory>",
                memory.0
            )),
        });
        let used = estimate_text_tokens(&memory.0).max(1);
        audit.memory_after = used;
        remaining = remaining.saturating_sub(used);
    }
    let skill_budget = remaining.min(source.skills.post_compact_budget_tokens);
    let budgeted_skills = budget_skill_bucket(&source.skills, skill_budget);
    if !budgeted_skills.fragments.is_empty() {
        remaining = remaining.saturating_sub(budgeted_skills.audit.after);
        fragments.extend(budgeted_skills.fragments);
    }
    audit.skills_after = budgeted_skills.audit.after;
    audit.skill_bucket = budgeted_skills.audit;
    if source.enable_mcp_bucket {
        let mcp_budget = remaining
            .min(source.post_compact_mcp_budget_tokens)
            .min(source.post_compact_max_tokens_per_mcp);
        if let Some(mcp) = fit_bucket(source.mcp.as_deref(), mcp_budget) {
            fragments.push(ResponseItem::User {
                content: text_input_items(format!("<mcp_context>\n{}\n</mcp_context>", mcp.0)),
            });
            audit.mcp_after = estimate_text_tokens(&mcp.0).max(1);
        }
    }
    audit.hard_cap_triggered = audit.memory_after < audit.memory_before
        || audit.skills_after < audit.skills_before
        || audit.mcp_after < audit.mcp_before;
    BudgetedFragments { fragments, audit }
}

#[derive(Clone, Debug, Default)]
struct BudgetedSkillBucket {
    fragments: Vec<ResponseItem>,
    audit: SkillBucketAudit,
}

fn budget_skill_bucket(source: &SkillBudgetSource, remaining_tokens: usize) -> BudgetedSkillBucket {
    let before = estimate_skill_context_tokens(source);
    if remaining_tokens < 32 {
        return BudgetedSkillBucket {
            fragments: Vec::new(),
            audit: SkillBucketAudit {
                before,
                truncated: before > 0,
                ..SkillBucketAudit::default()
            },
        };
    }

    let mut fragments = Vec::new();
    let mut remaining = remaining_tokens;
    let mut used_tokens = 0usize;
    let max_tokens_per_item = source.max_tokens_per_item.min(remaining_tokens).max(32);

    if source.enable_summary_bucket
        && let Some(summary) = fit_bucket(
            source.summary.as_deref(),
            remaining.min(max_tokens_per_item),
        )
    {
        let item = ResponseItem::User {
            content: text_input_items(format!(
                "<skills_context>\n{}\n</skills_context>",
                summary.0
            )),
        };
        let item_tokens = estimate_response_item_tokens(&item).max(1);
        fragments.push(item);
        used_tokens = used_tokens.saturating_add(item_tokens);
        remaining = remaining.saturating_sub(item_tokens);
    }

    for document in &source.explicit_documents {
        if remaining < 32 {
            break;
        }
        let item_budget = remaining.min(max_tokens_per_item);
        let Some(item) = fit_skill_document_bucket(document, item_budget) else {
            continue;
        };
        let item_tokens = estimate_response_item_tokens(&item).max(1);
        fragments.push(item);
        used_tokens = used_tokens.saturating_add(item_tokens);
        remaining = remaining.saturating_sub(item_tokens);
    }

    let kept_items = fragments.len();
    BudgetedSkillBucket {
        fragments,
        audit: SkillBucketAudit {
            before,
            after: used_tokens,
            truncated: used_tokens < before,
            kept_items,
        },
    }
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

fn fit_skill_document_bucket(
    document: &SkillDocument,
    remaining_tokens: usize,
) -> Option<ResponseItem> {
    if remaining_tokens < 32 {
        return None;
    }

    let mut char_budget = remaining_tokens.saturating_mul(3);
    while char_budget > 0 {
        let item = render_truncated_skill_injection(document, char_budget);
        let item_tokens = estimate_response_item_tokens(&item);
        if item_tokens > 0 && item_tokens <= remaining_tokens {
            return Some(item);
        }
        if char_budget <= 16 {
            break;
        }
        char_budget = char_budget.saturating_sub(16);
    }

    None
}

fn estimate_text_tokens(text: &str) -> usize {
    text.chars().count().saturating_div(3).max(1)
}

fn estimate_response_item_tokens(item: &ResponseItem) -> usize {
    match item {
        ResponseItem::System { content } => estimate_text_tokens(content),
        ResponseItem::User { content } => estimate_text_tokens(&input_items_to_plain_text(content)),
        ResponseItem::Assistant {
            content,
            tool_calls,
            ..
        } => {
            let text = content.as_deref().unwrap_or_default();
            let tool_text = tool_calls
                .iter()
                .map(|call| format!("{}{}", call.name, call.arguments))
                .collect::<String>();
            estimate_text_tokens(&format!("{text}{tool_text}"))
        }
        ResponseItem::Tool { name, content, .. } => {
            estimate_text_tokens(&format!("{name}{content}"))
        }
    }
}

fn estimate_skill_context_tokens(source: &SkillBudgetSource) -> usize {
    let summary_tokens = source
        .summary
        .as_deref()
        .map(estimate_text_tokens)
        .unwrap_or_default();
    let explicit_tokens = source
        .explicit_documents
        .iter()
        .map(render_skill_injection)
        .map(|item| estimate_response_item_tokens(&item))
        .sum::<usize>();
    summary_tokens.saturating_add(explicit_tokens)
}
