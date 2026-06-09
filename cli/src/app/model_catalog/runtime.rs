use crate::app::model_catalog::refresh::ModelCatalogRefreshService;
use crate::app::model_catalog::{ModelCatalog, ModelCatalogSource};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::RwLock;
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelCatalogSnapshot {
    pub(crate) catalog: ModelCatalog,
    pub(crate) source: ModelCatalogSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelCatalogSnapshotState {
    Ready(ModelCatalogSnapshot),
    Loading,
    Empty,
    Failed(String),
}

#[async_trait]
trait ModelCatalogProvider: Send + Sync + 'static {
    async fn load_fresh_cache(&self) -> Result<Option<ModelCatalog>>;
    async fn load_any_cache(&self) -> Result<Option<ModelCatalog>>;
    async fn refresh_network(&self) -> Result<ModelCatalog>;
}

#[async_trait]
impl ModelCatalogProvider for ModelCatalogRefreshService {
    async fn load_fresh_cache(&self) -> Result<Option<ModelCatalog>> {
        ModelCatalogRefreshService::load_fresh_cache(self).await
    }

    async fn load_any_cache(&self) -> Result<Option<ModelCatalog>> {
        ModelCatalogRefreshService::load_any_cache(self).await
    }

    async fn refresh_network(&self) -> Result<ModelCatalog> {
        ModelCatalogRefreshService::refresh_network(self).await
    }
}

#[derive(Clone)]
pub(crate) struct ModelCatalogRuntime {
    inner: Arc<ModelCatalogRuntimeInner>,
}

struct ModelCatalogRuntimeInner {
    state: RwLock<ModelCatalogSnapshotState>,
    version: AtomicU64,
    refresh_in_flight: AtomicBool,
}

impl ModelCatalogRuntime {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ModelCatalogRuntimeInner {
                state: RwLock::new(ModelCatalogSnapshotState::Empty),
                version: AtomicU64::new(0),
                refresh_in_flight: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) async fn snapshot(&self) -> ModelCatalogSnapshotState {
        self.inner.state.read().await.clone()
    }

    pub(crate) async fn snapshot_with_version(&self) -> (u64, ModelCatalogSnapshotState) {
        (self.version(), self.snapshot().await)
    }

    pub(crate) fn version(&self) -> u64 {
        self.inner.version.load(Ordering::SeqCst)
    }

    pub(crate) async fn reset(&self) {
        self.set_state(ModelCatalogSnapshotState::Empty).await;
    }

    pub(crate) fn spawn_prewarm(&self, base_url: String, api_key: String) {
        let Ok(provider) = ModelCatalogRefreshService::new(base_url, api_key) else {
            return;
        };
        self.spawn_prewarm_with_provider(Arc::new(provider));
    }

    pub(crate) async fn load_for_picker(
        &self,
        base_url: String,
        api_key: String,
    ) -> ModelCatalogSnapshotState {
        let Ok(provider) = ModelCatalogRefreshService::new(base_url, api_key) else {
            let state =
                ModelCatalogSnapshotState::Failed("Base URL is not a valid URL".to_string());
            self.set_state(state.clone()).await;
            return state;
        };
        self.load_for_picker_with_provider(Arc::new(provider)).await
    }

    async fn set_state(&self, state: ModelCatalogSnapshotState) {
        *self.inner.state.write().await = state;
        self.inner.version.fetch_add(1, Ordering::SeqCst);
    }

    async fn set_ready(&self, catalog: ModelCatalog, source: ModelCatalogSource) {
        self.set_state(ModelCatalogSnapshotState::Ready(ModelCatalogSnapshot {
            catalog,
            source,
        }))
        .await;
    }

    fn spawn_prewarm_with_provider(&self, provider: Arc<dyn ModelCatalogProvider>) {
        let runtime = self.clone();
        tokio::spawn(async move {
            if let Ok(Some(catalog)) = provider.load_fresh_cache().await {
                runtime
                    .set_ready(catalog, ModelCatalogSource::FreshCache)
                    .await;
                return;
            }

            let had_stale = match provider.load_any_cache().await {
                Ok(Some(catalog)) => {
                    runtime
                        .set_ready(catalog, ModelCatalogSource::StaleCache)
                        .await;
                    true
                }
                Ok(None) => false,
                Err(err) => {
                    warn!("failed to load model catalog cache during prewarm: {err}");
                    false
                }
            };

            runtime.spawn_refresh_with_provider(provider, !had_stale);
        });
    }

    fn spawn_refresh_with_provider(
        &self,
        provider: Arc<dyn ModelCatalogProvider>,
        mark_loading: bool,
    ) {
        if self.inner.refresh_in_flight.swap(true, Ordering::SeqCst) {
            return;
        }

        let runtime = self.clone();
        tokio::spawn(async move {
            if mark_loading {
                runtime.set_state(ModelCatalogSnapshotState::Loading).await;
            }

            match provider.refresh_network().await {
                Ok(catalog) => {
                    runtime
                        .set_ready(catalog, ModelCatalogSource::Network)
                        .await;
                }
                Err(err) => {
                    let current = runtime.snapshot().await;
                    if matches!(
                        current,
                        ModelCatalogSnapshotState::Loading | ModelCatalogSnapshotState::Empty
                    ) {
                        runtime
                            .set_state(ModelCatalogSnapshotState::Failed(err.to_string()))
                            .await;
                    } else {
                        warn!("failed to refresh model catalog: {err}");
                    }
                }
            }
            runtime
                .inner
                .refresh_in_flight
                .store(false, Ordering::SeqCst);
        });
    }

    async fn load_for_picker_with_provider(
        &self,
        provider: Arc<dyn ModelCatalogProvider>,
    ) -> ModelCatalogSnapshotState {
        if let ModelCatalogSnapshotState::Ready(snapshot) = self.snapshot().await {
            return ModelCatalogSnapshotState::Ready(ModelCatalogSnapshot {
                catalog: snapshot.catalog,
                source: ModelCatalogSource::Memory,
            });
        }

        match provider.load_fresh_cache().await {
            Ok(Some(catalog)) => {
                self.set_ready(catalog.clone(), ModelCatalogSource::FreshCache)
                    .await;
                return ModelCatalogSnapshotState::Ready(ModelCatalogSnapshot {
                    catalog,
                    source: ModelCatalogSource::FreshCache,
                });
            }
            Ok(None) => {}
            Err(err) => warn!("failed to load fresh model catalog cache: {err}"),
        }

        match provider.load_any_cache().await {
            Ok(Some(catalog)) => {
                self.set_ready(catalog.clone(), ModelCatalogSource::StaleCache)
                    .await;
                self.spawn_refresh_with_provider(provider, false);
                return ModelCatalogSnapshotState::Ready(ModelCatalogSnapshot {
                    catalog,
                    source: ModelCatalogSource::StaleCache,
                });
            }
            Ok(None) => {}
            Err(err) => warn!("failed to load model catalog cache: {err}"),
        }

        self.spawn_refresh_with_provider(provider, true);
        ModelCatalogSnapshotState::Loading
    }
}

impl Default for ModelCatalogRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
