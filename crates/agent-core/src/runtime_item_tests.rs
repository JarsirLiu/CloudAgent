use super::{RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress};
use crate::conversation::TranscriptItem;
use crate::tool::{StructuredToolResult, ToolIdentity, ToolSource};
use crate::{TurnItemKind, WebSearchAction};

#[test]
fn with_tool_identity_and_progress_enrich_started_item() {
    let item = RuntimeItem::started(
        "ws-1",
        Some("ws-1".to_string()),
        TurnItemKind::ToolResult,
        Some("web_search".to_string()),
    )
    .with_tool_identity(ToolIdentity::hosted("web_search"))
    .with_progress(RuntimeItemProgress::message("weather seattle"))
    .with_summary("weather seattle");

    let identity = item.tool_identity.expect("tool identity");
    assert_eq!(identity.source, ToolSource::Hosted);
    assert_eq!(identity.wire_name, "web_search");
    assert_eq!(
        item.progress.and_then(|progress| progress.message),
        Some("weather seattle".to_string())
    );
    assert_eq!(item.summary.as_deref(), Some("weather seattle"));
}

#[test]
fn completed_tool_result_derives_metrics_from_structured_payload() {
    let transcript_item = TranscriptItem::ToolResult {
        id: "ws-1".to_string(),
        tool_name: "web_search".to_string(),
        content: "weather seattle".to_string(),
        summary: "searched the web".to_string(),
        structured: Some(StructuredToolResult::WebSearch {
            query: "weather seattle".to_string(),
            action: Some(WebSearchAction::Search {
                query: Some("weather seattle".to_string()),
                queries: None,
            }),
            result_count: Some(6),
            source_count: Some(3),
        }),
    };

    let item = RuntimeItem::completed(&transcript_item, Some("ws-1".to_string()))
        .with_tool_identity(ToolIdentity::hosted("web_search"));

    let metrics = item.metrics.expect("metrics");
    assert_eq!(metrics.result_count, Some(6));
    assert_eq!(metrics.source_count, Some(3));
    assert_eq!(
        item.tool_identity.expect("tool identity").source,
        ToolSource::Hosted
    );
}

#[test]
fn runtime_item_metrics_extracts_file_counts_from_edit_results() {
    let metrics =
        RuntimeItemMetrics::from_structured_result(Some(&StructuredToolResult::EditFile {
            changed_paths: vec!["a.rs".to_string(), "b.rs".to_string()],
            files_changed: 2,
            status: crate::WriteFileStatus::Completed,
            version_token: None,
        }))
        .expect("metrics");

    assert_eq!(metrics.file_count, Some(2));
    assert_eq!(metrics.elapsed_ms, None);
}
