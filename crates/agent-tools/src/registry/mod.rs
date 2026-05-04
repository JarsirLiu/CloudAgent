mod catalog;
mod mcp;
mod presentation;
mod resolution;
pub(crate) mod shared;

use crate::selection::ToolSelector;
use crate::spec::ToolDescriptor;
use agent_core::{
    ApprovalPolicy, ApprovalRequirement, PermissionProfile, ResolvedToolSet, TaskKind,
    ToolBackend, ToolBatchExecutionStrategy, ToolCall, ToolExecutionContext, ToolExecutor,
    ToolMode, ToolResult, ToolSpec, ToolSurface,
};
use anyhow::{Result, bail};
use async_trait::async_trait;

use catalog::{LocalToolMap, build_descriptors, build_selector, build_tools};
use mcp::McpRegistry;
pub use mcp::{McpToolClient, McpToolDescriptor, McpToolInvocation, McpToolResponse};
use crate::policy::approval_requirement_for_tool;
use presentation::{
    default_rejection_message, denied_transcript_item, missing_tool_result,
    repeated_rejection_message, tool_item_title, tool_request_key,
    transcript_item_from_tool_result,
};
use shared::{LocalToolInvocation, LocalToolPayload, LocalToolSource, structured_failure_result};
use agent_protocol::TranscriptItem;
use std::path::Path;

#[derive(Clone)]
pub struct ToolRegistry {
    tools: LocalToolMap,
    mcp: McpRegistry,
    descriptors: Vec<ToolDescriptor>,
    selector: ToolSelector,
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
            selector: build_selector(),
        }
    }

    pub fn specs_for_mode(&self, mode: ToolMode, task_kind: TaskKind) -> Vec<ToolSpec> {
        let mut specs =
            resolution::specs_for_surface(self.selector.select(&mode, &task_kind, &self.descriptors));
        specs.extend(self.mcp.descriptor_specs());
        specs
    }

    pub fn specs_for_surface(&self, tool_surface: &ToolSurface) -> Vec<ToolSpec> {
        self.specs_for_mode(tool_surface.mode.clone(), tool_surface.task_kind.clone())
    }

    pub fn register_mcp_tool(&mut self, descriptor: McpToolDescriptor) {
        self.mcp.register(descriptor);
    }

    pub fn set_mcp_client(&mut self, client: std::sync::Arc<dyn McpToolClient>) {
        self.mcp.set_client(client);
    }

    pub fn resolve_surface(
        &self,
        tool_surface: &ToolSurface,
        permission_profile: &PermissionProfile,
    ) -> ResolvedToolSet {
        let mut resolved = resolution::resolve_surface(
            self.selector
                .select(&tool_surface.mode, &tool_surface.task_kind, &self.descriptors),
            permission_profile,
        );
        for spec in self.mcp.descriptor_specs() {
            if self.mcp.supports_parallel_tool(&spec.identity.wire_name) {
                resolved.mark_parallel_tool(spec.identity.wire_name.clone());
            }
            resolved.specs.push(spec);
        }
        resolved
    }

    pub fn tool_supports_parallel(&self, tool_name: &str) -> bool {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.spec.identity.wire_name == tool_name)
            .is_some_and(|descriptor| descriptor.supports_parallel_calls)
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
                    .find(|descriptor| descriptor.spec.identity.wire_name == call.identity.wire_name)
                    .is_some_and(|descriptor| {
                        !descriptor.spec.mutating && descriptor.supports_parallel_calls
                    })
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
mod tests {
    use super::*;
    use crate::selection::ToolSurface;
    use agent_core::{ToolExecutionContext, TurnItemDeltaKind, TurnItemKind};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn resolve_surface_filters_tools_by_permission_profile() {
        let registry = ToolRegistry::new(4_096);
        let surface = ToolSurface::regular_turn();

        let read_only = registry.resolve_surface(&surface, &PermissionProfile::ReadOnly);
        let workspace_write =
            registry.resolve_surface(&surface, &PermissionProfile::WorkspaceWrite);

        assert!(read_only.specs.iter().all(|spec| spec.name != "apply_patch"));
        assert!(workspace_write
            .specs
            .iter()
            .any(|spec| spec.name == "apply_patch"));
        assert!(read_only.specs.iter().any(|spec| spec.name == "exec_command"));
    }

    #[test]
    fn resolve_surface_tracks_parallel_safe_tools() {
        let registry = ToolRegistry::new(4_096);
        let surface = ToolSurface::regular_turn();
        let resolved = registry.resolve_surface(&surface, &PermissionProfile::ReadOnly);

        assert!(resolved.supports_parallel_tool("search_workspace"));
        assert!(resolved.supports_parallel_tool("read_files"));
        assert!(!resolved.supports_parallel_tool("exec_command"));
        assert!(!resolved.supports_parallel_tool("apply_patch"));
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
                    name: "read_files".to_string(),
                    identity: agent_core::ToolIdentity::built_in("read_files"),
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
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
            true,
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
                    default_shell_timeout_ms: 5_000,
                    cancellation_token: CancellationToken::new(),
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
            .descriptors
            .iter()
            .map(|descriptor| descriptor.spec.clone())
            .collect::<Vec<_>>();
        specs.extend(self.mcp.descriptor_specs());
        specs
    }

    fn specs_for_surface(&self, tool_surface: &ToolSurface) -> Vec<ToolSpec> {
        ToolRegistry::specs_for_surface(self, tool_surface)
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

    fn resolve_surface(
        &self,
        tool_surface: &ToolSurface,
        permission_profile: &Self::PermissionProfile,
    ) -> ResolvedToolSet {
        ToolRegistry::resolve_surface(self, tool_surface, permission_profile)
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
