mod catalog;
mod mcp;
mod presentation;
mod resolution;
pub(crate) mod shared;

use crate::spec::ToolDescriptor;
use agent_core::{
    ApprovalPolicy, ApprovalRequirement, PermissionProfile, RegularTurnToolExposure, ToolBackend,
    ToolBatchExecutionStrategy, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, ToolSpec,
};
use anyhow::{Result, bail};
use async_trait::async_trait;

use crate::policy::{approval_grant_key_for_tool, approval_requirement_for_tool};
use agent_protocol::TranscriptItem;
use catalog::{LocalToolMap, build_descriptors, build_tools};
use mcp::McpRegistry;
pub use mcp::{McpToolClient, McpToolDescriptor, McpToolInvocation, McpToolResponse};
use presentation::{
    default_rejection_message, denied_transcript_item, missing_tool_result,
    repeated_rejection_message, tool_item_title, tool_request_key,
    transcript_item_from_tool_result,
};
use shared::{LocalToolInvocation, LocalToolPayload, LocalToolSource, structured_failure_result};
use std::path::Path;

#[derive(Clone)]
pub struct ToolRegistry {
    tools: LocalToolMap,
    mcp: McpRegistry,
    descriptors: Vec<ToolDescriptor>,
}

#[derive(Clone, Debug)]
enum LocalToolRouteTarget {
    BuiltIn(String),
    Mcp,
}

#[derive(Clone, Debug)]
struct LocalToolRoute {
    display_name: String,
    target: LocalToolRouteTarget,
    invocation: LocalToolInvocation,
}

impl ToolRegistry {
    pub fn new(max_read_chars: usize) -> Self {
        Self {
            tools: build_tools(max_read_chars),
            mcp: McpRegistry::default(),
            descriptors: build_descriptors(max_read_chars),
        }
    }

    pub fn register_mcp_tool(&mut self, descriptor: McpToolDescriptor) {
        self.mcp.register(descriptor);
    }

    pub fn set_mcp_client(&mut self, client: std::sync::Arc<dyn McpToolClient>) {
        self.mcp.set_client(client);
    }

    pub fn resolve_regular_turn_tool_exposure(
        &self,
        permission_profile: &PermissionProfile,
    ) -> RegularTurnToolExposure {
        let mcp_client_is_configured = self.mcp.client_is_configured();
        let discoverable_count = self
            .descriptors
            .iter()
            .filter(|descriptor| {
                descriptor.default_visibility == crate::spec::ToolDefaultVisibility::Deferred
            })
            .count()
            + if mcp_client_is_configured {
                self.mcp
                    .registered_descriptors()
                    .into_iter()
                    .filter(|descriptor| {
                        descriptor.default_visibility
                            == crate::spec::ToolDefaultVisibility::Deferred
                    })
                    .count()
            } else {
                0
            };
        let registered = resolution::registered_tools(
            &self.descriptors,
            self.mcp.registered_descriptors(),
            mcp_client_is_configured,
            discoverable_count > 0,
        );
        let environment_visible = resolution::environment_visible_tools(registered);
        let permission_allowed =
            resolution::permission_allowed_tools(environment_visible, permission_profile);
        let default_visible = resolution::model_visible_default_tools(permission_allowed.clone());
        let deferred_visible = resolution::deferred_discoverable_tools(permission_allowed);
        RegularTurnToolExposure {
            default_tools: resolution::specs_for_registered_tools(default_visible),
            deferred_tools: resolution::specs_for_registered_tools(deferred_visible),
        }
    }

    pub fn tool_supports_parallel(&self, tool_name: &str) -> bool {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.spec.identity.wire_name == tool_name)
            .is_some_and(|descriptor| descriptor.spec.execution_policy.supports_parallel())
            || self.mcp.supports_parallel_tool(tool_name)
    }

    pub fn batch_execution_strategy(&self, calls: &[ToolCall]) -> ToolBatchExecutionStrategy {
        if calls
            .iter()
            .all(|call| self.tools.contains_key(&call.identity.wire_name))
        {
            return resolution::batch_execution_strategy(&self.descriptors, calls);
        }
        if calls.len() > 1
            && calls.iter().all(|call| {
                self.descriptors
                    .iter()
                    .find(|descriptor| {
                        descriptor.spec.identity.wire_name == call.identity.wire_name
                    })
                    .is_some_and(|descriptor| descriptor.spec.execution_policy.supports_parallel())
                    || self.mcp.supports_parallel_tool(&call.identity.wire_name)
            })
        {
            ToolBatchExecutionStrategy::Parallel
        } else {
            ToolBatchExecutionStrategy::Sequential
        }
    }

    pub fn approval_requirement_for_call(
        &self,
        spec: &ToolSpec,
        call: &ToolCall,
        workspace_root: &Path,
        permission_profile: &PermissionProfile,
        approval_policy: &ApprovalPolicy,
    ) -> ApprovalRequirement {
        approval_requirement_for_tool(
            spec,
            call,
            workspace_root,
            permission_profile,
            approval_policy,
        )
    }

    pub fn approval_grant_key_for_call(
        &self,
        spec: &ToolSpec,
        call: &ToolCall,
        workspace_root: &Path,
        permission_profile: &PermissionProfile,
        approval_policy: &ApprovalPolicy,
    ) -> Option<agent_core::ApprovalGrantKey> {
        approval_grant_key_for_tool(
            spec,
            call,
            workspace_root,
            permission_profile,
            approval_policy,
        )
    }

    pub fn tool_item_title(&self, call: &ToolCall) -> String {
        tool_item_title(call)
    }

    pub fn transcript_item_from_result(
        &self,
        item_id: &str,
        call: &ToolCall,
        result: &ToolResult,
    ) -> TranscriptItem {
        transcript_item_from_tool_result(item_id, &call.name, result)
    }

    pub fn denied_transcript_item(
        &self,
        item_id: &str,
        call: &ToolCall,
        reason: &str,
    ) -> TranscriptItem {
        denied_transcript_item(item_id, &call.name, &call.arguments, reason)
    }

    pub fn default_rejection_message(&self, tool_name: &str) -> String {
        default_rejection_message(tool_name)
    }

    pub fn repeated_rejection_message(&self, tool_name: &str) -> String {
        repeated_rejection_message(tool_name)
    }

    pub fn denied_structured_result(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        reason: String,
    ) -> Option<agent_protocol::StructuredToolResult> {
        presentation::denied_tool_result(tool_name, arguments, reason)
    }

    pub fn tool_request_key(&self, call: &ToolCall) -> String {
        tool_request_key(call)
    }

    pub fn missing_tool_result(&self, call: &ToolCall) -> ToolResult {
        missing_tool_result(call)
    }

    fn route_call(&self, call: &ToolCall) -> Result<LocalToolRoute> {
        if self.tools.contains_key(&call.identity.wire_name) {
            return Ok(LocalToolRoute {
                display_name: call.name.clone(),
                target: LocalToolRouteTarget::BuiltIn(call.identity.wire_name.clone()),
                invocation: LocalToolInvocation {
                    identity: call.identity.clone(),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: call.arguments.clone(),
                    },
                },
            });
        }

        if let Some(routed) = self
            .mcp
            .resolve(&call.identity.wire_name, call.arguments.clone())
        {
            return Ok(LocalToolRoute {
                display_name: routed.display_name,
                target: LocalToolRouteTarget::Mcp,
                invocation: routed.invocation,
            });
        }

        bail!("tool `{}` is not registered", call.identity.wire_name)
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use agent_core::{ToolExecutionContext, TurnItemDeltaKind, TurnItemKind};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn resolve_exposure_filters_tools_by_permission_profile() {
        let registry = ToolRegistry::new(4_096);
        let read_only = registry.resolve_regular_turn_tool_exposure(&PermissionProfile::ReadOnly);
        let workspace_write =
            registry.resolve_regular_turn_tool_exposure(&PermissionProfile::WorkspaceWrite);

        assert!(
            read_only
                .default_tools
                .iter()
                .all(|spec| spec.name != "edit_file")
        );
        assert!(
            workspace_write
                .default_tools
                .iter()
                .any(|spec| spec.name == "edit_file")
        );
        assert!(
            read_only
                .default_tools
                .iter()
                .any(|spec| spec.name == "exec_command")
        );
        assert!(
            read_only
                .default_tools
                .iter()
                .all(|spec| spec.name != "watch")
        );
        assert!(
            read_only
                .default_tools
                .iter()
                .all(|spec| spec.name != "watch")
        );
        assert!(
            read_only
                .deferred_tools
                .iter()
                .any(|spec| spec.name == "watch")
        );
    }

    #[test]
    fn regular_turn_default_visible_set_uses_default_priority_ordering() {
        let registry = ToolRegistry::new(4_096);
        let resolved = registry
            .resolve_regular_turn_tool_exposure(&PermissionProfile::ReadOnly)
            .default_tools;

        let ordered_names = resolved
            .iter()
            .map(|spec| spec.name.as_str())
            .take(3)
            .collect::<Vec<_>>();

        assert_eq!(
            ordered_names,
            vec!["search_workspace", "read_file", "exec_command"]
        );
    }

    #[test]
    fn workspace_write_keeps_repo_exploration_ahead_of_editing() {
        let registry = ToolRegistry::new(4_096);
        let resolved = registry
            .resolve_regular_turn_tool_exposure(&PermissionProfile::WorkspaceWrite)
            .default_tools;

        let ordered_names = resolved
            .iter()
            .map(|spec| spec.name.as_str())
            .take(5)
            .collect::<Vec<_>>();

        assert_eq!(
            ordered_names,
            vec![
                "search_workspace",
                "read_file",
                "edit_file",
                "exec_command",
                "tool_search",
            ]
        );
    }

    #[test]
    fn regular_turn_hides_mcp_tools_without_a_configured_client() {
        let mut registry = ToolRegistry::new(4_096);
        registry.register_mcp_tool(McpToolDescriptor::new(
            "mcp__demo__lookup".to_string(),
            "demo".to_string(),
            "lookup".to_string(),
            ToolSpec {
                name: "mcp__demo__lookup".to_string(),
                identity: agent_core::ToolIdentity::mcp(
                    "demo".to_string(),
                    "lookup".to_string(),
                    "mcp__demo__lookup".to_string(),
                ),
                description: "demo mcp tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
                mutating: false,
                execution_policy: agent_core::ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        ));

        let resolved = registry
            .resolve_regular_turn_tool_exposure(&PermissionProfile::ReadOnly)
            .default_tools;

        assert!(resolved.iter().all(|spec| spec.name != "mcp__demo__lookup"));
    }

    #[test]
    fn regular_turn_shows_mcp_tools_only_when_the_client_is_configured() {
        let mut registry = ToolRegistry::new(4_096);
        registry.register_mcp_tool(McpToolDescriptor::new(
            "mcp__demo__lookup".to_string(),
            "demo".to_string(),
            "lookup".to_string(),
            ToolSpec {
                name: "mcp__demo__lookup".to_string(),
                identity: agent_core::ToolIdentity::mcp(
                    "demo".to_string(),
                    "lookup".to_string(),
                    "mcp__demo__lookup".to_string(),
                ),
                description: "demo mcp tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
                mutating: false,
                execution_policy: agent_core::ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        ));
        registry.set_mcp_client(Arc::new(FakeMcpClient));

        let resolved = registry
            .resolve_regular_turn_tool_exposure(&PermissionProfile::ReadOnly)
            .default_tools;

        assert!(resolved.iter().any(|spec| spec.name == "mcp__demo__lookup"));
    }

    #[test]
    fn regular_turn_exposes_tool_search_when_deferred_tools_exist() {
        let registry = ToolRegistry::new(4_096);
        let exposure = registry.resolve_regular_turn_tool_exposure(&PermissionProfile::ReadOnly);

        assert!(
            exposure
                .default_tools
                .iter()
                .any(|spec| spec.name == "tool_search")
        );
        assert!(
            exposure
                .deferred_tools
                .iter()
                .any(|spec| spec.name == "watch")
        );
    }

    #[test]
    fn regular_turn_puts_deferred_mcp_tools_in_the_discoverable_set() {
        let mut registry = ToolRegistry::new(4_096);
        registry.register_mcp_tool(
            McpToolDescriptor::new(
                "mcp__demo__bytes".to_string(),
                "demo".to_string(),
                "bytes".to_string(),
                ToolSpec {
                    name: "mcp__demo__bytes".to_string(),
                    identity: agent_core::ToolIdentity::mcp(
                        "demo".to_string(),
                        "bytes".to_string(),
                        "mcp__demo__bytes".to_string(),
                    ),
                    description: "read raw bytes from external storage".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                    mutating: false,
                    execution_policy: agent_core::ToolExecutionPolicy::Sequential,
                    requires_approval: false,
                    item_kind: TurnItemKind::ToolCall,
                    delta_kind: TurnItemDeltaKind::ToolOutput,
                    approval_reason: None,
                },
            )
            .with_default_visibility(crate::spec::ToolDefaultVisibility::Deferred)
            .with_selection_priority(1),
        );
        registry.set_mcp_client(Arc::new(FakeMcpClient));

        let exposure = registry.resolve_regular_turn_tool_exposure(&PermissionProfile::ReadOnly);

        assert!(
            exposure
                .default_tools
                .iter()
                .any(|spec| spec.name == "tool_search")
        );
        assert!(
            exposure
                .default_tools
                .iter()
                .all(|spec| spec.name != "mcp__demo__bytes")
        );
        assert!(
            exposure
                .deferred_tools
                .iter()
                .any(|spec| spec.name == "mcp__demo__bytes")
        );
    }

    #[test]
    fn batch_execution_strategy_prefers_parallel_only_for_safe_batches() {
        let registry = ToolRegistry::new(4_096);
        let parallel_calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "search_workspace".to_string(),
                identity: agent_core::ToolIdentity::built_in("search_workspace"),
                arguments: serde_json::json!({"mode": "text", "query": "foo"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "read_file".to_string(),
                identity: agent_core::ToolIdentity::built_in("read_file"),
                arguments: serde_json::json!({"path": "src/main.rs"}),
            },
        ];
        let sequential_calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "search_workspace".to_string(),
                identity: agent_core::ToolIdentity::built_in("search_workspace"),
                arguments: serde_json::json!({"mode": "text", "query": "foo"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "exec_command".to_string(),
                identity: agent_core::ToolIdentity::built_in("exec_command"),
                arguments: serde_json::json!({"command": "git status"}),
            },
        ];

        assert_eq!(
            registry.batch_execution_strategy(&parallel_calls),
            ToolBatchExecutionStrategy::Parallel
        );
        assert_eq!(
            registry.batch_execution_strategy(&sequential_calls),
            ToolBatchExecutionStrategy::Sequential
        );
    }

    struct FakeMcpClient;

    #[async_trait]
    impl McpToolClient for FakeMcpClient {
        async fn call_tool(&self, invocation: McpToolInvocation) -> Result<McpToolResponse> {
            Ok(McpToolResponse {
                content: format!("mcp:{}:{}", invocation.server, invocation.tool),
                structured: Some(agent_protocol::StructuredToolResult::ToolError {
                    tool_name: invocation.wire_name,
                    message: "stub".to_string(),
                }),
            })
        }
    }

    #[tokio::test]
    async fn execute_routes_registered_mcp_tools_through_mcp_client() {
        let mut registry = ToolRegistry::new(4_096);
        registry.register_mcp_tool(McpToolDescriptor::new(
            "mcp__demo__lookup".to_string(),
            "demo".to_string(),
            "lookup".to_string(),
            ToolSpec {
                name: "mcp__demo__lookup".to_string(),
                identity: agent_core::ToolIdentity::mcp(
                    "demo".to_string(),
                    "lookup".to_string(),
                    "mcp__demo__lookup".to_string(),
                ),
                description: "demo mcp tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
                mutating: false,
                execution_policy: agent_core::ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        ));
        registry.set_mcp_client(Arc::new(FakeMcpClient));

        let result = registry
            .execute(
                ToolCall {
                    id: "call-1".to_string(),
                    name: "mcp__demo__lookup".to_string(),
                    identity: agent_core::ToolIdentity::mcp(
                        "demo".to_string(),
                        "lookup".to_string(),
                        "mcp__demo__lookup".to_string(),
                    ),
                    arguments: serde_json::json!({"q": "ping"}),
                },
                &ToolExecutionContext {
                    conversation_id: "test".to_string(),
                    workspace_root: std::env::temp_dir(),
                    conversation_store_dir: std::env::temp_dir(),
                    permission_profile: PermissionProfile::ReadOnly,
                    default_shell_timeout_ms: 5_000,
                    cancellation_token: CancellationToken::new(),
                    discoverable_tools: Vec::new(),
                    output_tx: None,
                },
            )
            .await
            .expect("mcp tool routed");

        assert_eq!(result.content, "mcp:demo:lookup");
        assert!(matches!(
            result.structured,
            Some(agent_protocol::StructuredToolResult::ToolError { tool_name, .. })
                if tool_name == "mcp__demo__lookup"
        ));
    }
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    fn specs(&self) -> Vec<ToolSpec> {
        let mut specs = self
            .resolve_regular_turn_tool_exposure(&PermissionProfile::FullAccess)
            .default_tools;
        specs.extend(
            self.resolve_regular_turn_tool_exposure(&PermissionProfile::FullAccess)
                .deferred_tools,
        );
        specs
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let route = self.route_call(&call)?;
        let call_name = route.display_name.clone();
        let result = match &route.target {
            LocalToolRouteTarget::BuiltIn(registry_name) => {
                let tool = self
                    .tools
                    .get(registry_name)
                    .expect("routed built-in tool should exist in registry");
                tool.invoke(route.invocation.clone(), ctx).await
            }
            LocalToolRouteTarget::Mcp => self.mcp.execute(route.invocation.clone()).await,
        };

        match result {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: output.content,
                is_error: false,
                structured: output.structured,
            }),
            Err(err) => {
                let message = format!("Tool execution failed: {err:#}");
                let structured = match structured_failure_result(&route.invocation) {
                    Some(agent_protocol::StructuredToolResult::ToolError { .. }) => {
                        Some(agent_protocol::StructuredToolResult::ToolError {
                            tool_name: call_name.clone(),
                            message: message.clone(),
                        })
                    }
                    other => other,
                };
                Ok(ToolResult {
                    tool_call_id: call.id,
                    name: call.name,
                    content: message,
                    is_error: true,
                    structured,
                })
            }
        }
    }
}

impl ToolBackend for ToolRegistry {
    type PermissionProfile = PermissionProfile;
    type ApprovalPolicy = ApprovalPolicy;

    fn resolve_regular_turn_tool_exposure(
        &self,
        permission_profile: &Self::PermissionProfile,
    ) -> RegularTurnToolExposure {
        ToolRegistry::resolve_regular_turn_tool_exposure(self, permission_profile)
    }

    fn batch_execution_strategy(&self, calls: &[ToolCall]) -> ToolBatchExecutionStrategy {
        ToolRegistry::batch_execution_strategy(self, calls)
    }

    fn approval_requirement_for_call(
        &self,
        spec: &ToolSpec,
        call: &ToolCall,
        workspace_root: &std::path::Path,
        permission_profile: &Self::PermissionProfile,
        approval_policy: &Self::ApprovalPolicy,
    ) -> ApprovalRequirement {
        ToolRegistry::approval_requirement_for_call(
            self,
            spec,
            call,
            workspace_root,
            permission_profile,
            approval_policy,
        )
    }

    fn approval_grant_key_for_call(
        &self,
        spec: &ToolSpec,
        call: &ToolCall,
        workspace_root: &Path,
        permission_profile: &Self::PermissionProfile,
        approval_policy: &Self::ApprovalPolicy,
    ) -> Option<agent_core::ApprovalGrantKey> {
        ToolRegistry::approval_grant_key_for_call(
            self,
            spec,
            call,
            workspace_root,
            permission_profile,
            approval_policy,
        )
    }

    fn tool_item_title(&self, call: &ToolCall) -> String {
        ToolRegistry::tool_item_title(self, call)
    }

    fn transcript_item_from_result(
        &self,
        item_id: &str,
        call: &ToolCall,
        result: &ToolResult,
    ) -> agent_core::TranscriptItem {
        ToolRegistry::transcript_item_from_result(self, item_id, call, result)
    }

    fn denied_transcript_item(
        &self,
        item_id: &str,
        call: &ToolCall,
        reason: &str,
    ) -> agent_core::TranscriptItem {
        ToolRegistry::denied_transcript_item(self, item_id, call, reason)
    }

    fn default_rejection_message(&self, tool_name: &str) -> String {
        ToolRegistry::default_rejection_message(self, tool_name)
    }

    fn repeated_rejection_message(&self, tool_name: &str) -> String {
        ToolRegistry::repeated_rejection_message(self, tool_name)
    }

    fn denied_structured_result(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        reason: String,
    ) -> Option<agent_core::StructuredToolResult> {
        ToolRegistry::denied_structured_result(self, tool_name, arguments, reason)
    }

    fn tool_request_key(&self, call: &ToolCall) -> String {
        ToolRegistry::tool_request_key(self, call)
    }

    fn missing_tool_result(&self, call: &ToolCall) -> ToolResult {
        ToolRegistry::missing_tool_result(self, call)
    }
}
