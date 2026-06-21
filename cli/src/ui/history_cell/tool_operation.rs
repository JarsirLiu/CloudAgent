use crate::tool_identity::is_web_search_tool_name;
use agent_core::{StructuredToolResult, ToolIdentity, ToolSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolOperation {
    Search,
    Read,
    Run,
    Edit,
    External,
}

pub(crate) fn classify_tool_name(tool_name: &str) -> ToolOperation {
    match tool_name {
        _ if is_web_search_tool_name(tool_name) => ToolOperation::External,
        "tool_search" | "search_workspace" => ToolOperation::Search,
        "read_file" | "read_directory" | "get_metadata" => ToolOperation::Read,
        "exec_command" | "write_stdin" | "tool" => ToolOperation::Run,
        "apply_patch" | "edit_file" | "create_directory" | "copy_path" | "remove_path"
        | "write_file" => ToolOperation::Edit,
        _ if tool_name.starts_with("mcp__") => ToolOperation::External,
        _ => ToolOperation::External,
    }
}

pub(crate) fn classify_structured_result(structured: &StructuredToolResult) -> ToolOperation {
    match structured {
        StructuredToolResult::WebSearch { .. } => ToolOperation::External,
        StructuredToolResult::CommandExecution { .. } => ToolOperation::Run,
        StructuredToolResult::SearchWorkspace { .. } | StructuredToolResult::ToolSearch { .. } => {
            ToolOperation::Search
        }
        StructuredToolResult::ReadDirectory { .. }
        | StructuredToolResult::ReadFileBytes { .. }
        | StructuredToolResult::ReadFile { .. }
        | StructuredToolResult::GetMetadata { .. } => ToolOperation::Read,
        StructuredToolResult::CreateDirectory { .. }
        | StructuredToolResult::WriteFileBytes { .. }
        | StructuredToolResult::CopyPath { .. }
        | StructuredToolResult::RemovePath { .. }
        | StructuredToolResult::EditFile { .. } => ToolOperation::Edit,
        StructuredToolResult::McpToolCall { .. } => ToolOperation::External,
        StructuredToolResult::ToolError { tool_name, .. } => classify_tool_name(tool_name),
        StructuredToolResult::Watch { .. } | StructuredToolResult::Unwatch { .. } => {
            ToolOperation::Read
        }
    }
}

pub(crate) fn classify_tool_operation(
    tool_name: Option<&str>,
    structured: Option<&StructuredToolResult>,
    identity: Option<&ToolIdentity>,
) -> ToolOperation {
    if let Some(structured) = structured {
        return classify_structured_result(structured);
    }
    if let Some(identity) = identity {
        return classify_tool_identity(identity);
    }
    tool_name
        .map(classify_tool_name)
        .unwrap_or(ToolOperation::External)
}

fn classify_tool_identity(identity: &ToolIdentity) -> ToolOperation {
    match identity.source {
        ToolSource::BuiltIn => classify_tool_name(&identity.wire_name),
        ToolSource::Hosted | ToolSource::Mcp | ToolSource::Dynamic => ToolOperation::External,
    }
}

#[cfg(test)]
#[path = "tool_operation_tests.rs"]
mod tests;
