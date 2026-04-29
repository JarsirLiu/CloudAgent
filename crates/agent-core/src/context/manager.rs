use crate::conversation::{ConversationHistory, ConversationMessage};
use crate::core::ModelRequest;
use crate::tool::ToolSpec;

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

#[derive(Clone, Debug, Default)]
pub struct ContextManager;

impl ContextManager {
    pub fn new() -> Self {
        Self
    }

    pub fn build_model_context(&self, history: &ConversationHistory) -> ModelContext {
        ModelContext::from_history(history)
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
}
