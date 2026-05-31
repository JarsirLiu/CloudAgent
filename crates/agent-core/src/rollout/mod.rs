use crate::context::CompactionSummary;
use crate::conversation::ResponseItem;
use crate::turn::{CompactionContinuation, EventMsg};
use serde::{Deserialize, Serialize};

pub mod policy;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RolloutItem {
    EventMsg {
        event: EventMsg,
    },
    ResponseItem {
        item: ResponseItem,
    },
    Compacted {
        summary: CompactionSummary,
        rendered_summary: String,
        continuation: CompactionContinuation,
        #[serde(default)]
        replacement_history: Vec<ResponseItem>,
    },
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
