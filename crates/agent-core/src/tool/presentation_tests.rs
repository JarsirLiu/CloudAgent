use super::*;
use serde_json::json;

#[test]
fn apply_patch_arguments_are_summarized_without_patch_body() {
    let summary = summarize_tool_arguments(
        "apply_patch",
        &json!({
            "patch": "*** Begin Patch\n*** Update File: a.rs\n@@\n-old\n+new\n*** Add File: b.rs\n+new\n*** End Patch"
        }),
    );

    assert_eq!(summary, "patch 2 files (1 add, 1 update) — a.rs, b.rs");
    assert!(!summary.contains("*** Begin Patch"));
}
