use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

const SKILL_FILENAME: &str = "SKILL.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillScaffoldSpec {
    pub name: String,
    pub parent_dir: PathBuf,
    pub create_scripts_dir: bool,
    pub create_references_dir: bool,
    pub create_assets_dir: bool,
    pub overwrite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillScaffoldOutcome {
    pub skill_name: String,
    pub skill_dir: PathBuf,
    pub skill_md_path: PathBuf,
}

impl SkillScaffoldSpec {
    pub fn normalized_name(raw: &str) -> Result<String> {
        let mut normalized = raw.trim().to_ascii_lowercase();
        normalized = normalized.replace(['_', ' '], "-");
        normalized = normalized
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' {
                    ch
                } else {
                    '-'
                }
            })
            .collect::<String>();
        while normalized.contains("--") {
            normalized = normalized.replace("--", "-");
        }
        normalized = normalized.trim_matches('-').to_string();
        if normalized.is_empty() {
            bail!("skill name must contain at least one letter or number");
        }
        Ok(normalized)
    }
}

pub fn create_skill_scaffold(spec: &SkillScaffoldSpec) -> Result<SkillScaffoldOutcome> {
    let skill_name = SkillScaffoldSpec::normalized_name(&spec.name)?;
    let skill_dir = spec.parent_dir.join(&skill_name);
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;

    if spec.create_scripts_dir {
        fs::create_dir_all(skill_dir.join("scripts"))
            .with_context(|| format!("failed to create {}", skill_dir.join("scripts").display()))?;
    }
    if spec.create_references_dir {
        fs::create_dir_all(skill_dir.join("references")).with_context(|| {
            format!(
                "failed to create {}",
                skill_dir.join("references").display()
            )
        })?;
    }
    if spec.create_assets_dir {
        fs::create_dir_all(skill_dir.join("assets"))
            .with_context(|| format!("failed to create {}", skill_dir.join("assets").display()))?;
    }

    let skill_md_path = skill_dir.join(SKILL_FILENAME);
    if skill_md_path.exists() && !spec.overwrite {
        bail!(
            "{} already exists; set overwrite to replace it",
            skill_md_path.display()
        );
    }

    fs::write(&skill_md_path, render_skill_template(&skill_name))
        .with_context(|| format!("failed to write {}", skill_md_path.display()))?;

    Ok(SkillScaffoldOutcome {
        skill_name,
        skill_dir,
        skill_md_path,
    })
}

pub fn render_skill_template(skill_name: &str) -> String {
    let title = skill_name
        .split('-')
        .filter(|part| !part.is_empty())
        .map(capitalize)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "---\nname: {skill_name}\ndescription: Use this skill when the user needs ...\ndependencies:\n  tools: []\n---\n\n# {title}\n\n## When to use\n\n- TODO: describe the repeatable workflow this skill covers\n\n## Workflow\n\n1. TODO: define the trigger and first step\n2. TODO: define the core procedure\n3. TODO: define validation or wrap-up steps\n\n## Extra files\n\n- Add `references/` files when the skill needs supporting docs\n- Add `scripts/` helpers when execution should be deterministic\n- Add `assets/` when the skill should reuse templates or fixtures\n"
    )
}

fn capitalize(part: &str) -> String {
    let mut chars = part.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = first.to_ascii_uppercase().to_string();
    out.push_str(chars.as_str());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        fs::write(root.join("repo-reader").join(SKILL_FILENAME), "existing").expect("seed skill");

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
}
