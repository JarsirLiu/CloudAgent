use agent_protocol::{AppServerEvent, FrontendMode, HistoryEntry, TurnEvent};
use anyhow::Result;
use tokio::io::{self, AsyncWriteExt};

#[derive(Clone, Debug)]
pub struct ConsoleBanner {
    pub title: String,
    pub commands: String,
    pub idle_prompt: String,
    pub approval_prompt: String,
}

impl ConsoleBanner {
    pub fn cli(session_id: &str) -> Self {
        Self {
            title: format!("cloudagent session `{session_id}`"),
            commands: "commands: /exit /reset /history /status /interrupt".to_string(),
            idle_prompt: "you> ".to_string(),
            approval_prompt: "approve> ".to_string(),
        }
    }

    pub fn daemon(session_id: &str) -> Self {
        Self {
            title: format!("agentd console attached to session `{session_id}`"),
            commands: "commands: /exit /interrupt".to_string(),
            idle_prompt: "daemon-you> ".to_string(),
            approval_prompt: "daemon-approve> ".to_string(),
        }
    }
}

pub(crate) struct ConsoleRenderer {
    banner: ConsoleBanner,
    stdout: io::Stdout,
}

impl ConsoleRenderer {
    pub(crate) fn new(banner: ConsoleBanner) -> Self {
        Self {
            banner,
            stdout: io::stdout(),
        }
    }

    pub(crate) async fn render_banner(&mut self) -> Result<()> {
        println!("{}", self.banner.title);
        println!("{}", self.banner.commands);
        self.render_prompt(FrontendMode::Idle).await
    }

    pub(crate) async fn render_protocol_event(&mut self, event: &AppServerEvent) -> Result<()> {
        match event {
            AppServerEvent::FrontendStateChanged { .. } => {}
            AppServerEvent::SessionStatus { snapshot, .. } => {
                println!(
                    "session> state={:?} active_turn={:?} turn_state={:?} messages={}",
                    snapshot.session_state,
                    snapshot.active_turn,
                    snapshot.turn_state,
                    snapshot.message_count
                );
            }
            AppServerEvent::SessionHistory { messages, .. } => {
                for message in messages {
                    render_history(message);
                }
            }
            AppServerEvent::Info { message, .. } => {
                println!("session> {message}");
            }
            AppServerEvent::Error { message, .. } => {
                println!("session> error: {message}");
            }
            AppServerEvent::ApprovalPrompt { request, .. } => {
                println!(
                    "approval> tool `{}` wants to run with args {}",
                    request.tool_name, request.arguments_preview
                );
                println!("approval> {}", request.reason);
                println!("approval> respond with `y`/`n`, or use /interrupt");
            }
            AppServerEvent::TurnFinished { result, .. } => match &result.error {
                Some(error) => println!("turn> failed: {error}"),
                None => println!("agent> {}", result.final_response),
            },
            AppServerEvent::TurnEvent { event, .. } => render_turn_event(event),
        }
        Ok(())
    }

    pub(crate) async fn render_prompt(&mut self, mode: FrontendMode) -> Result<()> {
        let prompt = match mode {
            FrontendMode::Idle => Some(self.banner.idle_prompt.as_str()),
            FrontendMode::WaitingForApproval => Some(self.banner.approval_prompt.as_str()),
            FrontendMode::Running => None,
        };

        if let Some(prompt) = prompt {
            self.stdout.write_all(prompt.as_bytes()).await?;
            self.stdout.flush().await?;
        }
        Ok(())
    }
}

fn render_history(message: &HistoryEntry) {
    match message {
        HistoryEntry::System { content } => println!("system> {content}"),
        HistoryEntry::User { content } => println!("you> {content}"),
        HistoryEntry::Assistant { content, has_tool_calls } => {
            if let Some(content) = content {
                println!("agent> {content}");
            } else if *has_tool_calls {
                println!("agent> [tool call]");
            } else {
                println!("agent> ");
            }
        }
        HistoryEntry::Tool { name, content, .. } => println!("tool:{name}> {content}"),
    }
}

fn render_turn_event(event: &TurnEvent) {
    match event {
        TurnEvent::TurnStarted { turn_id, .. } => println!("turn> started {turn_id}"),
        TurnEvent::ModelRequestStarted {
            message_count,
            tool_count,
            ..
        } => println!(
            "model> requesting completion with {message_count} messages and {tool_count} tools"
        ),
        TurnEvent::ModelResponseReceived {
            model_name,
            tool_call_count,
            ..
        } => println!(
            "model> responded from {} with {} tool calls",
            model_name.as_deref().unwrap_or("unknown-model"),
            tool_call_count
        ),
        TurnEvent::ToolCallRequested { call, .. } => println!("tool:{}> requested", call.name),
        TurnEvent::ApprovalResolved {
            tool_call_id,
            approved,
            ..
        } => println!(
            "approval> {} for {}",
            if *approved { "approved" } else { "denied" },
            tool_call_id
        ),
        TurnEvent::ToolCallCompleted { result, .. } => {
            println!("tool:{}> {}", result.name, result.summary)
        }
        TurnEvent::ToolCallFailed {
            tool_name, error, ..
        } => println!("tool:{}> failed: {}", tool_name, error),
        TurnEvent::TurnFailed { error, .. } => println!("turn> failed: {error}"),
        TurnEvent::TurnCancelled { reason, .. } => println!("turn> cancelled: {reason}"),
        TurnEvent::AssistantMessage { .. }
        | TurnEvent::ApprovalRequested { .. }
        | TurnEvent::TurnCompleted { .. } => {}
    }
}
