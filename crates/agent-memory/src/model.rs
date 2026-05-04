pub use shared::{MemoryConfig, MemoryMode};

#[derive(Clone, Debug)]
pub struct LoadPlan {
    pub inject_prefix: Option<String>,
}
