use agent_protocol::FrontendMode;

pub(crate) fn status_text_from_mode(mode: FrontendMode) -> &'static str {
    match mode {
        FrontendMode::Idle => "Ready",
        FrontendMode::Running => "Working",
        FrontendMode::WaitingForServerRequest => "Action required",
    }
}
