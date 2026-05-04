use anyhow::Result;
use infra_store::memory_repo::FileMemoryRepo;
use std::path::PathBuf;

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

    pub fn persist_sop_hint(&self, summary: &str) -> Result<()> {
        let hint = summary.lines().next().unwrap_or("").trim();
        if hint.is_empty() {
            return Ok(());
        }
        self.repo.append_l3_sop("Reusable flow", hint)
    }

    pub fn archive_session_line(&self, line: &str) -> Result<()> {
        self.repo.append_l4_archive(line)
    }
}
