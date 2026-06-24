use crate::conversation::TranscriptItem;
use crate::model::WebSearchAction;
use crate::tool::{StructuredToolResult, ToolIdentity};
use crate::{RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress, TurnItemKind};

pub const WEB_SEARCH_TOOL_NAME: &str = "web_search";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSearchPresentation {
    pub summary: String,
    pub detail: String,
}

pub fn web_search_presentation(
    query: &str,
    action: Option<&WebSearchAction>,
    source_count: Option<usize>,
    result_count: Option<usize>,
) -> WebSearchPresentation {
    WebSearchPresentation {
        summary: web_search_summary(query, action, source_count, result_count),
        detail: web_search_detail(query, action),
    }
}

pub fn web_search_detail(query: &str, action: Option<&WebSearchAction>) -> String {
    let _ = query;
    match action {
        Some(WebSearchAction::Search { query, queries }) => query
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                let first = queries
                    .as_ref()
                    .and_then(|values| values.first())
                    .cloned()
                    .unwrap_or_default();
                if queries.as_ref().is_some_and(|values| values.len() > 1) && !first.is_empty() {
                    format!("{first} ...")
                } else {
                    first
                }
            }),
        Some(WebSearchAction::OpenPage { url }) => url.clone().unwrap_or_default(),
        Some(WebSearchAction::FindInPage { url, pattern }) => match (pattern, url) {
            (Some(pattern), Some(url)) => format!("'{pattern}' in {url}"),
            (Some(pattern), None) => format!("'{pattern}'"),
            (None, Some(url)) => url.clone(),
            (None, None) => String::new(),
        },
        Some(WebSearchAction::Unknown { raw_type, raw }) => raw_type
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| raw.as_ref().map(ToString::to_string))
            .unwrap_or_default(),
        None => String::new(),
    }
}

pub fn web_search_summary(
    _query: &str,
    _action: Option<&WebSearchAction>,
    source_count: Option<usize>,
    result_count: Option<usize>,
) -> String {
    match (source_count, result_count) {
        (Some(count), _) if count > 0 => format!("searched {count} sources"),
        (_, Some(count)) if count > 0 => format!("found {count} results"),
        _ => "searched the web".to_string(),
    }
}

pub fn web_search_transcript_item(
    item_id: impl Into<String>,
    query: impl Into<String>,
    action: Option<WebSearchAction>,
) -> TranscriptItem {
    let item_id = item_id.into();
    let query = query.into();
    let presentation = web_search_presentation(&query, action.as_ref(), None, None);
    TranscriptItem::ToolResult {
        id: item_id,
        tool_name: WEB_SEARCH_TOOL_NAME.to_string(),
        content: presentation.detail.clone(),
        summary: presentation.summary,
        structured: Some(StructuredToolResult::WebSearch {
            query,
            action,
            result_count: None,
            source_count: None,
        }),
    }
}

pub fn web_search_runtime_item_started(
    item_id: impl Into<String>,
    query: impl Into<String>,
) -> RuntimeItem {
    let item_id = item_id.into();
    let query = query.into();
    let presentation = web_search_presentation(&query, None, None, None);
    RuntimeItem::started(
        item_id.clone(),
        Some(item_id),
        TurnItemKind::ToolResult,
        Some(WEB_SEARCH_TOOL_NAME.to_string()),
    )
    .with_tool_identity(ToolIdentity::hosted(WEB_SEARCH_TOOL_NAME))
    .with_structured(StructuredToolResult::WebSearch {
        query: query.clone(),
        action: None,
        result_count: None,
        source_count: None,
    })
    .with_progress(RuntimeItemProgress::message(presentation.detail))
    .with_summary(query)
}

pub fn web_search_runtime_item_completed(
    transcript_item: &TranscriptItem,
    call_id: impl Into<String>,
) -> RuntimeItem {
    let mut item = RuntimeItem::completed(transcript_item, Some(call_id.into()))
        .with_tool_identity(ToolIdentity::hosted(WEB_SEARCH_TOOL_NAME));
    if let Some(metrics) = RuntimeItemMetrics::from_transcript_item(transcript_item) {
        item = item.with_metrics(metrics);
    }
    item
}

#[cfg(test)]
#[path = "web_search_presentation_tests.rs"]
mod tests;
