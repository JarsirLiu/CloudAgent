use agent_protocol::{ToolCall, ToolResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub turn_count: u64,
    pub messages: Vec<ConversationMessage>,
}

impl AgentSession {
    pub fn new(id: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            turn_count: 0,
            messages: vec![ConversationMessage::System {
                content: system_prompt.into(),
            }],
        }
    }

    pub fn push_user_message(&mut self, content: impl Into<String>) {
        self.turn_count += 1;
        self.messages.push(ConversationMessage::User {
            content: content.into(),
        });
    }

    pub fn push_assistant_message(&mut self, content: Option<String>, tool_calls: Vec<ToolCall>) {
        self.messages.push(ConversationMessage::Assistant {
            content,
            tool_calls,
        });
    }

    pub fn push_tool_result(&mut self, result: ToolResult) {
        self.messages.push(ConversationMessage::Tool {
            tool_call_id: result.tool_call_id,
            name: result.name,
            content: result.content,
        });
    }

    pub fn ensure_system_prompt(&mut self, system_prompt: impl Into<String>) {
        let system_prompt = system_prompt.into();
        let has_system = matches!(
            self.messages.first(),
            Some(ConversationMessage::System { .. })
        );
        if has_system {
            return;
        }
        self.messages.insert(
            0,
            ConversationMessage::System {
                content: system_prompt,
            },
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ConversationMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        name: String,
        content: String,
    },
}

pub fn module_name() -> &'static str {
    "agent-core::session"
}
