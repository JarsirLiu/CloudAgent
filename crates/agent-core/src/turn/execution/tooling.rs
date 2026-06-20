use std::collections::BTreeMap;

pub(super) fn finish_reason_implies_tool_use(finish_reason: Option<&str>) -> bool {
    matches!(finish_reason, Some("tool_calls") | Some("tool_use"))
}

pub(super) fn compose_visible_tool_specs(
    default_tools: &[crate::ToolSpec],
    deferred_tool_map: &BTreeMap<String, crate::ToolSpec>,
    exposed_tool_names: &[String],
) -> Vec<crate::ToolSpec> {
    let mut tools = default_tools.to_vec();
    for tool_name in exposed_tool_names {
        if let Some(spec) = deferred_tool_map.get(tool_name)
            && !tools
                .iter()
                .any(|existing| existing.identity.wire_name == spec.identity.wire_name)
        {
            tools.push(spec.clone());
        }
    }
    tools
}

pub(super) fn collect_discoverable_tools(
    deferred_tool_map: &BTreeMap<String, crate::ToolSpec>,
    exposed_tool_names: &[String],
) -> Vec<crate::ToolSpec> {
    deferred_tool_map
        .iter()
        .filter(|(tool_name, _)| !exposed_tool_names.iter().any(|name| name == *tool_name))
        .map(|(_, spec)| spec.clone())
        .collect()
}
