use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillInvocationMode {
    #[default]
    Implicit,
    Explicit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillScope {
    Workspace,
    User,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub invocation_mode: SkillInvocationMode,
    pub dependencies: SkillDependencies,
    pub path: PathBuf,
    pub scope: SkillScope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SkillDependencies {
    pub tools: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillDocument {
    pub metadata: SkillMetadata,
    pub body: String,
    pub contents: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnSkillContext {
    pub catalog_summary: Option<String>,
    pub explicit_documents: Vec<SkillDocument>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillCatalog {
    pub skills: Vec<SkillMetadata>,
    pub errors: Vec<String>,
}

impl SkillCatalog {
    pub fn skills_allowed_for_implicit_invocation(&self) -> Vec<SkillMetadata> {
        self.skills
            .iter()
            .filter(|skill| skill.invocation_mode == SkillInvocationMode::Implicit)
            .cloned()
            .collect()
    }
}
