use crate::spec::{
    ToolCategory, ToolDescriptor, ToolLayer, ToolPermissionTier, ToolRisk, ToolUsageGuidance,
};
use agent_core::{ToolExecutionPolicy, ToolIdentity, ToolSpec, TurnItemDeltaKind, TurnItemKind};
use serde_json::json;

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::ExternalResources,
            ToolRisk::Medium,
            ToolPermissionTier::ReadOnly,
            vec!["research", "general"],
            ToolUsageGuidance {
                selection_priority: 30,
                preferred_for: vec![
                    "provider-hosted web lookups on OpenAI-compatible Responses APIs",
                    "questions that need fresh public internet information",
                ],
                avoid_for: vec![
                    "workspace file search",
                    "providers that only support chat completions or do not implement hosted tools",
                ],
                follow_up_hint: Some(
                    "use this only when the configured Responses provider supports hosted web search",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "web_search".to_string(),
                identity: ToolIdentity::built_in("web_search"),
                description:
                    "Use a provider-hosted web search tool when the configured OpenAI-compatible Responses endpoint supports it."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::Coordination)
    }
}
