use crate::app::model_catalog::cache::ModelCatalogCacheManager;
use crate::app::model_catalog::source::fetch_model_catalog_from_network;
use crate::app::model_catalog::{MODEL_CATALOG_CACHE_TTL, ModelCatalog};
use anyhow::Result;

#[derive(Debug, Clone)]
pub(crate) struct ModelCatalogRefreshService {
    base_url: String,
    api_key: String,
    cache: Option<ModelCatalogCacheManager>,
}

impl ModelCatalogRefreshService {
    pub(crate) fn new(base_url: String, api_key: String) -> Result<Self> {
        let cache = ModelCatalogCacheManager::new(&base_url, &api_key, MODEL_CATALOG_CACHE_TTL)?;
        Ok(Self {
            base_url,
            api_key,
            cache,
        })
    }

    pub(crate) async fn load_fresh_cache(&self) -> Result<Option<ModelCatalog>> {
        let Some(cache) = self.cache.as_ref() else {
            return Ok(None);
        };
        cache.load_fresh_catalog().await
    }

    pub(crate) async fn load_any_cache(&self) -> Result<Option<ModelCatalog>> {
        let Some(cache) = self.cache.as_ref() else {
            return Ok(None);
        };
        cache.load_any_catalog().await
    }

    pub(crate) async fn refresh_network(&self) -> Result<ModelCatalog> {
        let catalog = fetch_model_catalog_from_network(&self.base_url, &self.api_key).await?;
        if let Some(cache) = self.cache.as_ref() {
            cache.store(&catalog).await;
        }
        Ok(catalog)
    }
}
