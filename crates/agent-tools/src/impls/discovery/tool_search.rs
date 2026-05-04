use crate::registry::shared::{LocalTool, LocalToolInvocation, ToolInvocationOutput};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolEnvironmentRequirement,
    ToolPermissionTier, ToolRisk, ToolUsageGuidance,
};
use agent_core::{
    StructuredToolResult, ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSearchHit,
    ToolSpec,
};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

pub struct ToolSearchTool;

impl ToolSearchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::AgentCoordination,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["explore", "edit", "verify", "general"],
            ToolUsageGuidance {
                selection_priority: 10,
                preferred_for: vec![
                    "discovering deferred tools that are not on the default visible set",
                    "finding a specialized filesystem or external tool by capability",
                ],
                avoid_for: vec![
                    "normal repository search",
                    "reading code when `read_file` or `search_workspace` already fits",
                ],
                follow_up_hint: Some(
                    "call this when the default tools do not fit; matching deferred tools become visible on the next model roundtrip",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "tool_search".to_string(),
                identity: ToolIdentity::built_in("tool_search"),
                description:
                    "Search deferred tools that are available in the current environment and allowed by the current permission profile."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "max_results": { "type": "integer", "minimum": 1, "maximum": 20 }
                    },
                    "required": ["query"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_environment_requirement(ToolEnvironmentRequirement::RequiresDiscoverableTools)
        .with_default_visibility(ToolDefaultVisibility::Default)
    }
}

#[derive(Debug, Deserialize)]
struct ToolSearchArgs {
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
}

pub(crate) struct ToolSearchLocalTool;

#[async_trait]
impl LocalTool for ToolSearchLocalTool {
    fn spec(&self) -> ToolSpec {
        ToolSearchTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ToolSearchArgs = invocation.payload.parse_arguments()?;
        let query = args.query.trim();
        let max_results = args.max_results.unwrap_or(8).clamp(1, 20);

        let mut hits = ctx
            .discoverable_tools
            .iter()
            .filter_map(|spec| score_tool(query, spec))
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.tool_name.cmp(&right.1.tool_name))
        });
        hits.truncate(max_results);

        let hits = hits
            .into_iter()
            .enumerate()
            .map(|(idx, (_, mut hit))| {
                hit.rank = idx + 1;
                hit
            })
            .collect::<Vec<_>>();

        let structured = StructuredToolResult::ToolSearch {
            query: query.to_string(),
            max_results,
            match_count: hits.len(),
            hits: hits.clone(),
        };
        let content = if hits.is_empty() {
            format!("No deferred tools matched `{query}`.")
        } else {
            let lines = hits
                .iter()
                .map(|hit| format!("{}. {} - {}", hit.rank, hit.tool_name, hit.match_reason))
                .collect::<Vec<_>>()
                .join("\n");
            format!("Deferred tool matches for `{query}`:\n{lines}")
        };

        Ok(ToolInvocationOutput {
            content,
            structured: Some(structured),
        })
    }
}

fn score_tool(query: &str, spec: &ToolSpec) -> Option<(i32, ToolSearchHit)> {
    let lowered_query = query.to_ascii_lowercase();
    if lowered_query.is_empty() {
        return None;
    }

    let name = spec.name.to_ascii_lowercase();
    let description = spec.description.to_ascii_lowercase();

    let (score, match_reason) = if name == lowered_query {
        (300, "exact tool name match")
    } else if name.contains(&lowered_query) {
        (220, "tool name match")
    } else if description.contains(&lowered_query) {
        (140, "tool description match")
    } else {
        let query_terms = lowered_query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        let term_hits = query_terms
            .iter()
            .filter(|term| name.contains(**term) || description.contains(**term))
            .count();
        if term_hits == 0 {
            return None;
        }
        (80 + (term_hits as i32 * 10), "keyword match")
    };

    Some((
        score,
        ToolSearchHit {
            tool_name: spec.name.clone(),
            source: spec.identity.source.clone(),
            description: spec.description.clone(),
            mutating: spec.mutating,
            rank: 0,
            match_reason: match_reason.to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{PermissionProfile, ToolSource};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn tool_search_returns_matching_deferred_tools() {
        let tool = ToolSearchLocalTool;
        let result = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("tool_search"),
                    source: crate::registry::shared::LocalToolSource::BuiltIn,
                    payload: crate::registry::shared::LocalToolPayload::Function {
                        arguments: json!({"query": "bytes"}),
                    },
                },
                &ToolExecutionContext {
                    conversation_id: "test".to_string(),
                    workspace_root: std::env::temp_dir(),
                    conversation_store_dir: std::env::temp_dir(),
                    permission_profile: PermissionProfile::ReadOnly,
                    default_shell_timeout_ms: 5_000,
                    cancellation_token: CancellationToken::new(),
                    discoverable_tools: vec![ToolSpec {
                        name: "read_file_bytes".to_string(),
                        identity: ToolIdentity {
                            source: ToolSource::BuiltIn,
                            namespace: None,
                            wire_name: "read_file_bytes".to_string(),
                        },
                        description: "Read raw bytes from one known file.".to_string(),
                        parameters: json!({"type": "object"}),
                        mutating: false,
                        execution_policy: ToolExecutionPolicy::Sequential,
                        requires_approval: false,
                        item_kind: agent_protocol::TurnItemKind::ToolCall,
                        delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                        approval_reason: None,
                    }],
                    output_tx: None,
                },
            )
            .await
            .expect("tool search should succeed");

        let StructuredToolResult::ToolSearch { hits, .. } =
            result.structured.expect("structured result expected")
        else {
            panic!("expected tool_search result");
        };

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool_name, "read_file_bytes");
    }
}
