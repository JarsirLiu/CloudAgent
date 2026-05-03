use anyhow::Result;
use serde::Serialize;
use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
pub(crate) struct ContextBudgetLogEntry {
    pub conversation_id: String,
    pub turn_id: String,
    pub model_context_window: u64,
    pub trigger_ratio: f32,
    pub trigger_tokens: usize,
    pub estimated_total_tokens: usize,
    pub sdk_total_tokens: Option<usize>,
    pub history_tokens: usize,
    pub overhead_tokens: usize,
    pub memory_floor_tokens: usize,
    pub safety_buffer_tokens: usize,
    pub compaction_triggered: bool,
    pub hard_cap_triggered: bool,
    pub memory_before: usize,
    pub memory_after: usize,
    pub skills_before: usize,
    pub skills_after: usize,
    pub mcp_before: usize,
    pub mcp_after: usize,
}

pub(crate) fn append_context_budget_log(
    workspace_root: &Path,
    entry: &ContextBudgetLogEntry,
) -> Result<()> {
    let dir = workspace_root.join("data").join("logs");
    create_dir_all(&dir)?;
    let file = dir.join("context_budget.jsonl");
    let mut handle = OpenOptions::new().create(true).append(true).open(file)?;
    let line = serde_json::to_string(entry)?;
    handle.write_all(line.as_bytes())?;
    handle.write_all(b"\n")?;
    Ok(())
}
