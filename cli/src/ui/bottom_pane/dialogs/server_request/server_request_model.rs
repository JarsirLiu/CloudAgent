use agent_protocol::RequestId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ServerRequestKind {
    Command,
    Network,
    Permissions,
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
        let tool_name = tool_name.into();
        let reason = reason.into();
        let preview = preview.into();
        Self {
            kind: infer_command_kind(&tool_name, &reason, &preview),
            tool_name,
            reason,
            preview,
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
            ServerRequestKind::Command => {
                format!("Command approval required for {}", self.tool_name)
            }
            ServerRequestKind::Network => {
                format!("Network approval required for {}", self.tool_name)
            }
            ServerRequestKind::Permissions => {
                format!("Permission approval required for {}", self.tool_name)
            }
            ServerRequestKind::FileChange => {
                format!("File change approval required for {}", self.tool_name)
            }
        }
    }

    pub(crate) fn title_text(&self) -> String {
        match self.kind {
            ServerRequestKind::Command => format!("{} wants to run", self.tool_name),
            ServerRequestKind::Network => format!("{} wants network access", self.tool_name),
            ServerRequestKind::Permissions => {
                format!("{} wants broader permissions", self.tool_name)
            }
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

fn infer_command_kind(tool_name: &str, reason: &str, preview: &str) -> ServerRequestKind {
    let reason_lower = reason.to_ascii_lowercase();
    let preview_lower = preview.to_ascii_lowercase();
    let tool_lower = tool_name.to_ascii_lowercase();

    if reason_lower.contains("network")
        || preview_lower.contains("http://")
        || preview_lower.contains("https://")
        || preview_lower.contains("curl ")
        || preview_lower.contains("wget ")
        || preview_lower.contains("invoke-webrequest")
        || preview_lower.contains("invoke-restmethod")
    {
        return ServerRequestKind::Network;
    }

    if reason_lower.contains("stronger permissions")
        || reason_lower.contains("outside the workspace")
        || reason_lower.contains("writing outside the workspace")
        || reason_lower.contains("read-only permissions")
        || tool_lower.contains("write_stdin")
    {
        return ServerRequestKind::Permissions;
    }

    ServerRequestKind::Command
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
    fn command_presentation_infers_network_requests() {
        let presentation = ServerRequestPresentation::command(
            "exec_command",
            "Network commands require approval under the current approval policy.",
            "curl https://example.com",
        );

        assert_eq!(presentation.kind, ServerRequestKind::Network);
        assert_eq!(
            presentation.notice_text(),
            "Network approval required for exec_command"
        );
        assert_eq!(
            presentation.title_text(),
            "exec_command wants network access"
        );
    }

    #[test]
    fn command_presentation_infers_permission_requests() {
        let presentation = ServerRequestPresentation::command(
            "write_stdin",
            "Interactive command input requires stronger permissions because it can modify files.",
            "exit",
        );

        assert_eq!(presentation.kind, ServerRequestKind::Permissions);
        assert_eq!(
            presentation.notice_text(),
            "Permission approval required for write_stdin"
        );
        assert_eq!(
            presentation.title_text(),
            "write_stdin wants broader permissions"
        );
    }
}
