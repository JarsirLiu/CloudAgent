use crate::tool::{StructuredToolResult, ToolCall, ToolResult};
use serde::{Deserialize, Serialize};

use super::InputItem;

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

    pub fn push_user_message(&mut self, content: Vec<InputItem>) -> ResponseItem {
        self.turn_count += 1;
        let item = ResponseItem::User { content };
        self.messages.push(item.clone());
        item
    }

    pub fn rollback_last_user_message(&mut self, expected: &ResponseItem) -> bool {
        let Some(ResponseItem::User {
            content: expected_content,
        }) = Some(expected)
        else {
            return false;
        };
        let Some(ResponseItem::User {
            content: last_content,
        }) = self.messages.last()
        else {
            return false;
        };
        if last_content != expected_content {
            return false;
        }
        self.messages.pop();
        self.turn_count = self.turn_count.saturating_sub(1);
        true
    }

    pub fn push_assistant_message(
        &mut self,
        content: Option<String>,
        reasoning: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> ResponseItem {
        let item = ResponseItem::Assistant {
            content,
            reasoning,
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

    pub fn ensure_tool_outputs_present(&mut self) {
        ensure_tool_outputs_present(&mut self.messages);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum ResponseItem {
    System {
        content: String,
    },
    User {
        content: Vec<InputItem>,
    },
    Assistant {
        content: Option<String>,
        #[serde(default)]
        reasoning: Option<String>,
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

impl ResponseItem {
    pub fn user_plain_text(content: &[InputItem]) -> String {
        super::input_items_to_plain_text(content)
    }
}

pub fn ensure_tool_outputs_present(items: &mut Vec<ResponseItem>) {
    let mut missing_outputs_to_insert = Vec::new();

    for (index, item) in items.iter().enumerate() {
        let ResponseItem::Assistant { tool_calls, .. } = item else {
            continue;
        };
        for call in tool_calls {
            let has_output = items.iter().any(|candidate| {
                matches!(
                    candidate,
                    ResponseItem::Tool { tool_call_id, .. } if tool_call_id == &call.id
                )
            });
            if !has_output {
                missing_outputs_to_insert.push((
                    index,
                    ResponseItem::Tool {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: "aborted".to_string(),
                        structured: Some(StructuredToolResult::ToolError {
                            tool_name: call.name.clone(),
                            message: "aborted".to_string(),
                        }),
                    },
                ));
            }
        }
    }

    for (index, item) in missing_outputs_to_insert.into_iter().rev() {
        items.insert(index + 1, item);
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
