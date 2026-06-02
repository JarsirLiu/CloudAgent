use crate::app::model_catalog::ModelCatalog;
use anyhow::{Context, Result};
use config::default_user_data_root;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tracing::{error, info, warn};
use url::Url;

#[derive(Debug, Clone)]
pub(crate) struct ModelCatalogCacheManager {
    cache_path: PathBuf,
    cache_ttl: Duration,
}

impl ModelCatalogCacheManager {
    pub(crate) fn new(base_url: &str, api_key: &str, cache_ttl: Duration) -> Result<Option<Self>> {
        let Some(cache_root) = default_user_data_root() else {
            return Ok(None);
        };

        let cache_key = cache_key(base_url, api_key)?;
        Ok(Some(Self {
            cache_path: cache_root
                .join("model_catalog_cache")
                .join(format!("{cache_key}.json")),
            cache_ttl,
        }))
    }

    pub(crate) async fn load_fresh(&self) -> Result<Option<ModelCatalogCacheEntry>> {
        let Some(entry) = self.load().await else {
            return Ok(None);
        };

        if entry.is_fresh(self.cache_ttl) {
            info!(
                cache_path = %self.cache_path.display(),
                "model catalog cache: cache hit"
            );
            return Ok(Some(entry));
        }

        info!(
            cache_path = %self.cache_path.display(),
            "model catalog cache: cache is stale"
        );
        Ok(None)
    }

    pub(crate) async fn load_any(&self) -> Result<Option<ModelCatalogCacheEntry>> {
        Ok(self.load().await)
    }

    pub(crate) async fn store(&self, catalog: &ModelCatalog) {
        let entry = ModelCatalogCacheEntry::new(catalog.clone());
        if let Err(err) = self.save(&entry).await {
            error!("failed to write model catalog cache: {err}");
        }
    }

    pub(crate) fn cache_path(&self) -> &PathBuf {
        &self.cache_path
    }

    async fn load(&self) -> Option<ModelCatalogCacheEntry> {
        match fs::read(&self.cache_path).await {
            Ok(contents) => {
                match serde_json::from_slice(&contents) {
                    Ok(entry) => Some(entry),
                    Err(err) => {
                        warn!(
                            cache_path = %self.cache_path.display(),
                            error = %err,
                            "invalid model catalog cache"
                        );
                        None
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                warn!(
                    cache_path = %self.cache_path.display(),
                    error = %err,
                    "failed to read model catalog cache"
                );
                None
            }
        }
    }

    async fn save(&self, entry: &ModelCatalogCacheEntry) -> Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let bytes = serde_json::to_vec_pretty(entry)?;
        fs::write(&self.cache_path, bytes).await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModelCatalogCacheEntry {
    pub(crate) fetched_at: chrono::DateTime<chrono::Utc>,
    pub(crate) source_url: String,
    pub(crate) models: Vec<String>,
}

impl ModelCatalogCacheEntry {
    fn new(catalog: ModelCatalog) -> Self {
        Self {
            fetched_at: chrono::Utc::now(),
            source_url: catalog.source_url,
            models: catalog.models,
        }
    }

    fn is_fresh(&self, ttl: Duration) -> bool {
        if ttl.is_zero() {
            return false;
        }
        let Ok(ttl) = chrono::Duration::from_std(ttl) else {
            return false;
        };
        chrono::Utc::now().signed_duration_since(self.fetched_at) <= ttl
    }

    pub(crate) fn into_catalog(self) -> ModelCatalog {
        ModelCatalog {
            source_url: self.source_url,
            models: self.models,
        }
    }
}

fn cache_key(base_url: &str, api_key: &str) -> Result<String> {
    let normalized_base_url = normalize_base_url(base_url)?;
    let fingerprint = api_key.trim();
    let input = format!("{normalized_base_url}\n{fingerprint}");
    Ok(blake3::hash(input.as_bytes()).to_hex().to_string())
}

fn normalize_base_url(base_url: &str) -> Result<String> {
    let parsed = Url::parse(base_url).context("Base URL is not a valid URL")?;
    let mut normalized = parsed.clone();
    if let Some(stripped) = strip_known_model_path(&parsed) {
        normalized = stripped;
    }
    normalized.set_query(None);
    normalized.set_fragment(None);
    let path = normalized.path().trim_end_matches('/');
    let normalized_path = if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    normalized.set_path(&normalized_path);
    Ok(normalized.to_string())
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

#[cfg(test)]
mod tests {
    use super::{cache_key, normalize_base_url};

    #[test]
    fn normalize_base_url_strips_known_model_paths() {
        assert_eq!(
            normalize_base_url("https://example.com/openai/chat/completions?x=1#frag")
                .expect("normalized"),
            "https://example.com/openai"
        );
        assert_eq!(
            normalize_base_url("https://api.openai.com/v1/").expect("normalized"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn cache_key_changes_when_credentials_change() {
        let base_url = "https://api.openai.com/v1";
        let first = cache_key(base_url, "one").expect("key");
        let second = cache_key(base_url, "two").expect("key");
        assert_ne!(first, second);
    }
}
