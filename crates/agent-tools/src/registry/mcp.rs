use super::shared::{LocalToolInvocation, LocalToolPayload, LocalToolSource, ToolInvocationOutput};
use agent_core::{ToolExecutionPolicy, ToolSource, ToolSpec};
use agent_protocol::StructuredToolResult;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct McpToolDescriptor {
    pub wire_name: String,
    pub server: String,
    pub tool: String,
    pub spec: ToolSpec,
}

impl McpToolDescriptor {
    pub fn new(
        wire_name: String,
        server: String,
        tool: String,
        mut spec: ToolSpec,
        supports_parallel_calls: bool,
    ) -> Self {
        spec.identity =
            agent_core::ToolIdentity::mcp(server.clone(), tool.clone(), wire_name.clone());
        spec.execution_policy = if supports_parallel_calls && !spec.mutating {
            ToolExecutionPolicy::ParallelSafe
        } else {
            ToolExecutionPolicy::Sequential
        };
        Self {
            wire_name,
            server,
            tool,
            spec,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RoutedMcpTool {
    pub(crate) display_name: String,
    pub(crate) invocation: LocalToolInvocation,
}

#[derive(Clone, Debug)]
pub struct McpToolInvocation {
    pub wire_name: String,
    pub server: String,
    pub tool: String,
    pub arguments: Value,
}

#[derive(Clone, Debug)]
pub struct McpToolResponse {
    pub content: String,
    pub structured: Option<StructuredToolResult>,
}

#[async_trait]
pub trait McpToolClient: Send + Sync {
    async fn call_tool(&self, invocation: McpToolInvocation) -> Result<McpToolResponse>;
}

#[derive(Clone, Default)]
pub(crate) struct McpRegistry {
    descriptors: BTreeMap<String, McpToolDescriptor>,
    client: Option<Arc<dyn McpToolClient>>,
}

impl McpRegistry {
    pub(crate) fn register(&mut self, descriptor: McpToolDescriptor) {
        self.descriptors
            .insert(descriptor.wire_name.clone(), descriptor);
    }

    pub(crate) fn set_client(&mut self, client: Arc<dyn McpToolClient>) {
        self.client = Some(client);
    }

    pub(crate) fn resolve(&self, wire_name: &str, arguments: Value) -> Option<RoutedMcpTool> {
        let descriptor = self.descriptors.get(wire_name)?;
        Some(RoutedMcpTool {
            display_name: descriptor.spec.name.clone(),
            invocation: LocalToolInvocation {
                identity: descriptor.spec.identity.clone(),
                source: LocalToolSource::Mcp,
                payload: LocalToolPayload::Mcp {
                    server: descriptor.server.clone(),
                    tool: descriptor.tool.clone(),
                    arguments,
                },
            },
        })
    }

    pub(crate) fn descriptor_specs(&self) -> Vec<ToolSpec> {
        self.descriptors
            .values()
            .map(|descriptor| {
                debug_assert_eq!(descriptor.spec.identity.source, ToolSource::Mcp);
                debug_assert_eq!(descriptor.spec.identity.wire_name, descriptor.wire_name);
                descriptor.spec.clone()
            })
            .collect()
    }

    pub(crate) fn supports_parallel_tool(&self, wire_name: &str) -> bool {
        self.descriptors
            .get(wire_name)
            .is_some_and(|descriptor| descriptor.spec.execution_policy.supports_parallel())
    }

    pub(crate) async fn execute(
        &self,
        invocation: LocalToolInvocation,
    ) -> Result<ToolInvocationOutput> {
        let Some(client) = &self.client else {
            bail!("MCP tool client is not configured");
        };
        let (server, tool, arguments) = match &invocation.payload {
            LocalToolPayload::Mcp {
                server,
                tool,
                arguments,
            } => (server.clone(), tool.clone(), arguments.clone()),
            _ => bail!("non-MCP invocation reached MCP registry"),
        };
        let response = client
            .call_tool(McpToolInvocation {
                wire_name: invocation.identity.wire_name.clone(),
                server,
                tool,
                arguments,
            })
            .await?;
        Ok(ToolInvocationOutput {
            content: response.content,
            structured: response.structured,
        })
    }
}
