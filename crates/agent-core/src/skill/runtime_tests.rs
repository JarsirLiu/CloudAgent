use super::{
    SkillDependenciesFrontmatter, SkillFrontmatter, SkillPolicyFrontmatter, SkillRuntime,
    frontmatter_invocation_mode, match_skills, split_frontmatter,
};
use crate::InputItem;
use crate::conversation::ResponseItem;
use crate::skill::render::latest_user_items;
use crate::skill::{SkillDependencies, SkillInvocationMode, SkillMetadata, SkillScope};
use crate::text_input_items;
use std::fs;
use std::path::PathBuf;

#[test]
fn split_frontmatter_reads_yaml_and_body() {
    let text = "---\nname: demo\ndescription: test\n---\n\n# Body\n";
    let (frontmatter, body) = split_frontmatter(text).expect("frontmatter");
    assert!(frontmatter.contains("name: demo"));
    assert_eq!(body, "# Body");
}

#[test]
fn match_skills_detects_structured_and_text_mentions() {
    let skill = SkillMetadata {
        name: "repo-reader".to_string(),
        description: "Read repos".to_string(),
        version: None,
        invocation_mode: SkillInvocationMode::Explicit,
        dependencies: SkillDependencies::default(),
        path: PathBuf::from("D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md"),
        scope: SkillScope::Workspace,
    };
    let items = vec![
        InputItem::Text {
            text: "please use $repo-reader".to_string(),
        },
        InputItem::Skill {
            name: "repo-reader".to_string(),
            path: "D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md".to_string(),
        },
    ];
    let matched = match_skills(&items, std::slice::from_ref(&skill));
    assert_eq!(matched, vec![skill]);
}

#[test]
fn plain_name_substring_does_not_trigger_explicit_skill() {
    let skill = SkillMetadata {
        name: "repo-reader".to_string(),
        description: "Analyze repository structure and module relationships".to_string(),
        version: None,
        invocation_mode: SkillInvocationMode::Explicit,
        dependencies: SkillDependencies::default(),
        path: PathBuf::from("D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md"),
        scope: SkillScope::Workspace,
    };
    let items = vec![InputItem::Text {
        text: "can you do some repo-reader style analysis".to_string(),
    }];

    let matched = match_skills(&items, &[skill]);
    assert!(matched.is_empty());
}

#[test]
fn plain_description_keywords_do_not_trigger_implicit_skill_in_backend() {
    let skill = SkillMetadata {
        name: "repo-reader".to_string(),
        description: "Analyze repository structure and module relationships".to_string(),
        version: Some("1.0.0".to_string()),
        invocation_mode: SkillInvocationMode::Implicit,
        dependencies: SkillDependencies {
            tools: vec!["rg".to_string()],
        },
        path: PathBuf::from("D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md"),
        scope: SkillScope::Workspace,
    };
    let items = vec![InputItem::Text {
        text: "please analyze the repository structure and module layout".to_string(),
    }];

    let matched = match_skills(&items, &[skill]);
    assert!(matched.is_empty());
}

#[test]
fn policy_allow_implicit_invocation_true_enables_implicit_mode() {
    let frontmatter = SkillFrontmatter {
        name: "demo".to_string(),
        description: "demo".to_string(),
        version: None,
        dependencies: SkillDependenciesFrontmatter::default(),
        policy: SkillPolicyFrontmatter {
            allow_implicit_invocation: Some(true),
        },
    };

    assert_eq!(
        frontmatter_invocation_mode(&frontmatter),
        SkillInvocationMode::Implicit
    );
}

#[test]
fn policy_allow_implicit_invocation_false_disables_implicit_mode() {
    let frontmatter = SkillFrontmatter {
        name: "demo".to_string(),
        description: "demo".to_string(),
        version: None,
        dependencies: SkillDependenciesFrontmatter::default(),
        policy: SkillPolicyFrontmatter {
            allow_implicit_invocation: Some(false),
        },
    };

    assert_eq!(
        frontmatter_invocation_mode(&frontmatter),
        SkillInvocationMode::Explicit
    );
}

#[test]
fn missing_policy_defaults_to_implicit_mode() {
    let frontmatter = SkillFrontmatter {
        name: "demo".to_string(),
        description: "demo".to_string(),
        version: None,
        dependencies: SkillDependenciesFrontmatter::default(),
        policy: SkillPolicyFrontmatter {
            allow_implicit_invocation: None,
        },
    };

    assert_eq!(
        frontmatter_invocation_mode(&frontmatter),
        SkillInvocationMode::Implicit
    );
}

#[test]
fn latest_user_items_reads_latest_user_message() {
    let messages = vec![
        ResponseItem::System {
            content: "system".to_string(),
        },
        ResponseItem::User {
            content: text_input_items("hello"),
        },
    ];
    let items = latest_user_items(&messages).expect("user items");
    assert_eq!(items, text_input_items("hello").as_slice());
}

#[test]
fn ensure_system_skills_writes_creator_skill_markdown() {
    let temp_home = std::env::temp_dir().join(format!(
        "cloudagent-skill-runtime-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_home);
    fs::create_dir_all(&temp_home).expect("create temp home");

    let previous_home = std::env::var_os("HOME");
    let previous_userprofile = std::env::var_os("USERPROFILE");
    unsafe {
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
    }

    let runtime = SkillRuntime::new(true, Vec::new());
    runtime
        .ensure_system_skills()
        .expect("install system skill");

    let skill_md_path = temp_home
        .join(".cloudagent")
        .join("skills")
        .join(".system")
        .join("skill-creator")
        .join("SKILL.md");
    assert!(skill_md_path.is_file());

    match previous_home {
        Some(value) => unsafe { std::env::set_var("HOME", value) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    match previous_userprofile {
        Some(value) => unsafe { std::env::set_var("USERPROFILE", value) },
        None => unsafe { std::env::remove_var("USERPROFILE") },
    }
    let _ = fs::remove_dir_all(&temp_home);
}
