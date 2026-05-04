use agent_core::{PermissionProfile, ToolSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCategory {
    RepositoryExploration,
    CommandExecution,
    WorkspaceFileOps,
    ExternalResources,
    AgentCoordination,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolPermissionTier {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl ToolPermissionTier {
    pub fn allows(&self, profile: &PermissionProfile) -> bool {
        let granted = match profile {
            PermissionProfile::ReadOnly => Self::ReadOnly,
            PermissionProfile::WorkspaceWrite => Self::WorkspaceWrite,
            PermissionProfile::FullAccess => Self::FullAccess,
        };
        granted >= *self
    }
}

#[derive(Clone, Debug)]
pub struct ToolDescriptor {
    pub category: ToolCategory,
    pub risk: ToolRisk,
    pub min_permission: ToolPermissionTier,
    pub supports_parallel_calls: bool,
    pub mode_tags: Vec<&'static str>,
    pub usage: ToolUsageGuidance,
    pub spec: ToolSpec,
}

#[derive(Clone, Debug, Default)]
pub struct ToolUsageGuidance {
    pub preferred_for: Vec<&'static str>,
    pub avoid_for: Vec<&'static str>,
    pub follow_up_hint: Option<&'static str>,
    pub if_truncated_hint: Option<&'static str>,
}

impl ToolDescriptor {
    pub fn new(
        category: ToolCategory,
        risk: ToolRisk,
        min_permission: ToolPermissionTier,
        supports_parallel_calls: bool,
        mode_tags: Vec<&'static str>,
        spec: ToolSpec,
    ) -> Self {
        Self::new_with_guidance(
            category,
            risk,
            min_permission,
            supports_parallel_calls,
            mode_tags,
            ToolUsageGuidance::default(),
            spec,
        )
    }

    pub fn new_with_guidance(
        category: ToolCategory,
        risk: ToolRisk,
        min_permission: ToolPermissionTier,
        supports_parallel_calls: bool,
        mode_tags: Vec<&'static str>,
        usage: ToolUsageGuidance,
        mut spec: ToolSpec,
    ) -> Self {
        spec.description = render_tool_description(&spec.description, &usage);
        Self {
            category,
            risk,
            min_permission,
            supports_parallel_calls,
            mode_tags,
            usage,
            spec,
        }
    }
}

fn render_tool_description(base: &str, usage: &ToolUsageGuidance) -> String {
    let mut sections = vec![base.trim().to_string()];

    if !usage.preferred_for.is_empty() {
        sections.push(format!(
            "Preferred for: {}.",
            usage.preferred_for.join("; ")
        ));
    }
    if !usage.avoid_for.is_empty() {
        sections.push(format!("Avoid for: {}.", usage.avoid_for.join("; ")));
    }
    if let Some(hint) = usage.follow_up_hint {
        sections.push(format!("Follow-up: {hint}."));
    }
    if let Some(hint) = usage.if_truncated_hint {
        sections.push(format!("If output is truncated: {hint}."));
    }

    sections.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ToolIdentity, TurnItemDeltaKind, TurnItemKind};
    use serde_json::json;

    #[test]
    fn tool_guidance_renders_into_description_consistently() {
        let descriptor = ToolDescriptor::new_with_guidance(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            true,
            vec!["explore"],
            ToolUsageGuidance {
                preferred_for: vec!["first-step discovery"],
                avoid_for: vec!["editing files"],
                follow_up_hint: Some("open the strongest hit next"),
                if_truncated_hint: Some("narrow the line range"),
            },
            ToolSpec {
                name: "demo".to_string(),
                identity: ToolIdentity::built_in("demo"),
                description: "Base description.".to_string(),
                parameters: json!({"type": "object"}),
                mutating: false,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        );

        assert!(descriptor.spec.description.contains("Base description."));
        assert!(
            descriptor
                .spec
                .description
                .contains("Preferred for: first-step discovery.")
        );
        assert!(
            descriptor
                .spec
                .description
                .contains("Avoid for: editing files.")
        );
        assert!(
            descriptor
                .spec
                .description
                .contains("Follow-up: open the strongest hit next.")
        );
        assert!(
            descriptor
                .spec
                .description
                .contains("If output is truncated: narrow the line range.")
        );
    }
}
