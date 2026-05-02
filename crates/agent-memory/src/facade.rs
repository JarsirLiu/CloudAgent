use crate::load_plan::build_load_plan;
use crate::model::{LoadPlan, MemoryConfig};
use crate::service::MemoryService;
use crate::trigger::should_persist;
use agent_core::{ConversationHistory, ResponseItem};
use anyhow::Result;

pub struct LongTermMemoryFacade {
    config: MemoryConfig,
    service: MemoryService,
}

impl LongTermMemoryFacade {
    pub fn new(config: MemoryConfig) -> Result<Self> {
        let service = MemoryService::new(config.root_dir.clone());
        service.ensure_layout()?;
        Ok(Self { config, service })
    }

    pub fn build_load_plan(&self) -> Result<LoadPlan> {
        build_load_plan(&self.config, &self.service)
    }

    pub fn should_persist(&self, history: &ConversationHistory) -> bool {
        should_persist(&self.config, history)
    }

    pub fn persist_from_history(&self, history: &ConversationHistory) -> Result<()> {
        let summary = history.messages.iter().rev().find_map(|m| match m {
            ResponseItem::Assistant { content: Some(c), .. } if !c.trim().is_empty() => {
                Some(c.trim().to_string())
            }
            _ => None,
        });
        if let Some(summary) = summary {
            self.service.persist_summary_fact(&summary)?;
        }
        Ok(())
    }
}
