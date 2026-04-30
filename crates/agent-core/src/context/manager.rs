use super::fragments::{ContextFragment, insert_context_fragments_before_latest_user};
use crate::conversation::{ConversationHistory, ResponseItem};
use crate::model::ModelRequest;
use crate::tool::{ToolCall, ToolResult, ToolSpec};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ModelContext {
    messages: Vec<ResponseItem>,
}

impl ModelContext {
    pub fn from_history(history: &ConversationHistory) -> Self {
        Self {
            messages: history.messages.clone(),
        }
    }

    pub fn from_history_with_fragments(
        history: &ConversationHistory,
        fragments: &[ResponseItem],
    ) -> Self {
        Self {
            messages: insert_context_fragments_before_latest_user(
                history.messages.clone(),
                fragments,
            ),
        }
    }

    pub fn messages(&self) -> &[ResponseItem] {
        &self.messages
    }

    pub fn into_messages(self) -> Vec<ResponseItem> {
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

    pub fn record_user_message(&mut self, content: impl Into<String>) -> ResponseItem {
        self.history.push_user_message(content)
    }

    pub fn record_assistant_message(
        &mut self,
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> ResponseItem {
        self.history.push_assistant_message(content, tool_calls)
    }

    pub fn record_tool_result(&mut self, result: ToolResult) -> ResponseItem {
        self.history.push_tool_result(result)
    }

    pub fn build_model_context(&self, history: &ConversationHistory) -> ModelContext {
        ModelContext::from_history(history)
    }

    pub fn build_model_context_with_fragments(
        &self,
        history: &ConversationHistory,
        fragments: &[impl ContextFragment],
    ) -> ModelContext {
        let rendered = render_context_fragments(fragments);
        ModelContext::from_history_with_fragments(history, &rendered)
    }

    pub fn build_current_model_context(&self) -> ModelContext {
        ModelContext::from_history(&self.history)
    }

    pub fn build_current_model_context_with_fragments(
        &self,
        fragments: &[impl ContextFragment],
    ) -> ModelContext {
        self.build_model_context_with_fragments(&self.history, fragments)
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

    pub fn build_model_request_with_fragments(
        &self,
        history: &ConversationHistory,
        fragments: &[impl ContextFragment],
        tools: Vec<ToolSpec>,
        temperature: f32,
    ) -> ModelRequest {
        let context = self.build_model_context_with_fragments(history, fragments);
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

    pub fn build_current_model_request_with_fragments(
        &self,
        fragments: &[impl ContextFragment],
        tools: Vec<ToolSpec>,
        temperature: f32,
    ) -> ModelRequest {
        self.build_model_request_with_fragments(&self.history, fragments, tools, temperature)
    }
}

fn render_context_fragments(fragments: &[impl ContextFragment]) -> Vec<ResponseItem> {
    fragments
        .iter()
        .map(ContextFragment::render)
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::EnvironmentContext;

    #[test]
    fn contextual_fragments_are_model_request_only_and_before_latest_user() {
        let mut manager = ContextManager::new("default", "system");
        manager.record_user_message("hello");

        let environment = EnvironmentContext::new(
            r"D:\learn\gifti\cloudagent",
            "powershell",
            "2026-04-30",
            "19:16:01",
            "2026-04-30T19:16:01+08:00",
            "+08:00",
        );
        let request =
            manager.build_current_model_request_with_fragments(&[environment], Vec::new(), 0.0);

        assert_eq!(manager.history().messages.len(), 2);
        assert_eq!(request.messages.len(), 3);
        assert!(matches!(request.messages[0], ResponseItem::System { .. }));
        assert!(
            matches!(request.messages[1], ResponseItem::User { ref content } if content.starts_with("<environment_context>"))
        );
        assert!(
            matches!(request.messages[2], ResponseItem::User { ref content } if content == "hello")
        );
    }
}
