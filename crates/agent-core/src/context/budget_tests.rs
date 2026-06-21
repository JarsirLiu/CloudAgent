use super::{
    ContextBudgetSource, ContextFacade, FilterPolicy, SkillBudgetSource,
    build_context_budgeted_fragments,
};
use crate::conversation::{ResponseItem, input_items_to_plain_text};
use crate::skill::{
    SkillDependencies, SkillDocument, SkillInvocationMode, SkillMetadata, SkillScope,
};
use std::path::{Path, PathBuf};

fn sample_skill_document() -> SkillDocument {
    SkillDocument {
        metadata: SkillMetadata {
            name: "repo-reader".to_string(),
            description: "Read repositories".to_string(),
            version: Some("1.0.0".to_string()),
            invocation_mode: SkillInvocationMode::Explicit,
            dependencies: SkillDependencies {
                tools: vec!["rg".to_string()],
            },
            path: PathBuf::from("D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md"),
            scope: SkillScope::Workspace,
        },
        body: "# Repo Reader\nUse this skill to inspect large repositories.\n".to_string(),
        contents: concat!(
            "---\n",
            "name: repo-reader\n",
            "description: Read repositories\n",
            "---\n\n",
            "# Repo Reader\n",
            "Use this skill to inspect large repositories.\n",
            "Include additional guidance, examples, and policy notes so the document is long enough to be truncated when the budget is tight.\n",
            "Section A: repository mapping, ownership heuristics, and code search strategies.\n",
            "Section B: dependency tracing, entrypoint discovery, and test-surface enumeration.\n",
            "Section C: patch safety checks, rollout notes, and fallback recovery guidance.\n",
            "Section D: summarize diffs, verify assumptions, and capture residual risks.\n"
        )
        .to_string(),
    }
}

#[test]
fn build_context_budgeted_fragments_tracks_skill_bucket_audit() {
    let facade = ContextFacade::new();
    let source = ContextBudgetSource {
        skills: SkillBudgetSource {
            summary: Some("repo-reader: local repo analysis".to_string()),
            explicit_documents: vec![sample_skill_document()],
            enable_summary_bucket: true,
            post_compact_budget_tokens: 48,
            max_tokens_per_item: 24,
        },
        post_compact_budget_tokens: 96,
        post_compact_memory_floor_tokens: 0,
        post_compact_max_tokens_per_memory: 0,
        safety_buffer_tokens: 0,
        ..ContextBudgetSource::default()
    };

    let budgeted = build_context_budgeted_fragments(
        &facade,
        &[],
        FilterPolicy { enabled: false },
        ResponseItem::System {
            content: "system".to_string(),
        },
        Path::new("D:\\repo"),
        4_000,
        1.0,
        source,
    );

    assert!(budgeted.audit.skill_bucket.before > 0);
    assert!(budgeted.audit.skill_bucket.after > 0);
    assert_eq!(
        budgeted.audit.skills_before,
        budgeted.audit.skill_bucket.before
    );
    assert_eq!(
        budgeted.audit.skills_after,
        budgeted.audit.skill_bucket.after
    );
    assert!(budgeted.audit.skill_bucket.truncated);
    assert!(budgeted.audit.skill_bucket.kept_items >= 1);
}

#[test]
fn build_context_budgeted_fragments_uses_truncated_skill_injection_under_tight_budget() {
    let facade = ContextFacade::new();
    let source = ContextBudgetSource {
        skills: SkillBudgetSource {
            summary: None,
            explicit_documents: vec![sample_skill_document()],
            enable_summary_bucket: false,
            post_compact_budget_tokens: 40,
            max_tokens_per_item: 40,
        },
        post_compact_budget_tokens: 80,
        post_compact_memory_floor_tokens: 0,
        post_compact_max_tokens_per_memory: 0,
        safety_buffer_tokens: 0,
        ..ContextBudgetSource::default()
    };

    let budgeted = build_context_budgeted_fragments(
        &facade,
        &[],
        FilterPolicy { enabled: false },
        ResponseItem::System {
            content: "system".to_string(),
        },
        Path::new("D:\\repo"),
        4_000,
        1.0,
        source,
    );

    let rendered = budgeted
        .fragments
        .iter()
        .find_map(|item| match item {
            ResponseItem::User { content } => {
                let text = input_items_to_plain_text(content);
                text.contains("<skill>").then_some(text)
            }
            _ => None,
        })
        .expect("rendered skill fragment");

    assert!(budgeted.audit.skill_bucket.truncated);
    assert!(rendered.contains("<name>repo-reader</name>"));
    assert!(
        !rendered.contains(
            "Section D: summarize diffs, verify assumptions, and capture residual risks."
        )
    );
}
