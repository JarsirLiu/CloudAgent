pub(crate) mod cache;
mod refresh;
mod runtime;
mod source;

pub(crate) use runtime::{ModelCatalogRuntime, ModelCatalogSnapshot, ModelCatalogSnapshotState};
use std::time::Duration;

const MODEL_CATALOG_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelCatalog {
    pub(crate) source_url: String,
    pub(crate) models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelCatalogSource {
    Memory,
    FreshCache,
    StaleCache,
    Network,
}

#[cfg(test)]
mod source_tests;
