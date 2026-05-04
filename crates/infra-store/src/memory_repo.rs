use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FileMemoryRepo {
    root: PathBuf,
}

impl FileMemoryRepo {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in ["l0", "l1", "l2", "l3", "l4"] {
            fs::create_dir_all(self.root.join(dir))?;
        }
        self.ensure_seed_files()?;
        Ok(())
    }

    pub fn read_l1_index(&self) -> Result<Option<String>> {
        read_optional_text(&self.root.join("l1").join("insight.md"))
    }

    pub fn append_l2_fact(&self, line: &str) -> Result<()> {
        let path = self.root.join("l2").join("global_facts.md");
        let mut content = read_optional_text(&path)?.unwrap_or_else(|| "# Global Facts\n\n".to_string());
        if content.lines().any(|l| l.trim() == line.trim()) {
            return Ok(());
        }
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        content.push('\n');
        fs::write(path, content)?;
        Ok(())
    }

    pub fn append_l3_sop(&self, title: &str, body: &str) -> Result<()> {
        let path = self.root.join("l3").join("auto_sop.md");
        let mut content =
            read_optional_text(&path)?.unwrap_or_else(|| "# Auto SOP\n\n".to_string());
        let block = format!("## {title}\n- {body}\n\n");
        if content.contains(&block) {
            return Ok(());
        }
        content.push_str(&block);
        fs::write(path, content)?;
        Ok(())
    }

    pub fn append_l4_archive(&self, line: &str) -> Result<()> {
        let path = self.root.join("l4").join("session_archive.log");
        let mut content = read_optional_text(&path)?.unwrap_or_default();
        content.push_str(line.trim());
        content.push('\n');
        fs::write(path, content)?;
        Ok(())
    }

    fn ensure_seed_files(&self) -> Result<()> {
        let l0 = self.root.join("l0").join("memory_rules.md");
        if !l0.exists() {
            fs::write(
                l0,
                "# Memory Rules\n\n- Only persist action-verified, reusable facts.\n- Never store volatile values.\n- Prefer minimal patch-like updates.\n",
            )?;
        }
        let l1 = self.root.join("l1").join("insight.md");
        if !l1.exists() {
            fs::write(l1, "# Insight Index\n\n- facts -> l2/global_facts.md\n- sop -> l3/auto_sop.md\n")?;
        }
        Ok(())
    }
}

fn read_optional_text(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}
