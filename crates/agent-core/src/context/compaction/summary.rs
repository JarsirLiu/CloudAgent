use crate::conversation::{ResponseItem, input_items_are_blank, text_input_items};
use crate::model::ModelRequest;

use super::support::{estimate_text_tokens, render_input_items_for_compaction, single_line};
use super::{ContextCompactionConfig, ContextCompactionPlan};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompactionSummary {
    pub current_task: Vec<String>,
    pub progress: Vec<String>,
    pub key_decisions: Vec<String>,
    pub important_context: Vec<String>,
    pub tool_code_facts: Vec<String>,
    pub next_steps: Vec<String>,
}

pub fn build_compaction_summary_request(
    plan: &ContextCompactionPlan,
    config: ContextCompactionConfig,
    temperature: f32,
) -> ModelRequest {
    let rendered_prefix = render_compaction_source(&plan.prefix);
    let prompt = format!(
        "{}\n{}",
        concat!(
            "You are summarizing earlier conversation context for a coding agent handoff.\n",
            "Produce a compact, factual summary using exactly these sections and headings:\n",
            "Current Task:\n",
            "Progress:\n",
            "Key Decisions:\n",
            "Important Context:\n",
            "Tool / Code Facts:\n",
            "Next Steps:\n\n",
            "Rules:\n",
            "- Keep it concise and high signal.\n",
            "- Preserve concrete file paths, constraints, decisions, errors, and unfinished work.\n",
            "- Do not add information that is not supported by the source messages.\n",
            "- Prefer bullets under each heading.\n",
            "- Do not include markdown fences.\n\n",
            "Source messages to summarize:\n"
        ),
        truncate_summary_source(&rendered_prefix, config.summary_source_max_tokens.max(1))
    );

    ModelRequest {
        messages: vec![
            ResponseItem::System {
                content: "You create structured context-compaction summaries for agent handoff."
                    .to_string(),
            },
            ResponseItem::User {
                content: text_input_items(prompt),
            },
        ],
        tools: Vec::new(),
        temperature,
        reasoning_effort: None,
        tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
    }
}

impl CompactionSummary {
    pub fn fallback_from_plan(plan: &ContextCompactionPlan) -> Self {
        let current_task = latest_user_message(&plan.prefix)
            .map(|text| vec![single_line(&text)])
            .unwrap_or_else(|| vec!["Continue the active coding task.".to_string()]);
        let progress = collect_prefix_lines(&plan.prefix, "Progress", 4);
        let key_decisions = collect_decision_lines(&plan.prefix);
        let important_context = collect_context_lines(&plan.prefix);
        let tool_code_facts = collect_tool_lines(&plan.prefix);
        let next_steps = vec!["Continue from the preserved recent conversation tail.".to_string()];

        Self {
            current_task,
            progress,
            key_decisions,
            important_context,
            tool_code_facts,
            next_steps,
        }
    }

    pub fn from_model_output(output: &str) -> Self {
        let mut summary = Self {
            current_task: Vec::new(),
            progress: Vec::new(),
            key_decisions: Vec::new(),
            important_context: Vec::new(),
            tool_code_facts: Vec::new(),
            next_steps: Vec::new(),
        };

        let mut current_section: Option<&str> = None;
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed == "[Context Summary]" {
                continue;
            }

            current_section = match trimmed {
                "Current Task:" => Some("current_task"),
                "Progress:" => Some("progress"),
                "Key Decisions:" => Some("key_decisions"),
                "Important Context:" => Some("important_context"),
                "Tool / Code Facts:" => Some("tool_code_facts"),
                "Next Steps:" => Some("next_steps"),
                _ => current_section,
            };

            if trimmed.ends_with(':') {
                continue;
            }

            let value = trimmed.trim_start_matches("- ").to_string();
            if value.is_empty() {
                continue;
            }

            match current_section {
                Some("current_task") => summary.current_task.push(value),
                Some("progress") => summary.progress.push(value),
                Some("key_decisions") => summary.key_decisions.push(value),
                Some("important_context") => summary.important_context.push(value),
                Some("tool_code_facts") => summary.tool_code_facts.push(value),
                Some("next_steps") => summary.next_steps.push(value),
                _ => {}
            }
        }

        summary
    }

    pub fn ensure_defaults(mut self) -> Self {
        if self.current_task.is_empty() {
            self.current_task
                .push("Continue the active coding task.".to_string());
        }
        if self.progress.is_empty() {
            self.progress
                .push("Earlier conversation context was compacted.".to_string());
        }
        if self.key_decisions.is_empty() {
            self.key_decisions.push(
                "Preserve the system prompt and the recent raw conversation tail.".to_string(),
            );
        }
        if self.important_context.is_empty() {
            self.important_context
                .push("Treat the preserved tail as the authoritative recent context.".to_string());
        }
        if self.tool_code_facts.is_empty() {
            self.tool_code_facts.push(
                "No additional tool or code facts were retained from the compacted prefix."
                    .to_string(),
            );
        }
        if self.next_steps.is_empty() {
            self.next_steps.push(
                "Continue from the preserved tail without re-expanding compacted context."
                    .to_string(),
            );
        }
        self
    }

    pub fn rendered(&self) -> String {
        normalize_summary(format!(
            "Current Task:\n{}\n\nProgress:\n{}\n\nKey Decisions:\n{}\n\nImportant Context:\n{}\n\nTool / Code Facts:\n{}\n\nNext Steps:\n{}",
            render_bullets(&self.current_task),
            render_bullets(&self.progress),
            render_bullets(&self.key_decisions),
            render_bullets(&self.important_context),
            render_bullets(&self.tool_code_facts),
            render_bullets(&self.next_steps),
        ))
    }
}

fn normalize_summary(summary: String) -> String {
    let trimmed = summary.trim().replace("\r\n", "\n");
    if trimmed.starts_with("[Context Summary]") {
        trimmed
    } else {
        format!("[Context Summary]\n\n{trimmed}")
    }
}

fn render_bullets(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_compaction_source(prefix: &[ResponseItem]) -> String {
    let mut lines = Vec::new();
    for item in prefix {
        match item {
            ResponseItem::System { content } => {
                lines.push(format!("SYSTEM: {}", single_line(content)))
            }
            ResponseItem::User { content } => lines.push(format!(
                "USER: {}",
                single_line(&render_input_items_for_compaction(content))
            )),
            ResponseItem::Assistant {
                content,
                tool_calls,
                ..
            } => {
                if let Some(content) = content
                    && !content.trim().is_empty()
                {
                    lines.push(format!("ASSISTANT: {}", single_line(content)));
                }
                for call in tool_calls {
                    lines.push(format!(
                        "ASSISTANT_TOOL_CALL: {} {}",
                        call.name,
                        single_line(&call.arguments.to_string())
                    ));
                }
            }
            ResponseItem::Tool { name, content, .. } => {
                lines.push(format!("TOOL_RESULT({name}): {}", single_line(content)))
            }
        }
    }

    if lines.is_empty() {
        "No prior context available.".to_string()
    } else {
        lines.join("\n")
    }
}

fn truncate_summary_source(source: &str, max_tokens: usize) -> String {
    if estimate_text_tokens(source) <= max_tokens {
        return source.to_string();
    }

    let mut kept = Vec::new();
    let mut remaining = max_tokens;
    for line in source.lines().rev() {
        let line_tokens = estimate_text_tokens(line).max(1);
        if line_tokens > remaining && !kept.is_empty() {
            break;
        }
        if line_tokens > remaining {
            kept.push(truncate_text_tokens(line, remaining.max(1)));
            break;
        }
        kept.push(line.to_string());
        remaining = remaining.saturating_sub(line_tokens);
    }
    kept.reverse();
    kept.join("\n")
}

fn truncate_text_tokens(text: &str, token_budget: usize) -> String {
    let target_chars = token_budget.saturating_mul(3).max(1);
    let mut chars = text.chars();
    let snippet = chars.by_ref().take(target_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}...")
    } else {
        snippet
    }
}

fn latest_user_message(prefix: &[ResponseItem]) -> Option<String> {
    prefix.iter().rev().find_map(|item| match item {
        ResponseItem::User { content } if !input_items_are_blank(content) => {
            Some(render_input_items_for_compaction(content))
        }
        _ => None,
    })
}

fn collect_prefix_lines(prefix: &[ResponseItem], label: &str, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for item in prefix {
        match item {
            ResponseItem::User { content } => {
                lines.push(format!(
                    "{label}: {}",
                    single_line(&render_input_items_for_compaction(content))
                ));
            }
            ResponseItem::Assistant {
                content: Some(content),
                ..
            } if !content.trim().is_empty() => {
                lines.push(format!("{label}: {}", single_line(content)));
            }
            _ => {}
        }
    }
    dedupe_limit(lines, limit)
}

fn collect_decision_lines(prefix: &[ResponseItem]) -> Vec<String> {
    let mut lines = Vec::new();
    for text in prefix.iter().filter_map(|item| match item {
        ResponseItem::System { content } => Some(content.clone()),
        ResponseItem::User { content } => Some(render_input_items_for_compaction(content)),
        ResponseItem::Assistant {
            content: Some(content),
            ..
        } => Some(content.clone()),
        _ => None,
    }) {
        let lower = text.to_ascii_lowercase();
        if lower.contains("should")
            || lower.contains("must")
            || lower.contains("keep")
            || lower.contains("move")
            || lower.contains("use")
            || lower.contains("belongs")
        {
            lines.push(single_line(&text));
        }
    }
    dedupe_limit(lines, 4)
}

fn collect_context_lines(prefix: &[ResponseItem]) -> Vec<String> {
    let mut lines = Vec::new();
    for text in prefix.iter().filter_map(|item| match item {
        ResponseItem::System { content } => Some(content.clone()),
        ResponseItem::User { content } => Some(render_input_items_for_compaction(content)),
        ResponseItem::Assistant {
            content: Some(content),
            ..
        } => Some(content.clone()),
        _ => None,
    }) {
        if text.contains("crates/")
            || text.contains("src/")
            || text.contains('\\')
            || text.contains("context")
            || text.contains("history")
            || text.contains("turn")
        {
            lines.push(single_line(&text));
        }
    }
    dedupe_limit(lines, 5)
}

fn collect_tool_lines(prefix: &[ResponseItem]) -> Vec<String> {
    let mut lines = Vec::new();
    for item in prefix {
        match item {
            ResponseItem::Assistant { tool_calls, .. } => {
                for call in tool_calls {
                    lines.push(format!("Tool invoked: {}", call.name));
                }
            }
            ResponseItem::Tool { name, content, .. } => {
                lines.push(format!("{name}: {}", single_line(content)));
            }
            _ => {}
        }
    }
    dedupe_limit(lines, 6)
}

fn dedupe_limit(lines: Vec<String>, limit: usize) -> Vec<String> {
    let mut deduped = Vec::new();
    for line in lines {
        if !deduped.contains(&line) {
            deduped.push(line);
        }
        if deduped.len() >= limit {
            break;
        }
    }
    deduped
}
