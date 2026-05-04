use crate::registry::shared::{
    LocalToolInvocation, LocalToolPayload, LocalToolSource, ToolInvocationOutput,
};
use agent_core::{
    McpCallResult, ToolExecutionPolicy, ToolIdentity, ToolSource, ToolSpec, TurnItemDeltaKind,
    TurnItemKind,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct McpServerDefinition {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub startup_timeout: Duration,
    pub supports_parallel_tool_calls: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct McpDiscoveredTool {
    pub tool: String,
    pub description: String,
    pub parameters: Value,
    pub mutating: bool,
    pub requires_approval: bool,
    pub item_kind: TurnItemKind,
    pub delta_kind: TurnItemDeltaKind,
    pub approval_reason: Option<String>,
}

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
        spec.identity = ToolIdentity::mcp(server.clone(), tool.clone(), wire_name.clone());
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

impl McpDiscoveredTool {
    pub fn into_descriptor(self, server: &McpServerDefinition) -> McpToolDescriptor {
        let wire_name = default_wire_name(&server.name, &self.tool);
        McpToolDescriptor::new(
            wire_name.clone(),
            server.name.clone(),
            self.tool,
            ToolSpec {
                name: wire_name,
                identity: ToolIdentity::mcp(server.name.clone(), "", ""),
                description: self.description,
                parameters: self.parameters,
                mutating: self.mutating,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: self.requires_approval,
                item_kind: self.item_kind,
                delta_kind: self.delta_kind,
                approval_reason: self.approval_reason,
            },
            server.supports_parallel_tool_calls,
        )
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
    pub result: McpCallResult,
}

#[async_trait]
pub trait McpClient: Send + Sync {
    async fn list_tools(&self, server: &McpServerDefinition) -> Result<Vec<McpDiscoveredTool>>;
    async fn call_tool(&self, invocation: McpToolInvocation) -> Result<McpToolResponse>;
}

#[derive(Default)]
struct McpState {
    servers: BTreeMap<String, McpServerDefinition>,
    manual_descriptors: BTreeMap<String, McpToolDescriptor>,
    discovered_descriptors: BTreeMap<String, McpToolDescriptor>,
    client: Option<Arc<dyn McpClient>>,
}

#[derive(Clone, Default)]
pub(crate) struct McpRegistry {
    state: Arc<RwLock<McpState>>,
}

impl McpRegistry {
    pub(crate) fn register_server(&self, server: McpServerDefinition) {
        if let Ok(mut state) = self.state.write() {
            state.servers.insert(server.name.clone(), server);
        }
    }

    pub(crate) fn register(&self, descriptor: McpToolDescriptor) {
        if let Ok(mut state) = self.state.write() {
            state
                .manual_descriptors
                .insert(descriptor.wire_name.clone(), descriptor);
        }
    }

    pub(crate) fn set_client(&self, client: Arc<dyn McpClient>) {
        if let Ok(mut state) = self.state.write() {
            state.client = Some(client);
        }
    }

    pub(crate) fn server_count(&self) -> usize {
        self.state.read().map(|state| state.servers.len()).unwrap_or(0)
    }

    pub(crate) async fn refresh_registered_tools(&self) -> Result<()> {
        let (client, servers) = {
            let state = self
                .state
                .read()
                .map_err(|_| anyhow::anyhow!("mcp registry lock poisoned"))?;
            let client = state
                .client
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MCP client is not configured"))?;
            let servers = state
                .servers
                .values()
                .filter(|server| server.enabled)
                .cloned()
                .collect::<Vec<_>>();
            (client, servers)
        };

        let mut discovered_descriptors = BTreeMap::new();
        for server in servers {
            let tools = client.list_tools(&server).await?;
            for tool in tools {
                let descriptor = tool.into_descriptor(&server);
                discovered_descriptors.insert(descriptor.wire_name.clone(), descriptor);
            }
        }

        let mut state = self
            .state
            .write()
            .map_err(|_| anyhow::anyhow!("mcp registry lock poisoned"))?;
        state.discovered_descriptors = discovered_descriptors;
        Ok(())
    }

    pub(crate) fn resolve(&self, wire_name: &str, arguments: Value) -> Option<RoutedMcpTool> {
        let state = self.state.read().ok()?;
        let descriptor = state
            .manual_descriptors
            .get(wire_name)
            .or_else(|| state.discovered_descriptors.get(wire_name))?;
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
        let Ok(state) = self.state.read() else {
            return Vec::new();
        };
        state
            .manual_descriptors
            .values()
            .chain(state.discovered_descriptors.values())
            .map(|descriptor| {
                debug_assert_eq!(descriptor.spec.identity.source, ToolSource::Mcp);
                debug_assert_eq!(descriptor.spec.identity.wire_name, descriptor.wire_name);
                descriptor.spec.clone()
            })
            .collect()
    }

    pub(crate) fn supports_parallel_tool(&self, wire_name: &str) -> bool {
        let Ok(state) = self.state.read() else {
            return false;
        };
        state
            .manual_descriptors
            .get(wire_name)
            .or_else(|| state.discovered_descriptors.get(wire_name))
            .is_some_and(|descriptor| descriptor.supports_parallel_calls)
    }

    pub(crate) async fn execute(&self, invocation: LocalToolInvocation) -> Result<ToolInvocationOutput> {
        let client = {
            let state = self
                .state
                .read()
                .map_err(|_| anyhow::anyhow!("mcp registry lock poisoned"))?;
            state
                .client
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MCP client is not configured"))?
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
                server: server.clone(),
                tool: tool.clone(),
                arguments,
            })
            .await?;
        Ok(ToolInvocationOutput {
            content: response.content,
            structured: Some(agent_core::StructuredToolResult::McpToolCall {
                server,
                tool,
                result: response.result,
            }),
        })
    }
}

pub fn default_wire_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", sanitize_segment(server), sanitize_segment(tool))
}

fn sanitize_segment(value: &str) -> String {
    let mut result = String::new();
    let mut previous_was_separator = false;

    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() { ch } else { '_' };
        if normalized == '_' {
            if previous_was_separator {
                continue;
            }
            previous_was_separator = true;
            result.push('_');
        } else {
            previous_was_separator = false;
            result.push(normalized.to_ascii_lowercase());
        }
    }

    result.trim_matches('_').to_string()
}
