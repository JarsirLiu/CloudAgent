use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserTask {
    pub prompt: String,
}

pub fn module_name() -> &'static str {
    "agent-core::task"
}
