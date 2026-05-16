use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use super::scaffold::SkillScaffoldSpec;

const SKILL_FILENAME: &str = "SKILL.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillValidationReport {
    pub skill_dir: PathBuf,
    pub skill_md_path: PathBuf,
    pub skill_name: String,
}

pub fn validate_skill_dir(skill_dir: &Path) -> Result<SkillValidationReport> {
    if !skill_dir.is_dir() {
        bail!("{} is not a skill directory", skill_dir.display());
    }

    let skill_md_path = skill_dir.join(SKILL_FILENAME);
    if !skill_md_path.is_file() {
        bail!("{} is missing", skill_md_path.display());
    }

    let contents = fs::read_to_string(&skill_md_path)
        .with_context(|| format!("failed to read {}", skill_md_path.display()))?;
    let (frontmatter, body) = split_frontmatter(&contents)
        .with_context(|| format!("invalid skill frontmatter in {}", skill_md_path.display()))?;
    let parsed: SkillFrontmatter = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("failed to parse YAML in {}", skill_md_path.display()))?;

    let normalized_name = SkillScaffoldSpec::normalized_name(&parsed.name)?;
    if parsed.name != normalized_name {
        bail!(
            "skill name `{}` must already be normalized as `{}`",
            parsed.name,
            normalized_name
        );
    }

    if parsed.description.trim().is_empty() {
        bail!("skill description must not be empty");
    }
    if body.trim().is_empty() {
        bail!("skill body must not be empty");
    }

    let folder_name = skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .context("skill directory name is not valid unicode")?;
    if folder_name != normalized_name {
        bail!(
            "skill directory `{folder_name}` must match normalized skill name `{normalized_name}`"
        );
    }

    Ok(SkillValidationReport {
        skill_dir: skill_dir.to_path_buf(),
        skill_md_path,
        skill_name: normalized_name,
    })
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

fn split_frontmatter(contents: &str) -> Result<(&str, &str)> {
    let mut sections = contents.splitn(3, "---");
    let before = sections.next().unwrap_or_default();
    let frontmatter = sections.next().context("missing frontmatter")?;
    let body = sections.next().context("missing body")?;
    if !before.trim().is_empty() {
        bail!("frontmatter must start at file beginning");
    }
    Ok((frontmatter.trim(), body.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::{SkillScaffoldSpec, create_skill_scaffold};

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
            skill_dir.join(SKILL_FILENAME),
            "---\nname: repo-reader\ndescription: demo\n---\n\n# Demo\n",
        )
        .expect("write skill");

        let err = validate_skill_dir(&skill_dir).expect_err("should reject mismatch");
        assert!(err.to_string().contains("must match normalized skill name"));

        let _ = fs::remove_dir_all(&root);
    }
}
