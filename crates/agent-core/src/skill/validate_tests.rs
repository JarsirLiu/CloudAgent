use super::validate_skill_dir;
use crate::skill::{SkillScaffoldSpec, create_skill_scaffold};
use std::fs;

#[test]
fn validate_skill_dir_accepts_generated_skill() {
    let root = std::env::temp_dir().join(format!(
        "cloudagent-skill-validate-test-{}",
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

    let report = validate_skill_dir(&outcome.skill_dir).expect("validate skill");
    assert_eq!(report.skill_name, "repo-reader");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn validate_skill_dir_rejects_mismatched_folder_name() {
    let root = std::env::temp_dir().join(format!(
        "cloudagent-skill-validate-bad-folder-test-{}",
        std::process::id()
    ));
    let skill_dir = root.join("wrong-folder");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: repo-reader\ndescription: demo\n---\n\n# Demo\n",
    )
    .expect("write skill");

    let err = validate_skill_dir(&skill_dir).expect_err("should reject mismatch");
    assert!(err.to_string().contains("must match normalized skill name"));

    let _ = fs::remove_dir_all(&root);
}
