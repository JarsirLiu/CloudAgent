use anyhow::Result;
use std::path::PathBuf;
use storage::memory_repo::FileMemoryRepo;

pub struct MemoryService {
    repo: FileMemoryRepo,
}

impl MemoryService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            repo: FileMemoryRepo::new(root),
        }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        self.repo.ensure_layout()
    }

    pub fn read_l1_index(&self) -> Result<Option<String>> {
        self.repo.read_l1_index()
    }

    pub fn persist_summary_fact(&self, summary: &str) -> Result<()> {
        let normalized = summary
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(220)
            .collect::<String>();
        if normalized.is_empty() {
            return Ok(());
        }
        self.repo.append_l2_fact(&format!("- {}", normalized))
    }
}
