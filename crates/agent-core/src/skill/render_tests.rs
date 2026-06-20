use super::{render_skill_catalog, render_skill_injection};
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
fn render_skill_catalog_lists_name_description_and_path() {
    let catalog = render_skill_catalog(&[sample_skill("repo-reader")]).expect("catalog");
    assert!(catalog.contains("## Skills"));
    assert!(catalog.contains("repo-reader"));
    assert!(catalog.contains("Use repo-reader for repository tasks"));
    assert!(catalog.contains("file: D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md"));
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
