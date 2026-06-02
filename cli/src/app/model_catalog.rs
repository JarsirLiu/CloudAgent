mod cache;

use anyhow::{anyhow, Context, Result};
use cache::ModelCatalogCacheManager;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::Duration;
use tracing::warn;
use url::Url;

const MODEL_CATALOG_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelCatalog {
    pub(crate) source_url: String,
    pub(crate) models: Vec<String>,
}

pub(crate) async fn fetch_model_catalog(base_url: &str, api_key: &str) -> Result<ModelCatalog> {
    let candidate_urls = candidate_model_urls(base_url)?;
    let cache = ModelCatalogCacheManager::new(base_url, api_key, MODEL_CATALOG_CACHE_TTL)?;

    if let Some(cache) = cache.as_ref()
        && let Some(entry) = cache.load_fresh().await?
    {
        return Ok(entry.into_catalog());
    }

    let mut last_error = None;
    for candidate in candidate_urls {
        match fetch_models_once(&candidate, api_key).await {
            Ok(models) if !models.is_empty() => {
                let catalog = ModelCatalog {
                    source_url: candidate,
                    models,
                };
                if let Some(cache) = cache.as_ref() {
                    cache.store(&catalog).await;
                }
                return Ok(catalog);
            }
            Ok(_) => {
                last_error = Some(format!("`{candidate}` returned no models"));
            }
            Err(err) => {
                last_error = Some(format!("`{candidate}` failed: {err}"));
            }
        }
    }

    if let Some(cache) = cache.as_ref()
        && let Some(entry) = cache.load_any().await?
    {
        warn!(
            cache_path = %cache.cache_path().display(),
            "model catalog cache: serving stale cache after fetch failure"
        );
        return Ok(entry.into_catalog());
    }

    Err(anyhow!(
        "Unable to load a model list from the configured Base URL. {}",
        last_error.unwrap_or_else(|| "No compatible `/models` endpoint was found.".to_string())
    ))
}

fn candidate_model_urls(base_url: &str) -> Result<Vec<String>> {
    let parsed = Url::parse(base_url).context("Base URL is not a valid URL")?; // keep user-facing error clear
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let mut bases = vec![parsed.clone()];

    if let Some(stripped) = strip_known_model_path(&parsed) {
        bases.push(stripped);
    }

    for base in bases {
        for suffix in candidate_model_suffixes(&base) {
            let mut candidate = base.clone();
            candidate.set_path(&suffix);
            candidate.set_query(None);
            candidate.set_fragment(None);
            let value = candidate.to_string();
            if seen.insert(value.clone()) {
                candidates.push(value);
            }
        }
    }

    Ok(candidates)
}

fn strip_known_model_path(url: &Url) -> Option<Url> {
    let path = url.path().trim_end_matches('/');
    let known_suffixes = [
        "/chat/completions",
        "/completions",
        "/responses",
        "/embeddings",
        "/models",
    ];
    for suffix in known_suffixes {
        if let Some(prefix) = path.strip_suffix(suffix) {
            let mut stripped = url.clone();
            stripped.set_path(if prefix.is_empty() { "/" } else { prefix });
            return Some(stripped);
        }
    }
    None
}

fn candidate_model_suffixes(base: &Url) -> Vec<String> {
    let path = base.path().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return vec!["/v1/models".to_string(), "/models".to_string()];
    }
    if path.ends_with("/v1") {
        return vec![format!("{path}/models"), "/models".to_string()];
    }
    if path.ends_with("/models") {
        return vec![path.to_string()];
    }
    vec![format!("{path}/models"), format!("{path}/v1/models")]
}

async fn fetch_models_once(url: &str, api_key: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .user_agent("cloudagent/0.1.0")
        .build()
        .context("failed to build HTTP client")?;

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if !api_key.trim().is_empty() {
        let value = HeaderValue::from_str(&format!("Bearer {}", api_key.trim()))
            .context("invalid API key header value")?;
        headers.insert(AUTHORIZATION, value);
    }

    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .with_context(|| format!("failed to request {url}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "HTTP {}: {}",
            status.as_u16(),
            truncate_body(&body)
        ));
    }

    parse_model_ids(&body).context("response body did not contain a compatible model list")
}

fn parse_model_ids(body: &str) -> Result<Vec<String>> {
    let value: Value = serde_json::from_str(body).context("invalid JSON response")?;
    let mut models = Vec::new();
    if let Some(data) = value.get("data").and_then(Value::as_array) {
        for item in data {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                models.push(id.to_string());
            } else if let Some(id) = item.as_str() {
                models.push(id.to_string());
            }
        }
    } else if let Some(models_array) = value.get("models").and_then(Value::as_array) {
        for item in models_array {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                models.push(id.to_string());
            } else if let Some(id) = item.as_str() {
                models.push(id.to_string());
            }
        }
    } else if let Some(array) = value.as_array() {
        for item in array {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                models.push(id.to_string());
            } else if let Some(id) = item.as_str() {
                models.push(id.to_string());
            }
        }
    }
    models.sort();
    models.dedup();
    Ok(models)
}

fn truncate_body(body: &str) -> String {
    const MAX_LEN: usize = 240;
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX_LEN {
        return trimmed.to_string();
    }
    let mut out = String::new();
    for ch in trimmed.chars().take(MAX_LEN) {
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::{candidate_model_urls, parse_model_ids};

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
}
