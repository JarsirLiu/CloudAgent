use super::fragments::ContextFragment;
use crate::conversation::ResponseItem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentContext {
    pub cwd: PathBuf,
    pub shell: String,
    pub current_date: String,
    pub current_time: String,
    pub current_datetime: String,
    pub timezone: String,
}

impl EnvironmentContext {
    pub fn new(
        cwd: impl Into<PathBuf>,
        shell: impl Into<String>,
        current_date: impl Into<String>,
        current_time: impl Into<String>,
        current_datetime: impl Into<String>,
        timezone: impl Into<String>,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            shell: shell.into(),
            current_date: current_date.into(),
            current_time: current_time.into(),
            current_datetime: current_datetime.into(),
            timezone: timezone.into(),
        }
    }

    pub fn render_text(&self) -> String {
        format!(
            "<environment_context>\n  <cwd>{}</cwd>\n  <shell>{}</shell>\n  <current_date>{}</current_date>\n  <current_time>{}</current_time>\n  <current_datetime>{}</current_datetime>\n  <timezone>{}</timezone>\n</environment_context>",
            self.cwd.display(),
            self.shell,
            self.current_date,
            self.current_time,
            self.current_datetime,
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
            "19:16:01",
            "2026-04-30T19:16:01+08:00",
            "+08:00",
        );

        assert_eq!(
            context.render_text(),
            "<environment_context>\n  <cwd>D:\\learn\\gifti\\cloudagent</cwd>\n  <shell>powershell</shell>\n  <current_date>2026-04-30</current_date>\n  <current_time>19:16:01</current_time>\n  <current_datetime>2026-04-30T19:16:01+08:00</current_datetime>\n  <timezone>+08:00</timezone>\n</environment_context>"
        );
    }
}
