use crate::conversation::{ResponseItem, text_input_items};
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
