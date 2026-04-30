use crate::conversation::ResponseItem;
use crate::turn::EventMsg;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RolloutItem {
    EventMsg { event: EventMsg },
    ResponseItem { item: ResponseItem },
    Compacted { summary: String },
    SessionMeta { conversation_id: String },
}

impl From<EventMsg> for RolloutItem {
    fn from(event: EventMsg) -> Self {
        Self::EventMsg { event }
    }
}

impl From<ResponseItem> for RolloutItem {
    fn from(item: ResponseItem) -> Self {
        Self::ResponseItem { item }
    }
}

pub fn module_name() -> &'static str {
    "agent-core::rollout"
}
