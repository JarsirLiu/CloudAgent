use super::WebSearchTool;
use crate::spec::{ToolDefaultVisibility, ToolLayer};

#[test]
fn web_search_is_default_visible_coordination_tool() {
    let descriptor = WebSearchTool::descriptor();
    assert_eq!(descriptor.spec.identity.wire_name, "web_search");
    assert_eq!(descriptor.layer, ToolLayer::Coordination);
    assert_eq!(descriptor.default_visibility, ToolDefaultVisibility::Default);
}
