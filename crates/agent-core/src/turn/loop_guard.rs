use crate::conversation::ResponseItem;
use crate::tool::{ToolCall, ToolSpec};

const DEFAULT_REPEAT_THRESHOLD: usize = 3;

#[derive(Clone, Debug, Default)]
pub(crate) struct LoopGuard {
    last_signature: Option<RoundtripSignature>,
    repeated_count: usize,
    threshold: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RoundtripSignature {
    tool_calls: Vec<String>,
    tool_results: Vec<String>,
}

impl LoopGuard {
    pub(crate) fn new() -> Self {
        Self {
            last_signature: None,
            repeated_count: 0,
            threshold: DEFAULT_REPEAT_THRESHOLD,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.last_signature = None;
        self.repeated_count = 0;
    }

    pub(crate) fn record_roundtrip(
        &mut self,
        tool_calls: &[ToolCall],
        tool_specs: &[ToolSpec],
        history: &[ResponseItem],
    ) -> Option<LoopAbort> {
        let Some(signature) = build_signature(tool_calls, tool_specs, history) else {
            self.reset();
            return None;
        };
        if self.last_signature.as_ref() == Some(&signature) {
            self.repeated_count += 1;
        } else {
            self.last_signature = Some(signature);
            self.repeated_count = 1;
        }

        if self.repeated_count >= self.threshold {
            return Some(LoopAbort {
                repeated_count: self.repeated_count,
            });
        }

        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoopAbort {
    pub(crate) repeated_count: usize,
}

fn build_signature(
    tool_calls: &[ToolCall],
    tool_specs: &[ToolSpec],
    history: &[ResponseItem],
) -> Option<RoundtripSignature> {
    if tool_calls.is_empty() {
        return None;
    }
    if tool_calls
        .iter()
        .any(|call| spec_is_mutating(tool_specs, call))
    {
        return None;
    }

    let tool_calls = tool_calls
        .iter()
        .map(|call| {
            format!(
                "{}:{}",
                call.identity.wire_name,
                serde_json::to_string(&call.arguments)
                    .unwrap_or_else(|_| call.arguments.to_string())
            )
        })
        .collect::<Vec<_>>();

    let mut tool_results = Vec::with_capacity(tool_calls.len());
    for call in tool_calls.iter() {
        let (wire_name, arguments) = call
            .split_once(':')
            .expect("tool call signature should contain separator");
        let result = find_latest_result(history, wire_name, arguments)?;
        tool_results.push(result);
    }

    Some(RoundtripSignature {
        tool_calls,
        tool_results,
    })
}

fn spec_is_mutating(tool_specs: &[ToolSpec], call: &ToolCall) -> bool {
    tool_specs
        .iter()
        .find(|spec| spec.identity.wire_name == call.identity.wire_name)
        .is_some_and(|spec| spec.mutating)
}

fn find_latest_result(
    history: &[ResponseItem],
    wire_name: &str,
    arguments: &str,
) -> Option<String> {
    let assistant_index = history.iter().rposition(|item| {
        matches!(
            item,
            ResponseItem::Assistant { tool_calls, .. }
                if tool_calls.iter().any(|call| {
                    call.identity.wire_name == wire_name
                        && serde_json::to_string(&call.arguments)
                            .unwrap_or_else(|_| call.arguments.to_string())
                            == arguments
                })
        )
    })?;

    let call_ids = match &history[assistant_index] {
        ResponseItem::Assistant { tool_calls, .. } => tool_calls
            .iter()
            .filter(|call| {
                call.identity.wire_name == wire_name
                    && serde_json::to_string(&call.arguments)
                        .unwrap_or_else(|_| call.arguments.to_string())
                        == arguments
            })
            .map(|call| call.id.clone())
            .collect::<Vec<_>>(),
        _ => return None,
    };

    let mut results = Vec::new();
    for item in history.iter().skip(assistant_index + 1) {
        let ResponseItem::Tool {
            tool_call_id,
            name,
            content,
            structured,
        } = item
        else {
            continue;
        };
        if call_ids.iter().any(|id| id == tool_call_id) {
            let structured_text = structured
                .as_ref()
                .map(|value| serde_json::to_string(value).unwrap_or_default())
                .unwrap_or_default();
            results.push(format!("{name}:{content}:{structured_text}"));
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(results.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::ConversationHistory;
    use crate::tool::{StructuredToolResult, ToolIdentity};
    use crate::turn::{TurnItemDeltaKind, TurnItemKind};
    use serde_json::json;

    fn read_files_spec() -> ToolSpec {
        ToolSpec {
            name: "read_files".to_string(),
            identity: ToolIdentity::built_in("read_files"),
            description: String::new(),
            parameters: json!({}),
            mutating: false,
            requires_approval: false,
            item_kind: TurnItemKind::ToolCall,
            delta_kind: TurnItemDeltaKind::ToolOutput,
            approval_reason: None,
        }
    }

    #[test]
    fn trips_after_three_identical_read_only_roundtrips() {
        let mut history = ConversationHistory::new("conv".to_string(), "system".to_string());
        let tool_specs = vec![read_files_spec()];
        let tool_call = ToolCall {
            id: "call-1".to_string(),
            name: "read_files".to_string(),
            identity: ToolIdentity::built_in("read_files"),
            arguments: json!({"path":"cli/src/ui/chat_surface.rs"}),
        };
        let tool_result = crate::tool::ToolResult {
            tool_call_id: "call-1".to_string(),
            name: "read_files".to_string(),
            content: "same output".to_string(),
            is_error: false,
            structured: Some(StructuredToolResult::ReadFiles {
                paths: vec!["cli/src/ui/chat_surface.rs".to_string()],
                start_line: None,
                max_lines: None,
                file_count: 1,
                failed_count: 0,
                truncated_count: 0,
                total_chars: 8115,
                reads: Vec::new(),
            }),
        };
        let mut guard = LoopGuard::new();

        for index in 0..2 {
            let mut current_call = tool_call.clone();
            current_call.id = format!("call-{index}");
            let mut current_result = tool_result.clone();
            current_result.tool_call_id = current_call.id.clone();
            history.push_assistant_message(None, vec![current_call.clone()]);
            history.push_tool_result(current_result);
            assert!(
                guard
                    .record_roundtrip(&[current_call], &tool_specs, &history.messages)
                    .is_none()
            );
        }

        let mut final_call = tool_call.clone();
        final_call.id = "call-3".to_string();
        let mut final_result = tool_result;
        final_result.tool_call_id = final_call.id.clone();
        history.push_assistant_message(None, vec![final_call.clone()]);
        history.push_tool_result(final_result);
        assert_eq!(
            guard.record_roundtrip(&[final_call], &tool_specs, &history.messages),
            Some(LoopAbort { repeated_count: 3 })
        );
    }
}
