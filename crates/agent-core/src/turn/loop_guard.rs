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
#[path = "loop_guard_tests.rs"]
mod tests;
