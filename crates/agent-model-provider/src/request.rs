use agent_core::{ModelRequest, ResponseItem, ToolCall, ToolSpec};

#[derive(Clone, Debug)]
pub(crate) struct ProviderRequest {
    pub messages: Vec<ProviderMessage>,
    pub tools: Vec<ToolSpec>,
    pub temperature: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum ProviderMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        name: String,
        content: String,
    },
}

impl ProviderRequest {
    pub(crate) fn from_model_request(request: &ModelRequest) -> Self {
        Self {
            messages: request
                .messages
                .iter()
                .map(ProviderMessage::from_response_item)
                .collect(),
            tools: request.tools.clone(),
            temperature: request.temperature,
        }
    }
}

impl ProviderMessage {
    fn from_response_item(item: &ResponseItem) -> Self {
        match item {
            ResponseItem::System { content } => Self::System {
                content: content.clone(),
            },
            ResponseItem::User { content } => Self::User {
                content: content.clone(),
            },
            ResponseItem::Assistant {
                content,
                tool_calls,
            } => Self::Assistant {
                content: content.clone(),
                tool_calls: tool_calls.clone(),
            },
            ResponseItem::Tool {
                tool_call_id,
                name,
                content,
                ..
            } => Self::Tool {
                tool_call_id: tool_call_id.clone(),
                name: name.clone(),
                content: content.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{CommandExecutionStatus, StructuredToolResult, ToolIdentity};
    use serde_json::json;

    #[test]
    fn provider_request_drops_structured_tool_output_details() {
        let request = ModelRequest {
            messages: vec![ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: "[rtk:generic]\nsummary".to_string(),
                structured: Some(StructuredToolResult::CommandExecution {
                    command: "Get-ChildItem -Recurse".to_string(),
                    current_directory: "D:\\repo".to_string(),
                    session_id: None,
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    success: Some(true),
                    stdout: Some("very large raw output".to_string()),
                    stderr: Some(String::new()),
                    aggregated_output: Some("very large raw output".to_string()),
                    duration_ms: Some(1),
                }),
            }],
            tools: vec![ToolSpec {
                name: "exec_command".to_string(),
                identity: ToolIdentity::built_in("exec_command"),
                description: "run a command".to_string(),
                parameters: json!({ "type": "object" }),
                mutating: false,
                execution_policy: agent_core::ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: agent_core::turn::TurnItemKind::ToolCall,
                delta_kind: agent_core::turn::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            }],
            temperature: 0.0,
        };

        let provider_request = ProviderRequest::from_model_request(&request);
        match &provider_request.messages[0] {
            ProviderMessage::Tool { content, .. } => {
                assert_eq!(content, "[rtk:generic]\nsummary");
            }
            _ => panic!("expected tool message"),
        }
    }
}
