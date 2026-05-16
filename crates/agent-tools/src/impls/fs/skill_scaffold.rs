use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_write_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolLayer, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{
    SkillScaffoldSpec, StructuredToolResult, ToolExecutionContext, ToolExecutionPolicy,
    ToolIdentity, ToolSpec, TurnItemDeltaKind, TurnItemKind, create_skill_scaffold,
    validate_skill_dir,
};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

pub struct CreateSkillScaffoldTool;

impl CreateSkillScaffoldTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::WorkspaceWrite,
            vec!["skill", "scaffold", "fs"],
            ToolUsageGuidance {
                selection_priority: 6,
                preferred_for: vec![
                    "creating a new skill package with the standard CloudAgent layout",
                    "scaffolding a SKILL.md entrypoint plus optional skill resource folders",
                ],
                avoid_for: vec![
                    "editing an existing skill body in place",
                    "creating arbitrary non-skill directories",
                ],
                follow_up_hint: Some(
                    "after scaffolding, fill in the generated SKILL.md and then run `validate_skill` on the created directory",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "create_skill_scaffold".to_string(),
                identity: ToolIdentity::built_in("create_skill_scaffold"),
                description:
                    "Create a new CloudAgent skill directory with a generated SKILL.md template and optional scripts/references/assets folders."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "path": { "type": "string" },
                        "resources": {
                            "type": "array",
                            "items": { "type": "string", "enum": ["scripts", "references", "assets"] }
                        },
                        "overwrite": { "type": "boolean" }
                    },
                    "required": ["name", "path"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::PlatformFs)
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

pub struct ValidateSkillTool;

impl ValidateSkillTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["skill", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 5,
                preferred_for: vec![
                    "checking whether a generated or edited skill folder is structurally valid",
                    "verifying SKILL.md naming and frontmatter after edits",
                ],
                avoid_for: vec!["creating or editing files"],
                follow_up_hint: Some(
                    "if validation fails, fix the reported issue and rerun `validate_skill` before relying on the skill",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "validate_skill".to_string(),
                identity: ToolIdentity::built_in("validate_skill"),
                description:
                    "Validate one CloudAgent skill directory, including SKILL.md presence, normalized naming, and non-empty frontmatter/body."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::PlatformFs)
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Debug, Deserialize)]
struct CreateSkillScaffoldArgs {
    name: String,
    path: String,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    overwrite: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ValidateSkillArgs {
    path: String,
}

pub(crate) struct CreateSkillScaffoldLocalTool;

pub(crate) struct ValidateSkillLocalTool;

#[async_trait]
impl LocalTool for CreateSkillScaffoldLocalTool {
    fn spec(&self) -> ToolSpec {
        CreateSkillScaffoldTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: CreateSkillScaffoldArgs = invocation.payload.parse_arguments()?;
        let parent_dir = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.path.as_str()),
        )?;
        let spec = SkillScaffoldSpec {
            name: args.name,
            parent_dir,
            create_scripts_dir: args.resources.iter().any(|item| item == "scripts"),
            create_references_dir: args.resources.iter().any(|item| item == "references"),
            create_assets_dir: args.resources.iter().any(|item| item == "assets"),
            overwrite: args.overwrite.unwrap_or(false),
        };
        let outcome = create_skill_scaffold(&spec)?;
        Ok(ToolInvocationOutput {
            content: format!(
                "Created skill scaffold `{}` at `{}` with entry file `{}`.",
                outcome.skill_name,
                outcome.skill_dir.display(),
                outcome.skill_md_path.display()
            ),
            structured: Some(StructuredToolResult::ToolError {
                tool_name: "create_skill_scaffold".to_string(),
                message: format!(
                    "created skill scaffold `{}` at `{}`",
                    outcome.skill_name,
                    outcome.skill_dir.display()
                ),
            }),
        })
    }
}

#[async_trait]
impl LocalTool for ValidateSkillLocalTool {
    fn spec(&self) -> ToolSpec {
        ValidateSkillTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ValidateSkillArgs = invocation.payload.parse_arguments()?;
        let skill_dir = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.path.as_str()),
        )?;
        let report = validate_skill_dir(&skill_dir)?;
        Ok(ToolInvocationOutput {
            content: format!(
                "Validated skill `{}` at `{}`.",
                report.skill_name,
                report.skill_dir.display()
            ),
            structured: Some(StructuredToolResult::ToolError {
                tool_name: "validate_skill".to_string(),
                message: format!(
                    "validated skill `{}` at `{}`",
                    report.skill_name,
                    report.skill_dir.display()
                ),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{LocalToolPayload, LocalToolSource};
    use agent_core::PermissionProfile;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn create_skill_scaffold_tool_creates_normalized_skill_dir() {
        let base = test_workspace("create_skill_scaffold_tool");
        let tool = CreateSkillScaffoldLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("create_skill_scaffold"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "name": "Repo Reader",
                            "path": ".cloudagent/skills",
                            "resources": ["scripts", "references"]
                        }),
                    },
                },
                &tool_context(&base, PermissionProfile::WorkspaceWrite),
            )
            .await
            .expect("create skill scaffold works");

        assert!(
            base.join(".cloudagent/skills/repo-reader/SKILL.md")
                .is_file()
        );
        assert!(output.content.contains("repo-reader"));
    }

    #[tokio::test]
    async fn validate_skill_tool_accepts_generated_skill_dir() {
        let base = test_workspace("validate_skill_tool");
        let skill_dir = base.join(".cloudagent/skills/repo-reader");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: repo-reader\ndescription: demo\n---\n\n# Repo Reader\n",
        )
        .expect("write skill");

        let tool = ValidateSkillLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("validate_skill"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": ".cloudagent/skills/repo-reader"
                        }),
                    },
                },
                &tool_context(&base, PermissionProfile::ReadOnly),
            )
            .await
            .expect("validate skill works");

        assert!(output.content.contains("Validated skill `repo-reader`"));
    }

    fn tool_context(
        workspace_root: &std::path::Path,
        permission_profile: PermissionProfile,
    ) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn test_workspace(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        path.push(format!("cloudagent_{name}_{stamp}"));
        fs::create_dir_all(&path).expect("create temp workspace");
        path
    }
}
