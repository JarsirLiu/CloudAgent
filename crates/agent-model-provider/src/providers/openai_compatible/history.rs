use agent_core::conversation::ResponseItem;
use agent_core::model::ModelRequest;
use agent_core::tool::{StructuredToolResult, ToolCall};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Clone, Debug)]
struct CachedToolContext {
    assistant_item: ResponseItem,
    tool_results: Vec<ResponseItem>,
}

#[derive(Default)]
pub(super) struct RequestHistory {
    by_call_id: RwLock<HashMap<String, CachedToolContext>>,
}

impl RequestHistory {
    pub(super) fn enrich_chat_request(&self, mut request: ModelRequest) -> ModelRequest {
        self.record_from_messages(&request.messages);
        restore_missing_tool_call_context(&mut request.messages, &self.by_call_id);
        ensure_tool_outputs_present(&mut request.messages);
        request
    }

    fn record_from_messages(&self, messages: &[ResponseItem]) {
        let mut cache = self
            .by_call_id
            .write()
            .expect("history cache lock poisoned");

        for (assistant_index, item) in messages.iter().enumerate() {
            let ResponseItem::Assistant { tool_calls, .. } = item else {
                continue;
            };
            if tool_calls.is_empty() {
                continue;
            }

            for call in tool_calls {
                let tool_results = collect_adjacent_tool_results(messages, assistant_index, call);
                cache.insert(
                    call.id.clone(),
                    CachedToolContext {
                        assistant_item: ResponseItem::Assistant {
                            content: match item {
                                ResponseItem::Assistant { content, .. } => content.clone(),
                                _ => None,
                            },
                            reasoning: match item {
                                ResponseItem::Assistant { reasoning, .. } => reasoning.clone(),
                                _ => None,
                            },
                            tool_calls: vec![call.clone()],
                        },
                        tool_results,
                    },
                );
            }
        }
    }
}

fn collect_adjacent_tool_results(
    messages: &[ResponseItem],
    assistant_index: usize,
    call: &ToolCall,
) -> Vec<ResponseItem> {
    let mut results = Vec::new();
    for item in messages.iter().skip(assistant_index + 1) {
        match item {
            ResponseItem::Tool { tool_call_id, .. } if tool_call_id == &call.id => {
                results.push(item.clone());
            }
            ResponseItem::Tool { .. } => {}
            ResponseItem::Assistant { .. }
            | ResponseItem::User { .. }
            | ResponseItem::System { .. } => {
                if !results.is_empty() {
                    break;
                }
            }
        }
    }
    results
}

fn restore_missing_tool_call_context(
    items: &mut Vec<ResponseItem>,
    cache: &RwLock<HashMap<String, CachedToolContext>>,
) {
    let cache = cache.read().expect("history cache lock poisoned");
    let mut restored = Vec::new();

    for (index, item) in items.iter().enumerate() {
        let ResponseItem::Tool { tool_call_id, .. } = item else {
            continue;
        };
        if has_matching_assistant_before(items, index, tool_call_id) {
            continue;
        }
        let Some(context) = cache.get(tool_call_id) else {
            continue;
        };
        restored.push((index, context.clone()));
    }

    for (index, context) in restored.into_iter().rev() {
        items.insert(index, context.assistant_item.clone());

        let mut insert_at = index + 1;
        for tool_result in context.tool_results {
            let tool_call_id = match &tool_result {
                ResponseItem::Tool { tool_call_id, .. } => tool_call_id,
                _ => continue,
            };
            let already_present = items.iter().skip(insert_at).any(|candidate| {
                matches!(
                    candidate,
                    ResponseItem::Tool { tool_call_id: existing, .. } if existing == tool_call_id
                )
            });
            if already_present {
                continue;
            }
            items.insert(insert_at, tool_result);
            insert_at += 1;
        }
    }
}

fn has_matching_assistant_before(
    items: &[ResponseItem],
    tool_index: usize,
    tool_call_id: &str,
) -> bool {
    items[..tool_index].iter().rev().any(|item| {
        matches!(
            item,
            ResponseItem::Assistant { tool_calls, .. }
                if tool_calls.iter().any(|call| call.id == tool_call_id)
        )
    })
}

fn ensure_tool_outputs_present(items: &mut Vec<ResponseItem>) {
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
