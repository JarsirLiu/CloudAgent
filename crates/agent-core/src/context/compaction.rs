use crate::conversation::{
    AttachmentRef, ImageDetail, InputItem, ResponseItem, input_items_are_blank, text_input_items,
};
use crate::model::ModelRequest;

#[derive(Clone, Copy, Debug)]
pub struct ContextCompactionConfig {
    pub model_context_window: u64,
    pub trigger_ratio: f32,
    pub request_overhead_tokens: usize,
    pub compacted_target_tokens: usize,
    pub preserved_user_turns: usize,
    pub preserved_tail_tokens: usize,
    pub summary_source_max_tokens: usize,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionPlan {
    prefix: Vec<ResponseItem>,
    preserved_tail: Vec<ResponseItem>,
    compacted_target_tokens: usize,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionResult {
    pub summary: CompactionSummary,
    pub replacement_history: Vec<ResponseItem>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompactionSummary {
    pub current_task: Vec<String>,
    pub progress: Vec<String>,
    pub key_decisions: Vec<String>,
    pub important_context: Vec<String>,
    pub tool_code_facts: Vec<String>,
    pub next_steps: Vec<String>,
}

pub fn plan_history_compaction(
    messages: &[ResponseItem],
    config: ContextCompactionConfig,
) -> Option<ContextCompactionPlan> {
    let estimated = estimate_message_tokens(messages);
    if messages.len() <= 6 {
        return None;
    }
    let trigger_tokens = ((config.model_context_window as f32) * config.trigger_ratio) as usize;
    let available_history_tokens = trigger_tokens
        .saturating_sub(config.request_overhead_tokens)
        .max(1);
    if estimated <= available_history_tokens {
        return None;
    }

    build_compaction_plan(messages, config)
}

pub fn plan_manual_history_compaction(
    messages: &[ResponseItem],
    config: ContextCompactionConfig,
    minimum_history_tokens: usize,
) -> Option<ContextCompactionPlan> {
    if messages.len() <= 6 {
        return None;
    }

    let estimated = estimate_message_tokens(messages);
    if estimated < minimum_history_tokens.max(1) {
        return None;
    }

    build_compaction_plan(messages, config)
}

fn build_compaction_plan(
    messages: &[ResponseItem],
    config: ContextCompactionConfig,
) -> Option<ContextCompactionPlan> {
    let mut keep_start = choose_tail_start(
        messages,
        config.preserved_user_turns.max(1),
        config.preserved_tail_tokens.max(1),
    );
    keep_start = adjust_tail_start_for_tool_invariants(messages, keep_start);

    if keep_start <= 1 || keep_start >= messages.len() {
        keep_start =
            choose_tail_start_from_token_budget(messages, config.compacted_target_tokens.max(1));
        keep_start = adjust_tail_start_for_tool_invariants(messages, keep_start);
    }

    if keep_start <= 1 || keep_start >= messages.len() {
        return None;
    }

    let prefix = messages[1..keep_start].to_vec();
    if prefix.is_empty() {
        return None;
    }

    Some(ContextCompactionPlan {
        prefix,
        preserved_tail: messages[keep_start..].to_vec(),
        compacted_target_tokens: config.compacted_target_tokens.max(1),
    })
}

fn choose_tail_start(
    messages: &[ResponseItem],
    preserved_user_turns: usize,
    tail_budget: usize,
) -> usize {
    let candidate = find_recent_user_boundary(messages, preserved_user_turns).unwrap_or(1);
    if estimate_message_tokens(&messages[candidate..]) <= tail_budget {
        return candidate;
    }

    let mut remaining_turns = preserved_user_turns.saturating_sub(1);
    while remaining_turns > 0 {
        let fallback = find_recent_user_boundary(messages, remaining_turns).unwrap_or(candidate);
        if estimate_message_tokens(&messages[fallback..]) <= tail_budget {
            return fallback;
        }
        remaining_turns -= 1;
    }

    choose_tail_start_from_token_budget(messages, tail_budget)
}

fn find_recent_user_boundary(
    messages: &[ResponseItem],
    preserved_user_turns: usize,
) -> Option<usize> {
    let mut seen_users = 0usize;
    for index in (1..messages.len()).rev() {
        if matches!(messages[index], ResponseItem::User { .. }) {
            seen_users += 1;
            if seen_users == preserved_user_turns {
                return Some(index);
            }
        }
    }
    None
}

fn choose_tail_start_from_token_budget(messages: &[ResponseItem], target_limit: usize) -> usize {
    let mut keep_start = messages.len();
    let mut kept_tokens = 0usize;

    for index in (1..messages.len()).rev() {
        let item_tokens = estimate_message_tokens(std::slice::from_ref(&messages[index]));
        if kept_tokens.saturating_add(item_tokens) > target_limit && keep_start < messages.len() {
            break;
        }

        keep_start = index;
        kept_tokens = kept_tokens.saturating_add(item_tokens);
    }

    keep_start.max(1)
}

fn adjust_tail_start_for_tool_invariants(
    messages: &[ResponseItem],
    mut keep_start: usize,
) -> usize {
    loop {
        let mut changed = false;
        let mut missing_call_index = None;
        for item in &messages[keep_start..] {
            let ResponseItem::Tool { tool_call_id, .. } = item else {
                continue;
            };
            let call_index = find_matching_tool_call(messages, tool_call_id);
            if let Some(index) = call_index
                && index < keep_start
            {
                missing_call_index = Some(index);
                break;
            }
        }

        if let Some(index) = missing_call_index {
            keep_start = index;
            changed = true;
        }

        if !changed {
            break;
        }
    }

    keep_start
}

fn find_matching_tool_call(messages: &[ResponseItem], tool_call_id: &str) -> Option<usize> {
    for index in (1..messages.len()).rev() {
        let ResponseItem::Assistant { tool_calls, .. } = &messages[index] else {
            continue;
        };
        if tool_calls.iter().any(|call| call.id == tool_call_id) {
            return Some(index);
        }
    }
    None
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

pub fn apply_history_compaction(
    messages: &mut Vec<ResponseItem>,
    plan: &ContextCompactionPlan,
    summary: CompactionSummary,
) -> ContextCompactionResult {
    let system_prompt = messages[0].clone();
    let tail_budget = plan
        .compacted_target_tokens
        .saturating_sub(estimate_message_tokens(std::slice::from_ref(
            &system_prompt,
        )));
    let preserved_tail = trim_tail_to_total_budget(&plan.preserved_tail, tail_budget);
    let tail_tokens = estimate_message_tokens(&preserved_tail);
    let summary_budget = plan
        .compacted_target_tokens
        .saturating_sub(estimate_message_tokens(std::slice::from_ref(
            &system_prompt,
        )))
        .saturating_sub(tail_tokens)
        .max(1);
    let rendered_summary = truncate_summary_source(&summary.rendered(), summary_budget);

    let mut replacement_history = Vec::with_capacity(preserved_tail.len() + 2);
    replacement_history.push(system_prompt);
    replacement_history.push(ResponseItem::System {
        content: rendered_summary,
    });
    replacement_history.extend(preserved_tail);

    *messages = replacement_history.clone();

    ContextCompactionResult {
        summary,
        replacement_history,
    }
}

fn trim_tail_to_total_budget(tail: &[ResponseItem], tail_budget: usize) -> Vec<ResponseItem> {
    if tail.is_empty() || estimate_message_tokens(tail) <= tail_budget {
        return tail.to_vec();
    }

    let mut keep_start = choose_tail_start_from_token_budget_within_slice(tail, tail_budget.max(1));
    while keep_start < tail.len() {
        if !matches!(tail[keep_start], ResponseItem::Tool { .. }) {
            break;
        }
        keep_start += 1;
    }

    if keep_start >= tail.len() {
        Vec::new()
    } else {
        tail[keep_start..].to_vec()
    }
}

fn choose_tail_start_from_token_budget_within_slice(
    messages: &[ResponseItem],
    target_limit: usize,
) -> usize {
    let mut keep_start = messages.len();
    let mut kept_tokens = 0usize;

    for index in (0..messages.len()).rev() {
        let item_tokens = estimate_message_tokens(std::slice::from_ref(&messages[index]));
        if kept_tokens.saturating_add(item_tokens) > target_limit && keep_start < messages.len() {
            break;
        }

        keep_start = index;
        kept_tokens = kept_tokens.saturating_add(item_tokens);
    }

    keep_start.min(messages.len())
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

fn single_line(value: &str) -> String {
    let compact = value.replace(['\n', '\r'], " ");
    let trimmed = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = trimmed.chars();
    let snippet = chars.by_ref().take(220).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}...")
    } else {
        snippet
    }
}

fn estimate_text_tokens(text: &str) -> usize {
    text.chars().count().saturating_div(3).max(1)
}

fn estimate_message_tokens(messages: &[ResponseItem]) -> usize {
    let chars = messages
        .iter()
        .map(|item| match item {
            ResponseItem::System { content } => content.len(),
            ResponseItem::User { content } => render_input_items_for_compaction(content).len(),
            ResponseItem::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let text_len = content.as_ref().map_or(0, String::len);
                let tool_len: usize = tool_calls
                    .iter()
                    .map(|call| call.name.len() + call.arguments.to_string().len())
                    .sum();
                text_len + tool_len
            }
            ResponseItem::Tool { name, content, .. } => name.len() + content.len(),
        })
        .sum::<usize>();

    chars.saturating_div(3).max(1)
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

fn render_input_items_for_compaction(items: &[InputItem]) -> String {
    items
        .iter()
        .map(render_input_item_for_compaction)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_input_item_for_compaction(item: &InputItem) -> String {
    match item {
        InputItem::Text { text } => text.clone(),
        InputItem::Image {
            source,
            detail,
            alt,
        } => {
            let mut parts = vec!["[image".to_string()];
            if let Some(alt) = alt.as_ref().filter(|alt| !alt.trim().is_empty()) {
                parts.push(format!("alt={}", single_line(alt)));
            }
            if let Some(detail) = detail {
                parts.push(format!("detail={}", render_image_detail(detail)));
            }
            parts.push(format!("source={}", render_attachment_ref(source)));
            format!("{}]", parts.join(" "))
        }
        InputItem::File {
            source,
            mime_type,
            name,
        } => {
            let mut parts = vec!["[file".to_string()];
            if let Some(name) = name.as_ref().filter(|name| !name.trim().is_empty()) {
                parts.push(format!("name={}", single_line(name)));
            }
            if let Some(mime_type) = mime_type
                .as_ref()
                .filter(|mime_type| !mime_type.trim().is_empty())
            {
                parts.push(format!("mime={mime_type}"));
            }
            parts.push(format!("source={}", render_attachment_ref(source)));
            format!("{}]", parts.join(" "))
        }
        InputItem::Mention { name, path } => {
            format!("[mention @{name} path={}]", single_line(path))
        }
        InputItem::Skill { name, path } => format!("[skill ${name} path={}]", single_line(path)),
    }
}

fn render_attachment_ref(source: &AttachmentRef) -> String {
    match source {
        AttachmentRef::InlineDataUrl { data_url } => {
            let prefix = data_url.split(',').next().unwrap_or("data:unknown");
            format!("{prefix},...")
        }
        AttachmentRef::RemoteUrl { url } => single_line(url),
        AttachmentRef::HubAsset {
            asset_id,
            download_url,
        } => match download_url {
            Some(download_url) => format!("hub:{asset_id} ({})", single_line(download_url)),
            None => format!("hub:{asset_id}"),
        },
        AttachmentRef::LocalPath { path } => single_line(path),
    }
}

fn render_image_detail(detail: &ImageDetail) -> &'static str {
    match detail {
        ImageDetail::Auto => "auto",
        ImageDetail::Low => "low",
        ImageDetail::High => "high",
        ImageDetail::Original => "original",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{CommandExecutionStatus, StructuredToolResult, ToolCall};
    use crate::{
        AttachmentRef, ImageDetail, InputItem, input_items_to_plain_text, text_input_items,
    };
    use serde_json::json;

    #[test]
    fn plans_compaction_when_history_exceeds_trigger() {
        let mut messages = vec![ResponseItem::System {
            content: "system".to_string(),
        }];
        for i in 0..40 {
            messages.push(ResponseItem::User {
                content: text_input_items(format!("user line {i} {}", "x".repeat(50))),
            });
            messages.push(ResponseItem::Assistant {
                content: Some(format!("assistant line {i} {}", "y".repeat(50))),
                reasoning: None,
                tool_calls: Vec::new(),
            });
        }

        let plan = plan_history_compaction(
            &messages,
            ContextCompactionConfig {
                model_context_window: 2_048,
                trigger_ratio: 0.5,
                request_overhead_tokens: 128,
                compacted_target_tokens: 720,
                preserved_user_turns: 3,
                preserved_tail_tokens: 512,
                summary_source_max_tokens: 600,
            },
        )
        .expect("should compact");

        let request = build_compaction_summary_request(
            &plan,
            ContextCompactionConfig {
                model_context_window: 2_048,
                trigger_ratio: 0.5,
                request_overhead_tokens: 128,
                compacted_target_tokens: 720,
                preserved_user_turns: 3,
                preserved_tail_tokens: 512,
                summary_source_max_tokens: 600,
            },
            0.0,
        );
        assert_eq!(request.tools.len(), 0);
        assert!(matches!(request.messages[0], ResponseItem::System { .. }));
        assert!(matches!(request.messages[1], ResponseItem::User { .. }));
    }

    #[test]
    fn keeps_recent_messages_when_applying_compaction() {
        let mut messages = vec![ResponseItem::System {
            content: "system".to_string(),
        }];
        for i in 0..20 {
            messages.push(ResponseItem::User {
                content: text_input_items(format!("q{i} {}", "z".repeat(80))),
            });
            messages.push(ResponseItem::Assistant {
                content: Some(format!("a{i} {}", "w".repeat(80))),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: format!("call-{i}"),
                    name: "exec_command".to_string(),
                    identity: crate::tool::ToolIdentity::built_in("exec_command"),
                    arguments: json!({"command":"echo test"}),
                }],
            });
            messages.push(ResponseItem::Tool {
                tool_call_id: format!("call-{i}"),
                name: "exec_command".to_string(),
                content: "ok".to_string(),
                structured: Some(StructuredToolResult::CommandExecution {
                    command: "echo test".to_string(),
                    current_directory: "D:\\work".to_string(),
                    session_id: None,
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    success: Some(true),
                    output: Some("ok".to_string()),
                    duration_ms: Some(1),
                    original_token_count: Some(1),
                    max_output_tokens: Some(10_000),
                }),
            });
        }
        let tail_before = messages[messages.len().saturating_sub(6)..].to_vec();
        let plan = plan_history_compaction(
            &messages,
            ContextCompactionConfig {
                model_context_window: 2_048,
                trigger_ratio: 0.45,
                request_overhead_tokens: 128,
                compacted_target_tokens: 614,
                preserved_user_turns: 3,
                preserved_tail_tokens: 512,
                summary_source_max_tokens: 600,
            },
        )
        .expect("plan should exist");

        let result = apply_history_compaction(
            &mut messages,
            &plan,
            CompactionSummary::from_model_output(
                "Current Task:\n- Test\n\nProgress:\n- Done\n\nKey Decisions:\n- Keep core-owned compaction\n\nImportant Context:\n- Preserve system prompt\n\nTool / Code Facts:\n- exec_command used\n\nNext Steps:\n- Continue",
            )
            .ensure_defaults(),
        );

        let tail_after = messages[messages.len().saturating_sub(6)..].to_vec();
        assert_eq!(tail_before.len(), tail_after.len());
        assert!(matches!(
            (&tail_before[0], &tail_after[0]),
            (ResponseItem::User { content: before }, ResponseItem::User { content: after }) if before == after
        ));
        assert!(result.summary.rendered().contains("[Context Summary]"));
        assert_eq!(result.replacement_history.len(), messages.len());
    }

    #[test]
    fn compaction_source_keeps_multimodal_user_item_details() {
        let source = render_compaction_source(&[ResponseItem::User {
            content: vec![
                InputItem::Text {
                    text: "please inspect".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::RemoteUrl {
                        url: "https://example.com/diagram.png".to_string(),
                    },
                    detail: Some(ImageDetail::High),
                    alt: Some("system diagram".to_string()),
                },
                InputItem::File {
                    source: AttachmentRef::HubAsset {
                        asset_id: "asset-1".to_string(),
                        download_url: None,
                    },
                    mime_type: Some("application/pdf".to_string()),
                    name: Some("spec.pdf".to_string()),
                },
                InputItem::Mention {
                    name: "browser-use".to_string(),
                    path: "plugin://browser-use".to_string(),
                },
            ],
        }]);

        assert!(source.contains("please inspect"));
        assert!(source.contains(
            "[image alt=system diagram detail=high source=https://example.com/diagram.png]"
        ));
        assert!(source.contains("[file name=spec.pdf mime=application/pdf source=hub:asset-1]"));
        assert!(source.contains("[mention @browser-use path=plugin://browser-use]"));
    }

    #[test]
    fn fallback_summary_uses_multimodal_user_rendering() {
        let plan = ContextCompactionPlan {
            prefix: vec![ResponseItem::User {
                content: vec![
                    InputItem::Text {
                        text: "compare this".to_string(),
                    },
                    InputItem::Image {
                        source: AttachmentRef::LocalPath {
                            path: "D:\\images\\shot.png".to_string(),
                        },
                        detail: Some(ImageDetail::Low),
                        alt: Some("ui screenshot".to_string()),
                    },
                ],
            }],
            preserved_tail: Vec::new(),
            compacted_target_tokens: 128,
        };

        let summary = CompactionSummary::fallback_from_plan(&plan);

        assert!(summary.current_task[0].contains("compare this"));
        assert!(
            summary.current_task[0]
                .contains("[image alt=ui screenshot detail=low source=D:\\images\\shot.png]")
        );
    }

    #[test]
    fn adjust_tail_start_includes_tool_call_for_preserved_tool_result() {
        let messages = vec![
            ResponseItem::System {
                content: "system".repeat(20),
            },
            ResponseItem::User {
                content: text_input_items(format!("first {}", "x".repeat(80))),
            },
            ResponseItem::Assistant {
                content: Some(format!("calling tool {}", "y".repeat(80))),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "exec_command".to_string(),
                    identity: crate::tool::ToolIdentity::built_in("exec_command"),
                    arguments: json!({"command":"pwd"}),
                }],
            },
            ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: format!("D:/learn/gifti/cloudagent {}", "z".repeat(80)),
                structured: Some(StructuredToolResult::CommandExecution {
                    command: "pwd".to_string(),
                    current_directory: "D:/learn/gifti/cloudagent".to_string(),
                    session_id: None,
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    success: Some(true),
                    output: Some("D:/learn/gifti/cloudagent".to_string()),
                    duration_ms: Some(1),
                    original_token_count: Some(8),
                    max_output_tokens: Some(10_000),
                }),
            },
            ResponseItem::User {
                content: text_input_items(format!("continue {}", "q".repeat(80))),
            },
            ResponseItem::Assistant {
                content: Some(format!("done {}", "w".repeat(80))),
                reasoning: None,
                tool_calls: Vec::new(),
            },
            ResponseItem::User {
                content: text_input_items(format!("follow up {}", "n".repeat(80))),
            },
        ];

        let adjusted = adjust_tail_start_for_tool_invariants(&messages, 3);
        assert_eq!(adjusted, 2);
    }

    #[test]
    fn prefers_recent_user_boundary_when_tail_budget_allows() {
        let mut messages = vec![ResponseItem::System {
            content: "system".to_string(),
        }];
        for i in 0..6 {
            messages.push(ResponseItem::User {
                content: text_input_items(format!("user-{i} {}", "x".repeat(40))),
            });
            messages.push(ResponseItem::Assistant {
                content: Some(format!("assistant-{i} {}", "y".repeat(20))),
                reasoning: None,
                tool_calls: Vec::new(),
            });
        }

        let keep_start = choose_tail_start(&messages, 3, 200);
        assert!(matches!(
            &messages[keep_start],
            ResponseItem::User { content } if input_items_to_plain_text(content).starts_with("user-3")
        ));
    }

    #[test]
    fn falls_back_to_smaller_recent_suffix_when_requested_user_count_exceeds_tail_budget() {
        let mut messages = vec![ResponseItem::System {
            content: "system".to_string(),
        }];
        for i in 0..4 {
            messages.push(ResponseItem::User {
                content: text_input_items(format!("user-{i} {}", "x".repeat(160))),
            });
            messages.push(ResponseItem::Assistant {
                content: Some(format!("assistant-{i} {}", "y".repeat(160))),
                reasoning: None,
                tool_calls: Vec::new(),
            });
        }

        let keep_start = choose_tail_start(&messages, 3, 120);
        assert!(keep_start > 1);
        assert!(estimate_message_tokens(&messages[keep_start..]) <= 120);
    }
}
