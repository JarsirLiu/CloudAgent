use agent_protocol::RequestId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ServerRequestKind {
    Command,
    FileChange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServerRequestPresentation {
    pub(crate) kind: ServerRequestKind,
    pub(crate) tool_name: String,
    pub(crate) reason: String,
    pub(crate) preview: String,
}

impl ServerRequestPresentation {
    pub(crate) fn command(
        tool_name: impl Into<String>,
        reason: impl Into<String>,
        preview: impl Into<String>,
    ) -> Self {
        Self {
            kind: ServerRequestKind::Command,
            tool_name: tool_name.into(),
            reason: reason.into(),
            preview: preview.into(),
        }
    }

    pub(crate) fn file_change(
        tool_name: impl Into<String>,
        reason: impl Into<String>,
        preview: impl Into<String>,
    ) -> Self {
        Self {
            kind: ServerRequestKind::FileChange,
            tool_name: tool_name.into(),
            reason: reason.into(),
            preview: preview.into(),
        }
    }

    pub(crate) fn notice_text(&self) -> String {
        match self.kind {
            ServerRequestKind::Command => format!("Command approval required for {}", self.tool_name),
            ServerRequestKind::FileChange => {
                format!("File change approval required for {}", self.tool_name)
            }
        }
    }

    pub(crate) fn title_text(&self) -> String {
        match self.kind {
            ServerRequestKind::Command => format!("{} wants to run", self.tool_name),
            ServerRequestKind::FileChange => format!("{} wants to edit files", self.tool_name),
        }
    }

    pub(crate) fn preview_text(&self) -> &str {
        self.preview.trim()
    }

    pub(crate) fn reason_text(&self) -> &str {
        self.reason.trim()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ServerRequestInlineState {
    pub(crate) request_id: RequestId,
    pub(crate) presentation: ServerRequestPresentation,
}

#[cfg(test)]
mod tests {
    use super::{ServerRequestKind, ServerRequestPresentation};

    #[test]
    fn command_presentation_is_always_command_kind() {
        let presentation = ServerRequestPresentation::command(
            "exec_command",
            "Network commands require approval under the current approval policy.",
            "curl https://example.com",
        );

        assert_eq!(presentation.kind, ServerRequestKind::Command);
        assert_eq!(presentation.notice_text(), "Command approval required for exec_command");
        assert_eq!(presentation.title_text(), "exec_command wants to run");
    }

    #[test]
    fn file_change_presentation_stays_file_change_kind() {
        let presentation =
            ServerRequestPresentation::file_change("edit_file", "needs review", "patch");

        assert_eq!(presentation.kind, ServerRequestKind::FileChange);
        assert_eq!(presentation.notice_text(), "File change approval required for edit_file");
        assert_eq!(presentation.title_text(), "edit_file wants to edit files");
    }
}
