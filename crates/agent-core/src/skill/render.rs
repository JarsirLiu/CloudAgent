use crate::conversation::{InputItem, ResponseItem, text_input_items};

use super::model::{SkillDocument, SkillMetadata};

pub fn render_skill_catalog(skills: &[SkillMetadata]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(skills.len() + 16);
    lines.push("## Skills".to_string());
    lines.push(
        "A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and file path so you can open the source for full instructions when using a specific skill.".to_string(),
    );
    lines.push("### Available skills".to_string());
    for skill in skills {
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
        lines.push(format!(
            "- {}: {}{} (file: {})",
            skill.name,
            skill.description,
            metadata_suffix,
            skill.path.display()
        ));
    }
    lines.push("### How to use skills".to_string());
    lines.push("- Discovery: The list above is the skills available in this session (name + description + file path). Skill bodies live on disk at the listed paths.".to_string());
    lines.push("- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.".to_string());
    lines.push("- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.".to_string());
    lines.push("- How to use a skill (progressive disclosure):".to_string());
    lines.push("  1) After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.".to_string());
    lines.push("  2) When `SKILL.md` references relative paths (e.g., `scripts/foo.py`), resolve them relative to the skill directory listed above first, and only consider other paths if needed.".to_string());
    lines.push("  3) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.".to_string());
    lines.push("  4) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.".to_string());
    lines.push(
        "  5) If `assets/` or templates exist, reuse them instead of recreating from scratch."
            .to_string(),
    );
    lines.push("- Coordination and sequencing:".to_string());
    lines.push("  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.".to_string());
    lines.push("  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.".to_string());
    lines.push("- Context hygiene:".to_string());
    lines.push("  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.".to_string());
    lines.push("  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.".to_string());
    lines.push("  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.".to_string());
    lines.push("- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue.".to_string());
    Some(lines.join("\n"))
}

pub fn render_skill_injection(document: &SkillDocument) -> ResponseItem {
    ResponseItem::User {
        content: text_input_items(format!(
            "<skill>\n<name>{}</name>\n<path>{}</path>\n{}\n</skill>",
            document.metadata.name,
            document.metadata.path.display(),
            document.contents
        )),
    }
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
