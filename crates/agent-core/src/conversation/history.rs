use crate::tool::{StructuredToolResult, ToolCall, ToolResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationHistory {
    pub id: String,
    pub turn_count: u64,
    pub messages: Vec<ResponseItem>,
}

impl ConversationHistory {
    pub fn new(id: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            turn_count: 0,
            messages: vec![ResponseItem::System {
                content: system_prompt.into(),
            }],
        }
    }

    pub fn push_user_message(&mut self, content: impl Into<String>) -> ResponseItem {
        self.turn_count += 1;
        let item = ResponseItem::User {
            content: content.into(),
        };
        self.messages.push(item.clone());
        item
    }

    pub fn push_assistant_message(
        &mut self,
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> ResponseItem {
        let item = ResponseItem::Assistant {
            content,
            tool_calls,
        };
        self.messages.push(item.clone());
        item
    }

    pub fn push_tool_result(&mut self, result: ToolResult) -> ResponseItem {
        let item = ResponseItem::Tool {
            tool_call_id: result.tool_call_id,
            name: result.name,
            content: result.content,
            structured: result.structured,
        };
        self.messages.push(item.clone());
        item
    }

    pub fn ensure_system_prompt(&mut self, system_prompt: impl Into<String>) {
        let system_prompt = system_prompt.into();
        let has_system = matches!(self.messages.first(), Some(ResponseItem::System { .. }));
        if has_system {
            return;
        }
        self.messages.insert(
            0,
            ResponseItem::System {
                content: system_prompt,
            },
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ResponseItem {
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
        #[serde(default)]
        structured: Option<StructuredToolResult>,
    },
}
