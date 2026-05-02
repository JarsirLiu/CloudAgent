use std::path::PathBuf;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MemoryMode {
    Off,
    Basic,
    Evolve,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub mode: MemoryMode,
    pub root_dir: PathBuf,
    pub max_inject_chars: usize,
    pub min_turns_to_persist: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: MemoryMode::Off,
            root_dir: PathBuf::from("data/state/memory"),
            max_inject_chars: 6_000,
            min_turns_to_persist: 8,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadPlan {
    pub inject_prefix: Option<String>,
}
