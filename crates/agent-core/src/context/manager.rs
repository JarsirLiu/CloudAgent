use crate::conversation::{ConversationHistory, ConversationMessage};
use crate::core::ModelRequest;
use crate::tool::{ToolCall, ToolResult, ToolSpec};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ModelContext {
    messages: Vec<ConversationMessage>,
}

impl ModelContext {
    pub fn from_history(history: &ConversationHistory) -> Self {
        Self {
            messages: history.messages.clone(),
        }
    }

    pub fn messages(&self) -> &[ConversationMessage] {
        &self.messages
    }

    pub fn into_messages(self) -> Vec<ConversationMessage> {
        self.messages
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextManager {
    history: ConversationHistory,
}

impl ContextManager {
    pub fn new(id: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            history: ConversationHistory::new(id, system_prompt),
        }
    }

    pub fn from_history(history: ConversationHistory) -> Self {
        Self { history }
    }

    pub fn history(&self) -> &ConversationHistory {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut ConversationHistory {
        &mut self.history
    }

    pub fn into_history(self) -> ConversationHistory {
        self.history
    }

    pub fn ensure_system_prompt(&mut self, system_prompt: impl Into<String>) {
        self.history.ensure_system_prompt(system_prompt);
    }

    pub fn record_user_message(&mut self, content: impl Into<String>) {
        self.history.push_user_message(content);
    }

    pub fn record_assistant_message(&mut self, content: Option<String>, tool_calls: Vec<ToolCall>) {
        self.history.push_assistant_message(content, tool_calls);
    }

    pub fn record_tool_result(&mut self, result: ToolResult) {
        self.history.push_tool_result(result);
    }

    pub fn build_model_context(&self, history: &ConversationHistory) -> ModelContext {
        ModelContext::from_history(history)
    }

    pub fn build_current_model_context(&self) -> ModelContext {
        ModelContext::from_history(&self.history)
    }

    pub fn build_model_request(
        &self,
        history: &ConversationHistory,
        tools: Vec<ToolSpec>,
        temperature: f32,
    ) -> ModelRequest {
        let context = self.build_model_context(history);
        ModelRequest {
            messages: context.into_messages(),
            tools,
            temperature,
        }
    }

    pub fn build_current_model_request(
        &self,
        tools: Vec<ToolSpec>,
        temperature: f32,
    ) -> ModelRequest {
        let context = self.build_current_model_context();
        ModelRequest {
            messages: context.into_messages(),
            tools,
            temperature,
        }
    }
}
