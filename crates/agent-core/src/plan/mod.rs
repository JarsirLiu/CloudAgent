use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TurnPlan {
    pub summary: Option<String>,
    pub tool_names: Vec<String>,
}

pub fn module_name() -> &'static str {
    "agent-core::plan"
}
