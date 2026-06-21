use super::{
    render_skill_budget_summary, render_skill_injection, render_skill_summary_item,
    render_truncated_skill_injection,
};
use crate::skill::{
    SkillDependencies, SkillDocument, SkillInvocationMode, SkillMetadata, SkillScope,
};
use crate::{ResponseItem, input_items_to_plain_text};
use std::path::PathBuf;

fn sample_skill(name: &str) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("Use {name} for repository tasks"),
        version: Some("1.0.0".to_string()),
        invocation_mode: SkillInvocationMode::Implicit,
        dependencies: SkillDependencies {
            tools: vec!["rg".to_string(), "git".to_string()],
        },
        path: PathBuf::from(format!("D:\\repo\\.cloudagent\\skills\\{name}\\SKILL.md")),
        scope: SkillScope::Workspace,
    }
}

#[test]
fn render_skill_budget_summary_is_more_compact_but_keeps_path() {
    let summary = render_skill_budget_summary(&[sample_skill("repo-reader")]).expect("summary");
    assert!(summary.contains("## Skills"));
    assert!(summary.contains("Available local skills for this turn"));
    assert!(summary.contains("repo-reader"));
    assert!(summary.contains("file: D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md"));
    assert!(!summary.contains("### How to use skills"));
}

#[test]
fn render_skill_summary_item_includes_metadata_suffixes() {
    let line = render_skill_summary_item(&sample_skill("repo-reader"));
    assert!(line.contains("repo-reader"));
    assert!(line.contains("version: 1.0.0"));
    assert!(line.contains("deps: rg, git"));
}

#[test]
fn render_skill_injection_wraps_full_document_with_metadata() {
    let document = SkillDocument {
        metadata: sample_skill("repo-reader"),
        body: "# Repo Reader\nUse this skill.\n".to_string(),
        contents: "---\nname: repo-reader\ndescription: Use repo-reader for repository tasks\n---\n\n# Repo Reader\nUse this skill.\n".to_string(),
    };

    let rendered = render_skill_injection(&document);
    let text = match rendered {
        ResponseItem::User { content } => input_items_to_plain_text(&content),
        _ => panic!("expected user response item"),
    };
    assert!(text.contains("<skill>"));
    assert!(text.contains("<name>repo-reader</name>"));
    assert!(text.contains("<path>D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md</path>"));
    assert!(text.contains("# Repo Reader"));
}

#[test]
fn render_truncated_skill_injection_marks_budget_truncation() {
    let document = SkillDocument {
        metadata: sample_skill("repo-reader"),
        body: "# Repo Reader\nUse this skill.\n".to_string(),
        contents: "---\nname: repo-reader\ndescription: Use repo-reader for repository tasks\n---\n\n# Repo Reader\nUse this skill with a longer body section that will be truncated.\n".to_string(),
    };

    let rendered = render_truncated_skill_injection(&document, 96);
    let text = match rendered {
        ResponseItem::User { content } => input_items_to_plain_text(&content),
        _ => panic!("expected user response item"),
    };
    assert!(text.contains("<name>repo-reader</name>"));
    assert!(text.contains("[truncated for budget]"));
}
