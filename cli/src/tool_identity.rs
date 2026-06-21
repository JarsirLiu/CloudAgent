pub(crate) const WEB_SEARCH_TOOL_NAME: &str = "web_search";

pub(crate) fn is_web_search_tool_name(tool_name: &str) -> bool {
    tool_name.trim() == WEB_SEARCH_TOOL_NAME
}
