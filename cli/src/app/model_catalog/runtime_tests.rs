use super::{ModelCatalogProvider, ModelCatalogRuntime};
use crate::app::model_catalog::{ModelCatalog, ModelCatalogSnapshotState, ModelCatalogSource};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Mutex, Notify};

struct FakeProvider {
    fresh: Mutex<Option<ModelCatalog>>,
    any: Mutex<Option<ModelCatalog>>,
    network: Mutex<std::result::Result<ModelCatalog, String>>,
    network_calls: AtomicUsize,
    network_notify: Notify,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self {
            fresh: Mutex::new(None),
            any: Mutex::new(None),
            network: Mutex::new(Err("network unavailable".to_string())),
            network_calls: AtomicUsize::new(0),
            network_notify: Notify::new(),
        }
    }
}

impl FakeProvider {
    fn with_fresh(catalog: ModelCatalog) -> Arc<Self> {
        Arc::new(Self {
            fresh: Mutex::new(Some(catalog)),
            network: Mutex::new(Err("unused".to_string())),
            ..Default::default()
        })
    }

    fn with_stale_and_network(stale: ModelCatalog, network: ModelCatalog) -> Arc<Self> {
        Arc::new(Self {
            any: Mutex::new(Some(stale)),
            network: Mutex::new(Ok(network)),
            ..Default::default()
        })
    }

    fn with_network(network: ModelCatalog) -> Arc<Self> {
        Arc::new(Self {
            network: Mutex::new(Ok(network)),
            ..Default::default()
        })
    }

    async fn wait_for_network(&self) {
        if self.network_calls.load(Ordering::SeqCst) > 0 {
            return;
        }
        self.network_notify.notified().await;
    }
}

#[async_trait]
impl ModelCatalogProvider for FakeProvider {
    async fn load_fresh_cache(&self) -> Result<Option<ModelCatalog>> {
        Ok(self.fresh.lock().await.clone())
    }

    async fn load_any_cache(&self) -> Result<Option<ModelCatalog>> {
        Ok(self.any.lock().await.clone())
    }

    async fn refresh_network(&self) -> Result<ModelCatalog> {
        self.network_calls.fetch_add(1, Ordering::SeqCst);
        self.network_notify.notify_waiters();
        match self.network.lock().await.clone() {
            Ok(catalog) => Ok(catalog),
            Err(err) => Err(anyhow!(err)),
        }
    }
}

fn catalog(source_url: &str, models: &[&str]) -> ModelCatalog {
    ModelCatalog {
        source_url: source_url.to_string(),
        models: models.iter().map(|model| model.to_string()).collect(),
    }
}

#[tokio::test]
async fn snapshot_returns_empty_initially() {
    let runtime = ModelCatalogRuntime::new();
    assert_eq!(runtime.snapshot().await, ModelCatalogSnapshotState::Empty);
}

#[tokio::test]
async fn load_for_picker_uses_fresh_cache_without_network() {
    let provider = FakeProvider::with_fresh(catalog("cache", &["a"]));
    let runtime = ModelCatalogRuntime::new();

    let state = runtime
        .load_for_picker_with_provider(provider.clone())
        .await;

    let ModelCatalogSnapshotState::Ready(snapshot) = state else {
        panic!("expected ready state");
    };
    assert_eq!(snapshot.source, ModelCatalogSource::FreshCache);
    assert_eq!(snapshot.catalog.models, vec!["a".to_string()]);
    assert_eq!(provider.network_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn load_for_picker_uses_stale_cache_and_refreshes_in_background() {
    let provider =
        FakeProvider::with_stale_and_network(catalog("stale", &["old"]), catalog("net", &["new"]));
    let runtime = ModelCatalogRuntime::new();

    let state = runtime
        .load_for_picker_with_provider(provider.clone())
        .await;

    let ModelCatalogSnapshotState::Ready(snapshot) = state else {
        panic!("expected stale ready state");
    };
    assert_eq!(snapshot.source, ModelCatalogSource::StaleCache);
    assert_eq!(snapshot.catalog.models, vec!["old".to_string()]);

    provider.wait_for_network().await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let ModelCatalogSnapshotState::Ready(snapshot) = runtime.snapshot().await else {
        panic!("expected refreshed ready state");
    };
    assert_eq!(snapshot.source, ModelCatalogSource::Network);
    assert_eq!(snapshot.catalog.models, vec!["new".to_string()]);
}

#[tokio::test]
async fn load_for_picker_returns_loading_when_no_cache() {
    let provider = FakeProvider::with_network(catalog("net", &["new"]));
    let runtime = ModelCatalogRuntime::new();

    let state = runtime
        .load_for_picker_with_provider(provider.clone())
        .await;

    assert_eq!(state, ModelCatalogSnapshotState::Loading);
    provider.wait_for_network().await;
}

#[tokio::test]
async fn memory_snapshot_returns_immediately() {
    let provider = FakeProvider::with_fresh(catalog("cache", &["a"]));
    let runtime = ModelCatalogRuntime::new();
    runtime.load_for_picker_with_provider(provider).await;

    let next_provider = FakeProvider::with_network(catalog("net", &["b"]));
    let state = runtime
        .load_for_picker_with_provider(next_provider.clone())
        .await;

    let ModelCatalogSnapshotState::Ready(snapshot) = state else {
        panic!("expected ready state");
    };
    assert_eq!(snapshot.source, ModelCatalogSource::Memory);
    assert_eq!(snapshot.catalog.models, vec!["a".to_string()]);
    assert_eq!(next_provider.network_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn reset_clears_snapshot() {
    let provider = FakeProvider::with_fresh(catalog("cache", &["a"]));
    let runtime = ModelCatalogRuntime::new();
    runtime.load_for_picker_with_provider(provider).await;

    runtime.reset().await;

    assert_eq!(runtime.snapshot().await, ModelCatalogSnapshotState::Empty);
}
