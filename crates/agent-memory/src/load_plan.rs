use crate::model::{LoadPlan, MemoryConfig, MemoryMode};
use crate::policy::sanitize_memory_text;
use crate::service::MemoryService;
use anyhow::Result;

pub fn build_load_plan(config: &MemoryConfig, service: &MemoryService) -> Result<LoadPlan> {
    if !config.enabled || config.mode == MemoryMode::Off {
        return Ok(LoadPlan {
            inject_prefix: None,
        });
    }

    let inject_prefix = service
        .read_l1_index()?
        .map(|raw| sanitize_memory_text(&raw, config.max_inject_chars))
        .filter(|s| !s.is_empty())
        .map(|s| format!("\n[LONG_TERM_MEMORY:L1]\n{s}\n"));

    Ok(LoadPlan { inject_prefix })
}
