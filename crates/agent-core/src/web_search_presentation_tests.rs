use super::*;
use crate::model::WebSearchAction;

#[test]
fn web_search_presentation_prefers_action_details_and_metrics() {
    let presentation = web_search_presentation(
        "ignored",
        Some(&WebSearchAction::Search {
            query: Some("openai docs".to_string()),
            queries: None,
        }),
        Some(3),
        Some(7),
    );

    assert_eq!(presentation.summary, "searched 3 sources");
    assert!(presentation.detail.contains("openai docs"));
}

#[test]
fn web_search_presentation_falls_back_to_query_when_action_is_empty() {
    let presentation =
        web_search_presentation("fallback query", Some(&WebSearchAction::Other), None, None);

    assert_eq!(presentation.summary, "searched the web");
    assert_eq!(presentation.detail, "fallback query");
}
