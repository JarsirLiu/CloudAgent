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
#[path = "validate_tests.rs"]
mod tests;
