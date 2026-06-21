use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::conversation::InputItem;

use super::model::{
    SkillCatalog, SkillDependencies, SkillDocument, SkillInvocationMode, SkillMetadata, SkillScope,
    TurnSkillContext,
};
use super::render::{latest_user_items, render_skill_budget_summary};

const SKILL_FILENAME: &str = "SKILL.md";
const SYSTEM_SKILL_CREATOR_NAME: &str = "skill-creator";
const SYSTEM_SKILL_CREATOR_DOC: &str = include_str!("assets/skill-creator.md");

#[derive(Clone, Debug, Default)]
pub struct SkillRuntime {
    enabled: bool,
    configured_roots: Vec<PathBuf>,
}

impl SkillRuntime {
    pub fn new(enabled: bool, configured_roots: Vec<PathBuf>) -> Self {
        Self {
            enabled,
            configured_roots,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn load_catalog(&self, workspace_root: &Path) -> SkillCatalog {
        if !self.enabled {
            return SkillCatalog::default();
        }

        let _ = self.ensure_system_skills();
        let mut seen = HashSet::new();
        let mut skills = Vec::new();
        let mut errors = Vec::new();
        for (root, scope) in self.skill_roots(workspace_root) {
            self.collect_skills_from_root(&root, scope, &mut seen, &mut skills, &mut errors);
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        SkillCatalog { skills, errors }
    }

    pub fn watch_roots(&self, workspace_root: &Path) -> Vec<PathBuf> {
        self.skill_roots(workspace_root)
            .into_iter()
            .map(|(root, _)| root)
            .collect()
    }

    pub fn collect_turn_explicit_skill_documents(
        &self,
        messages: &[crate::ResponseItem],
        catalog: &SkillCatalog,
    ) -> Vec<SkillDocument> {
        if !self.enabled {
            return Vec::new();
        }

        let matched = self.collect_turn_skill_candidates(messages, catalog);
        self.load_skill_documents_for_explicit_use(&matched)
    }

    pub fn collect_turn_skill_candidates(
        &self,
        messages: &[crate::ResponseItem],
        catalog: &SkillCatalog,
    ) -> Vec<SkillMetadata> {
        if !self.enabled {
            return Vec::new();
        }

        let Some(items) = latest_user_items(messages) else {
            return Vec::new();
        };
        match_skills(items, &catalog.skills)
    }

    pub fn load_skill_documents_for_explicit_use(
        &self,
        skills: &[SkillMetadata],
    ) -> Vec<SkillDocument> {
        if !self.enabled {
            return Vec::new();
        }

        skills
            .iter()
            .filter_map(|skill| load_skill_document(&skill.path, skill.scope.clone()).ok())
            .collect()
    }

    pub fn build_turn_skill_context(
        &self,
        workspace_root: &Path,
        messages: &[crate::ResponseItem],
    ) -> TurnSkillContext {
        let catalog = self.load_catalog(workspace_root);
        let matched = self.collect_turn_skill_candidates(messages, &catalog);
        TurnSkillContext {
            catalog_summary: render_skill_budget_summary(
                &catalog.skills_allowed_for_implicit_invocation(),
            ),
            explicit_documents: self.load_skill_documents_for_explicit_use(&matched),
        }
    }

    fn skill_roots(&self, workspace_root: &Path) -> Vec<(PathBuf, SkillScope)> {
        let mut roots = Vec::new();
        if self.configured_roots.is_empty() {
            roots.push((
                workspace_root.join(".cloudagent").join("skills"),
                SkillScope::Workspace,
            ));
            if let Some(home) = user_home_dir() {
                roots.push((home.join(".cloudagent").join("skills"), SkillScope::User));
                roots.push((
                    home.join(".cloudagent").join("skills").join(".system"),
                    SkillScope::System,
                ));
            }
        } else {
            roots.extend(
                self.configured_roots
                    .iter()
                    .cloned()
                    .map(|path| (path, SkillScope::Workspace)),
            );
        }
        roots
    }

    fn collect_skills_from_root(
        &self,
        root: &Path,
        scope: SkillScope,
        seen: &mut HashSet<PathBuf>,
        skills: &mut Vec<SkillMetadata>,
        errors: &mut Vec<String>,
    ) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_path = path.join(SKILL_FILENAME);
            if !skill_path.is_file() {
                continue;
            }
            let canonical = fs::canonicalize(&skill_path).unwrap_or(skill_path.clone());
            if !seen.insert(canonical.clone()) {
                continue;
            }
            match load_skill_document(&canonical, scope.clone()) {
                Ok(document) => skills.push(document.metadata),
                Err(err) => errors.push(format!("{}: {err:#}", canonical.display())),
            }
        }
    }

    fn ensure_system_skills(&self) -> Result<()> {
        let Some(home) = user_home_dir() else {
            return Ok(());
        };
        let skill_dir = home
            .join(".cloudagent")
            .join("skills")
            .join(".system")
            .join(SYSTEM_SKILL_CREATOR_NAME);
        write_skill_asset_if_missing(&skill_dir.join(SKILL_FILENAME), SYSTEM_SKILL_CREATOR_DOC)?;
        Ok(())
    }
}

fn write_skill_asset_if_missing(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("missing parent for {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    if !path.is_file() {
        fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    version: Option<String>,
    #[serde(default)]
    dependencies: SkillDependenciesFrontmatter,
    #[serde(default)]
    policy: SkillPolicyFrontmatter,
}

#[derive(Debug, Deserialize, Default)]
struct SkillPolicyFrontmatter {
    allow_implicit_invocation: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct SkillDependenciesFrontmatter {
    #[serde(default)]
    tools: Vec<String>,
}

fn frontmatter_invocation_mode(frontmatter: &SkillFrontmatter) -> SkillInvocationMode {
    match frontmatter.policy.allow_implicit_invocation {
        Some(false) => SkillInvocationMode::Explicit,
        Some(true) | None => SkillInvocationMode::Implicit,
    }
}

fn match_skills(items: &[InputItem], skills: &[SkillMetadata]) -> Vec<SkillMetadata> {
    let mut selected = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut explicit_names = HashSet::new();

    for item in items {
        if let InputItem::Skill { name, path } = item {
            explicit_names.insert(name.to_ascii_lowercase());
            if let Some(skill) = skills.iter().find(|skill| skill.path == Path::new(path))
                && seen_paths.insert(skill.path.clone())
            {
                selected.push(skill.clone());
            }
        }
    }

    for item in items {
        let InputItem::Text { text } = item else {
            continue;
        };
        let lowered = text.to_ascii_lowercase();
        for skill in skills {
            let skill_name = skill.name.to_ascii_lowercase();
            let is_explicit_text_match = contains_explicit_skill_mention(&lowered, &skill_name);
            if explicit_names.contains(&skill_name) && !is_explicit_text_match {
                continue;
            }
            if is_explicit_text_match && seen_paths.insert(skill.path.clone()) {
                selected.push(skill.clone());
            }
        }
    }

    selected
}

fn load_skill_document(path: &Path, scope: SkillScope) -> Result<SkillDocument> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read skill file {}", path.display()))?;
    let (frontmatter, body) = split_frontmatter(&contents)
        .with_context(|| format!("invalid skill frontmatter in {}", path.display()))?;
    let parsed: SkillFrontmatter = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("failed to parse YAML in {}", path.display()))?;
    let invocation_mode = frontmatter_invocation_mode(&parsed);
    let body = body.trim().to_string();
    Ok(SkillDocument {
        metadata: SkillMetadata {
            name: parsed.name,
            description: parsed.description,
            version: parsed.version,
            invocation_mode,
            dependencies: SkillDependencies {
                tools: parsed.dependencies.tools,
            },
            path: path.to_path_buf(),
            scope,
        },
        body,
        contents,
    })
}

fn split_frontmatter(contents: &str) -> Result<(&str, &str)> {
    let mut sections = contents.splitn(3, "---");
    let before = sections.next().unwrap_or_default();
    let frontmatter = sections.next().context("missing frontmatter")?;
    let body = sections.next().context("missing body")?;
    if !before.trim().is_empty() {
        anyhow::bail!("frontmatter must start at file beginning");
    }
    Ok((frontmatter.trim(), body.trim()))
}

fn contains_explicit_skill_mention(text: &str, skill_name: &str) -> bool {
    text.contains(&format!("${skill_name}"))
}

fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
