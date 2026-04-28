use agent_app_server_client::InProcessAppServerClient;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode,
    HistoryEntry, RequestId, TurnEvent, UserTurnInput,
};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};

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

#[derive(Clone, Debug)]
pub struct ConsoleConfig {
    pub session_id: String,
    pub banner: ConsoleBanner,
    pub auto_approve: bool,
    pub auto_approve_reason: Option<String>,
}

#[derive(Debug)]
enum ParsedInput {
    Command(AppClientCommand),
    ApprovalAnswer {
        approved: bool,
        reason: String,
    },
    Empty,
}

#[derive(Clone, Debug)]
struct ConsoleState {
    mode: FrontendMode,
    pending_approval_request_id: Option<RequestId>,
}

impl ConsoleState {
    fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
            pending_approval_request_id: None,
        }
    }

    fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }

    fn update_from_message(&mut self, message: &AppServerMessage) {
        match message {
            AppServerMessage::Notification(notification) => match notification {
                AppServerNotification::FrontendStateChanged { mode, .. } => {
                    self.mode = *mode;
                }
                AppServerNotification::TurnFinished { .. } => {
                    self.mode = FrontendMode::Idle;
                    self.pending_approval_request_id = None;
                }
                _ => {}
            },
            AppServerMessage::Request(AppServerRequest::Approval { request_id, .. }) => {
                self.mode = FrontendMode::WaitingForApproval;
                self.pending_approval_request_id = Some(request_id.clone());
            }
        }
    }
}

struct ConsoleRenderer {
    banner: ConsoleBanner,
    stdout: io::Stdout,
}

impl ConsoleRenderer {
    fn new(banner: ConsoleBanner) -> Self {
        Self {
            banner,
            stdout: io::stdout(),
        }
    }

    async fn render_banner(&mut self) -> Result<()> {
        println!("{}", self.banner.title);
        println!("{}", self.banner.commands);
        self.render_prompt(FrontendMode::Idle).await
    }

    async fn render_message(&mut self, message: &AppServerMessage) -> Result<()> {
        match message {
            AppServerMessage::Notification(notification) => match notification {
                AppServerNotification::FrontendStateChanged { .. } => {}
                AppServerNotification::SessionStatus { snapshot, .. } => {
                    println!(
                        "session> state={:?} active_turn={:?} turn_state={:?} messages={}",
                        snapshot.session_state,
                        snapshot.active_turn,
                        snapshot.turn_state,
                        snapshot.message_count
                    );
                }
                AppServerNotification::SessionHistory { messages, .. } => {
                    for message in messages {
                        render_history(message);
                    }
                }
                AppServerNotification::SubscriptionChanged {
                    session_id,
                    subscribed,
                } => {
                    println!(
                        "session> {} {}",
                        if *subscribed {
                            "subscribed to"
                        } else {
                            "unsubscribed from"
                        },
                        session_id
                    );
                }
                AppServerNotification::Info { message, .. } => {
                    println!("session> {message}");
                }
                AppServerNotification::Error { message, .. } => {
                    println!("session> error: {message}");
                }
                AppServerNotification::TurnFinished { result, .. } => match &result.error {
                    Some(error) => println!("turn> failed: {error}"),
                    None => println!("agent> {}", result.final_response),
                },
                AppServerNotification::TurnEvent { event, .. } => render_turn_event(event),
            },
            AppServerMessage::Request(AppServerRequest::Approval { request, .. }) => {
                println!(
                    "approval> tool `{}` wants to run with args {}",
                    request.tool_name, request.arguments_preview
                );
                println!("approval> {}", request.reason);
                println!("approval> respond with `y`/`n`, or use /interrupt");
            }
        }
        Ok(())
    }

    async fn render_prompt(&mut self, mode: FrontendMode) -> Result<()> {
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

pub async fn run_console(runtime: Arc<AgentRuntime>, config: ConsoleConfig) -> Result<()> {
    let session_id = config.session_id.clone();
    let mut client = InProcessAppServerClient::start(
        runtime,
        session_id.clone(),
        config.auto_approve,
        config.auto_approve_reason.clone(),
    );

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<ParsedInput>();
    let (mode_tx, mode_rx) = watch::channel(FrontendMode::Idle);
    let mut state = ConsoleState::new();
    let mut renderer = ConsoleRenderer::new(config.banner);

    spawn_input_loop(session_id.clone(), input_tx, mode_rx);
    renderer.render_banner().await?;

    loop {
        tokio::select! {
            Some(message) = client.next_message() => {
                state.update_from_message(&message);
                renderer.render_message(&message).await?;
                renderer.render_prompt(state.mode).await?;
                let _ = mode_tx.send(state.mode);
            }
            Some(input) = input_rx.recv() => {
                if handle_input(&session_id, &mut state, &mut renderer, &client, input).await? {
                    break;
                }
                let _ = mode_tx.send(state.mode);
            }
            else => break,
        }
    }

    client.shutdown().await
}

fn spawn_input_loop(
    session_id: String,
    input_tx: mpsc::UnboundedSender<ParsedInput>,
    mode_rx: watch::Receiver<FrontendMode>,
) {
    tokio::spawn(async move {
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();
        let mode_rx = mode_rx;

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) | Err(_) => {
                    let _ = input_tx.send(ParsedInput::Command(AppClientCommand::Exit));
                    break;
                }
            };

            let mode = *mode_rx.borrow();
            let _ = input_tx.send(parse_line(&line, &session_id, mode));
        }
    });
}

async fn handle_input(
    session_id: &str,
    state: &mut ConsoleState,
    renderer: &mut ConsoleRenderer,
    client: &InProcessAppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::Empty => {
            renderer.render_prompt(state.mode).await?;
        }
        ParsedInput::Command(command) => {
            if let AppClientCommand::Exit = command {
                if state.mode != FrontendMode::Idle {
                    client.send_command(AppClientCommand::InterruptTurn {
                        session_id: session_id.to_string(),
                    })?;
                }
                return Ok(true);
            }

            if matches!(command, AppClientCommand::SubmitTurn(_)) && !state.can_submit_turn() {
                renderer
                    .render_message(&AppServerMessage::Notification(
                        AppServerNotification::Info {
                            session_id: session_id.to_string(),
                            message:
                                "turn already running; wait, answer approval, or use /interrupt"
                                    .to_string(),
                        },
                    ))
                    .await?;
                renderer.render_prompt(state.mode).await?;
                return Ok(false);
            }

            if let AppClientCommand::ApprovalResponse { .. } = &command {
                state.mode = FrontendMode::Running;
                state.pending_approval_request_id = None;
            }
            if let AppClientCommand::SubmitTurn(_) = &command {
                state.mode = FrontendMode::Running;
            }

            client.send_command(command)?;
        }
        ParsedInput::ApprovalAnswer { approved, reason } => {
            let Some(request_id) = state.pending_approval_request_id.clone() else {
                renderer
                    .render_message(&AppServerMessage::Notification(
                        AppServerNotification::Error {
                            session_id: session_id.to_string(),
                            message: "no pending approval request".to_string(),
                        },
                    ))
                    .await?;
                renderer.render_prompt(state.mode).await?;
                return Ok(false);
            };

            state.mode = FrontendMode::Running;
            state.pending_approval_request_id = None;
            client.send_command(AppClientCommand::ApprovalResponse {
                session_id: session_id.to_string(),
                request_id,
                approved,
                reason: Some(reason),
            })?;
        }
    }

    Ok(false)
}

fn parse_line(line: &str, session_id: &str, mode: FrontendMode) -> ParsedInput {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedInput::Empty;
    }

    let command = match trimmed {
        "/exit" | "/quit" => AppClientCommand::Exit,
        "/reset" => AppClientCommand::ResetSession {
            session_id: session_id.to_string(),
        },
        "/history" => AppClientCommand::RequestHistory {
            session_id: session_id.to_string(),
        },
        "/status" => AppClientCommand::RequestStatus {
            session_id: session_id.to_string(),
        },
        "/interrupt" => AppClientCommand::InterruptTurn {
            session_id: session_id.to_string(),
        },
        _ if mode == FrontendMode::WaitingForApproval => {
            let approved = matches!(trimmed, "y" | "Y" | "yes" | "YES");
            return ParsedInput::ApprovalAnswer {
                approved,
                reason: if approved {
                    "approved by console operator".to_string()
                } else {
                    "denied by console operator".to_string()
                },
            };
        }
        _ => AppClientCommand::SubmitTurn(UserTurnInput {
            session_id: session_id.to_string(),
            content: trimmed.to_string(),
        }),
    };

    ParsedInput::Command(command)
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
