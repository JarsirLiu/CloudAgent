use super::{SkillScaffoldSpec, create_skill_scaffold};
use std::fs;

#[test]
fn normalized_name_keeps_hyphenated_slug_shape() {
    assert_eq!(
        SkillScaffoldSpec::normalized_name(" Repo Reader_v2 ").expect("normalized"),
        "repo-reader-v2"
    );
}

#[test]
fn create_skill_scaffold_writes_skill_md_and_optional_dirs() {
    let root = std::env::temp_dir().join(format!(
        "cloudagent-skill-scaffold-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp root");

    let spec = SkillScaffoldSpec {
        name: "repo-reader".to_string(),
        parent_dir: root.clone(),
        create_scripts_dir: true,
        create_references_dir: true,
        create_assets_dir: false,
        overwrite: false,
    };

    let outcome = create_skill_scaffold(&spec).expect("create scaffold");
    assert_eq!(outcome.skill_name, "repo-reader");
    assert!(outcome.skill_md_path.is_file());
    assert!(outcome.skill_dir.join("scripts").is_dir());
    assert!(outcome.skill_dir.join("references").is_dir());
    assert!(!outcome.skill_dir.join("assets").exists());

    let contents = fs::read_to_string(&outcome.skill_md_path).expect("read skill md");
    assert!(contents.contains("name: repo-reader"));
    assert!(contents.contains("# Repo Reader"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn create_skill_scaffold_refuses_existing_skill_without_overwrite() {
    let root = std::env::temp_dir().join(format!(
        "cloudagent-skill-scaffold-overwrite-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("repo-reader")).expect("create skill dir");
    fs::write(root.join("repo-reader").join("SKILL.md"), "existing").expect("seed skill");

    let spec = SkillScaffoldSpec {
        name: "repo-reader".to_string(),
        parent_dir: root.clone(),
        create_scripts_dir: false,
        create_references_dir: false,
        create_assets_dir: false,
        overwrite: false,
    };

    let err = create_skill_scaffold(&spec).expect_err("should reject overwrite");
    assert!(err.to_string().contains("already exists"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn create_skill_scaffold_normalizes_incoming_name() {
    let root = std::env::temp_dir().join(format!(
        "cloudagent-skill-scaffold-normalize-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create temp root");

    let outcome = create_skill_scaffold(&SkillScaffoldSpec {
        name: "Repo Reader".to_string(),
        parent_dir: root.clone(),
        create_scripts_dir: false,
        create_references_dir: false,
        create_assets_dir: false,
        overwrite: false,
    })
    .expect("create scaffold");

    assert_eq!(outcome.skill_name, "repo-reader");
    assert!(outcome.skill_dir.ends_with("repo-reader"));

    let _ = fs::remove_dir_all(&root);
}
