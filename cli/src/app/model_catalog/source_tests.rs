use super::source::{candidate_model_urls, parse_model_ids};

#[test]
fn candidate_urls_cover_v1_and_known_compat_paths() {
    assert_eq!(
        candidate_model_urls("https://api.openai.com/v1").expect("urls"),
        vec![
            "https://api.openai.com/v1/models".to_string(),
            "https://api.openai.com/models".to_string(),
        ]
    );

    assert_eq!(
        candidate_model_urls("https://example.com/openai/chat/completions").expect("urls"),
        vec![
            "https://example.com/openai/chat/completions/models".to_string(),
            "https://example.com/openai/chat/completions/v1/models".to_string(),
            "https://example.com/openai/models".to_string(),
            "https://example.com/openai/v1/models".to_string(),
        ]
    );
}

#[test]
fn parse_model_ids_accepts_openai_shape() {
    let models =
        parse_model_ids(r#"{"object":"list","data":[{"id":"gpt-4.1"},{"id":"gpt-4.1-mini"}]}"#)
            .expect("models");
    assert_eq!(
        models,
        vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string()]
    );
}

#[test]
fn parse_model_ids_accepts_plain_array() {
    let models = parse_model_ids(r#"["foo","bar"]"#).expect("models");
    assert_eq!(models, vec!["bar".to_string(), "foo".to_string()]);
}

#[test]
fn parse_model_ids_accepts_models_property_and_deduplicates() {
    let models = parse_model_ids(r#"{"models":[{"id":"foo"},"foo","bar"]}"#).expect("models");
    assert_eq!(models, vec!["bar".to_string(), "foo".to_string()]);
}

#[test]
fn parse_model_ids_rejects_invalid_json() {
    assert!(parse_model_ids("not json").is_err());
}
