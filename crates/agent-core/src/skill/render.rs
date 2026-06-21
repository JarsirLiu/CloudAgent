use crate::conversation::{InputItem, ResponseItem, text_input_items};

use super::model::{SkillDocument, SkillMetadata};

pub fn render_skill_budget_summary(skills: &[SkillMetadata]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(skills.len() + 2);
    lines.push("## Skills".to_string());
    lines.push(
        "Available local skills for this turn. Open the referenced `SKILL.md` before following one."
            .to_string(),
    );
    lines.extend(skills.iter().map(render_skill_summary_item));
    Some(lines.join("\n"))
}

pub fn render_skill_summary_item(skill: &SkillMetadata) -> String {
    let mut suffix = Vec::new();
    if let Some(version) = &skill.version {
        suffix.push(format!("version: {version}"));
    }
    if !skill.dependencies.tools.is_empty() {
        suffix.push(format!("deps: {}", skill.dependencies.tools.join(", ")));
    }
    let metadata_suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(" [{}]", suffix.join("; "))
    };
    format!(
        "- {}: {}{} (file: {})",
        skill.name,
        skill.description,
        metadata_suffix,
        skill.path.display()
    )
}

pub fn render_skill_injection(document: &SkillDocument) -> ResponseItem {
    render_skill_injection_contents(document, document.contents.clone())
}

pub fn render_truncated_skill_injection(
    document: &SkillDocument,
    max_chars: usize,
) -> ResponseItem {
    let contents = truncate_skill_contents(&document.contents, max_chars);
    render_skill_injection_contents(document, contents)
}

fn render_skill_injection_contents(document: &SkillDocument, contents: String) -> ResponseItem {
    ResponseItem::User {
        content: text_input_items(format!(
            "<skill>\n<name>{}</name>\n<path>{}</path>\n{}\n</skill>",
            document.metadata.name,
            document.metadata.path.display(),
            contents
        )),
    }
}

fn truncate_skill_contents(contents: &str, max_chars: usize) -> String {
    let trimmed = contents.trim();
    if trimmed.is_empty() || max_chars == 0 {
        return trimmed.to_string();
    }

    let total_chars = trimmed.chars().count();
    if total_chars <= max_chars {
        return trimmed.to_string();
    }

    let marker = "\n[truncated for budget]";
    let marker_chars = marker.chars().count();
    if max_chars <= marker_chars {
        return trimmed.chars().take(max_chars).collect();
    }

    let head_chars = max_chars.saturating_sub(marker_chars);
    let head = trimmed
        .chars()
        .take(head_chars)
        .collect::<String>()
        .trim_end()
        .to_string();
    format!("{head}{marker}")
}

pub fn latest_user_items(messages: &[ResponseItem]) -> Option<&[InputItem]> {
    messages.iter().rev().find_map(|item| match item {
        ResponseItem::User { content } => Some(content.as_slice()),
        _ => None,
    })
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
