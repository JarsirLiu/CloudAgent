use super::fragments::ContextFragment;
use crate::conversation::ResponseItem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentContext {
    pub cwd: PathBuf,
    pub shell: String,
    pub current_date: String,
    pub timezone: String,
}

impl EnvironmentContext {
    pub fn new(
        cwd: impl Into<PathBuf>,
        shell: impl Into<String>,
        current_date: impl Into<String>,
        timezone: impl Into<String>,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            shell: shell.into(),
            current_date: current_date.into(),
            timezone: timezone.into(),
        }
    }

    pub fn render_text(&self) -> String {
        format!(
            "<environment_context>\n  <cwd>{}</cwd>\n  <shell>{}</shell>\n  <current_date>{}</current_date>\n  <timezone>{}</timezone>\n</environment_context>",
            self.cwd.display(),
            self.shell,
            self.current_date,
            self.timezone
        )
    }
}

impl ContextFragment for EnvironmentContext {
    fn render(&self) -> ResponseItem {
        ResponseItem::User {
            content: self.render_text(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_codex_style_environment_context() {
        let context = EnvironmentContext::new(
            r"D:\learn\gifti\cloudagent",
            "powershell",
            "2026-04-30",
            "+08:00",
        );

        assert_eq!(
            context.render_text(),
            "<environment_context>\n  <cwd>D:\\learn\\gifti\\cloudagent</cwd>\n  <shell>powershell</shell>\n  <current_date>2026-04-30</current_date>\n  <timezone>+08:00</timezone>\n</environment_context>"
        );
    }
}
