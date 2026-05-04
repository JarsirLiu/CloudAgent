mod catalog;
mod presentation;
mod resolution;
pub(crate) mod shared;

use crate::selection::ToolSelector;
use crate::spec::ToolDescriptor;
use agent_core::{
    TaskKind, ToolCall, ToolExecutionContext, ToolExecutor, ToolMode, ToolResult, ToolSpec,
    ToolSurface,
};
use anyhow::{Result, bail};
use async_trait::async_trait;

use catalog::{LocalToolMap, build_descriptors, build_selector, build_tools};
pub use resolution::{ResolvedToolSet, ToolBatchExecutionStrategy};
use crate::policy::{ApprovalRequirement, approval_requirement_for_tool};
use presentation::{
    default_rejection_message, denied_transcript_item, missing_tool_result,
    repeated_rejection_message, tool_item_title, tool_request_key,
    transcript_item_from_tool_result,
};
use shared::{LocalToolInvocation, LocalToolPayload, LocalToolSource, structured_failure_result};
use agent_protocol::PermissionProfile;
use agent_protocol::ApprovalPolicy;
use agent_protocol::TranscriptItem;
use std::path::Path;

#[derive(Clone)]
pub struct ToolRegistry {
    tools: LocalToolMap,
    descriptors: Vec<ToolDescriptor>,
    selector: ToolSelector,
}

#[derive(Clone, Debug)]
enum LocalToolRouteTarget {
    BuiltIn(String),
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
            descriptors: build_descriptors(max_read_chars),
            selector: build_selector(),
        }
    }

    pub fn specs_for_mode(&self, mode: ToolMode, task_kind: TaskKind) -> Vec<ToolSpec> {
        resolution::specs_for_surface(self.selector.select(&mode, &task_kind, &self.descriptors))
    }

    pub fn specs_for_surface(&self, tool_surface: &ToolSurface) -> Vec<ToolSpec> {
        self.specs_for_mode(tool_surface.mode.clone(), tool_surface.task_kind.clone())
    }

    pub fn resolve_surface(
        &self,
        tool_surface: &ToolSurface,
        permission_profile: &PermissionProfile,
    ) -> ResolvedToolSet {
        resolution::resolve_surface(
            self.selector
                .select(&tool_surface.mode, &tool_surface.task_kind, &self.descriptors),
            permission_profile,
        )
    }

    pub fn tool_supports_parallel(&self, tool_name: &str) -> bool {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.spec.name == tool_name)
            .is_some_and(|descriptor| descriptor.supports_parallel_calls)
    }

    pub fn batch_execution_strategy(&self, calls: &[ToolCall]) -> ToolBatchExecutionStrategy {
        resolution::batch_execution_strategy(&self.descriptors, calls)
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
        if self.tools.contains_key(&call.name) {
            return Ok(LocalToolRoute {
                display_name: call.name.clone(),
                target: LocalToolRouteTarget::BuiltIn(call.name.clone()),
                invocation: LocalToolInvocation {
                    tool_name: call.name.clone(),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: call.arguments.clone(),
                    },
                },
            });
        }

        bail!("tool `{}` is not registered", call.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::ToolSurface;

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
                arguments: serde_json::json!({"mode": "text", "query": "foo"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "read_files".to_string(),
                arguments: serde_json::json!({"path": "src/main.rs"}),
            },
        ];
        let sequential_calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "search_workspace".to_string(),
                arguments: serde_json::json!({"mode": "text", "query": "foo"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "exec_command".to_string(),
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
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    fn specs(&self) -> Vec<ToolSpec> {
        self.descriptors
            .iter()
            .map(|descriptor| descriptor.spec.clone())
            .collect()
    }

    fn specs_for_surface(&self, tool_surface: &ToolSurface) -> Vec<ToolSpec> {
        ToolRegistry::specs_for_surface(self, tool_surface)
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let route = self.route_call(&call)?;
        let call_name = route.display_name.clone();
        let LocalToolRouteTarget::BuiltIn(registry_name) = &route.target;
        let tool = self
            .tools
            .get(registry_name)
            .expect("routed built-in tool should exist in registry");

        match tool.invoke(route.invocation.clone(), ctx).await {
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
