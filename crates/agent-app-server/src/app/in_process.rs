use crate::app::runtime_manager::{AppRuntimeManager, FixedRuntimeManager};
use crate::routing::command_router::{ServerState, handle_command};
use crate::session::skills_watch::spawn_skill_watch;
use crate::session::state as session_state;
use agent_core::AgentHost;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, CommandExecutionContext,
    SessionBootstrapContext,
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum ServerMessage {
    Command {
        command: AppClientCommand,
        context: Option<CommandExecutionContext>,
    },
    Shutdown {
        done: oneshot::Sender<()>,
    },
}

pub struct InProcessClientHandle {
    command_tx: mpsc::UnboundedSender<ServerMessage>,
    event_rx: mpsc::UnboundedReceiver<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
}

#[derive(Clone)]
pub struct InProcessClientSender {
    command_tx: mpsc::UnboundedSender<ServerMessage>,
}

impl InProcessClientSender {
    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.send_command_with_context(command, None)
    }

    pub fn send_command_with_context(
        &self,
        command: AppClientCommand,
        context: Option<CommandExecutionContext>,
    ) -> Result<()> {
        self.command_tx
            .send(ServerMessage::Command { command, context })
            .map_err(|_| anyhow!("in-process app server is closed"))
    }
}

impl InProcessClientHandle {
    pub fn sender(&self) -> InProcessClientSender {
        InProcessClientSender {
            command_tx: self.command_tx.clone(),
        }
    }

    pub async fn next_message(&mut self) -> Option<AppServerMessage> {
        self.event_rx.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        let (done_tx, done_rx) = oneshot::channel();
        self.command_tx
            .send(ServerMessage::Shutdown { done: done_tx })
            .map_err(|_| anyhow!("in-process app server is closed"))?;
        let _ = done_rx.await;
        Ok(())
    }

    pub(crate) fn state(&self) -> Arc<Mutex<ServerState>> {
        self.state.clone()
    }
}

pub struct InProcessServer;

pub fn start_in_process(
    runtime: Arc<AgentHost>,
    conversation_id: Option<String>,
    emit_all_conversations: bool,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> InProcessClientHandle {
    start_in_process_with_runtime_manager(
        Arc::new(FixedRuntimeManager::new(runtime)),
        None,
        conversation_id,
        emit_all_conversations,
        auto_approve,
        auto_approve_reason,
    )
}

pub fn start_in_process_with_runtime_manager(
    runtime_manager: Arc<dyn AppRuntimeManager>,
    session_context: Option<SessionBootstrapContext>,
    conversation_id: Option<String>,
    emit_all_conversations: bool,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> InProcessClientHandle {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AppServerMessage>();
    let initial_conversation_id = conversation_id.unwrap_or_else(|| "default".to_string());
    let state = Arc::new(Mutex::new(ServerState::new(
        initial_conversation_id.clone(),
        emit_all_conversations,
    )));

    let state_for_task = state.clone();
    let initial_runtime = runtime_manager
        .runtime_for_session(session_context.as_ref())
        .or_else(|_| runtime_manager.initial_runtime())
        .expect("failed to resolve initial app runtime");
    spawn_skill_watch(initial_runtime.clone(), event_tx.clone(), state.clone());
    tokio::spawn(async move {
        session_state::hydrate_active_conversation(&initial_runtime, &state_for_task).await;
        while let Some(message) = command_rx.recv().await {
            match message {
                ServerMessage::Command {
                    command: AppClientCommand::Exit,
                    ..
                } => {
                    let tasks = {
                        let mut guard = state_for_task.lock().await;
                        guard.take_all_turn_tasks()
                    };
                    for task in tasks {
                        let _ = task.await;
                    }
                    break;
                }
                ServerMessage::Command { command, context } => {
                    let runtime = runtime_manager
                        .runtime_for_command(context.as_ref())
                        .or_else(|_| runtime_manager.runtime_for_session(session_context.as_ref()))
                        .or_else(|_| runtime_manager.initial_runtime())
                        .unwrap_or_else(|_| initial_runtime.clone());
                    let command_conversation_id = command.conversation_id().map(str::to_string);
                    let should_mark_active = matches!(command, AppClientCommand::SubmitTurn(_));
                    let error_conversation_id = command_conversation_id
                        .clone()
                        .unwrap_or_else(|| initial_conversation_id.clone());
                    if handle_command(
                        runtime.clone(),
                        command,
                        &event_tx,
                        state_for_task.clone(),
                        auto_approve,
                        auto_approve_reason.clone(),
                    )
                    .await
                    .is_err_and(|error| {
                        let _ = event_tx.send(AppServerMessage::Notification(
                            AppServerNotification::Error {
                                conversation_id: error_conversation_id.clone(),
                                message: format!("command handling failed: {error:#}"),
                            },
                        ));
                        true
                    }) {
                    } else if let (Some(id), true) = (command_conversation_id, should_mark_active) {
                        session_state::persist_active_conversation(&runtime, &state_for_task, &id)
                            .await;
                    }
                }
                ServerMessage::Shutdown { done } => {
                    let tasks = {
                        let mut guard = state_for_task.lock().await;
                        guard.take_all_turn_tasks()
                    };
                    for task in tasks {
                        let _ = task.await;
                    }
                    let _ = done.send(());
                    break;
                }
            }
        }
    });

    InProcessClientHandle {
        command_tx,
        event_rx,
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::start_in_process_with_runtime_manager;
    use crate::app::runtime_manager::AppRuntimeManager;
    use agent_core::{AgentHost, AgentHostExt};
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, CommandExecutionContext,
        SessionBootstrapContext,
    };
    use anyhow::{Result, anyhow};
    use cli::agent_host::build_agent_host;
    use config::AgentConfig;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRuntimeManager {
        default_runtime: Arc<AgentHost>,
        by_workspace: HashMap<String, Arc<AgentHost>>,
    }

    impl TestRuntimeManager {
        fn new(
            default_runtime: Arc<AgentHost>,
            by_workspace: HashMap<String, Arc<AgentHost>>,
        ) -> Self {
            Self {
                default_runtime,
                by_workspace,
            }
        }

        fn runtime_by_workspace(&self, workspace_root: Option<&str>) -> Result<Arc<AgentHost>> {
            let Some(workspace_root) = workspace_root else {
                return Ok(self.default_runtime.clone());
            };
            self.by_workspace
                .get(workspace_root)
                .cloned()
                .ok_or_else(|| anyhow!("missing test runtime for workspace `{workspace_root}`"))
        }
    }

    impl AppRuntimeManager for TestRuntimeManager {
        fn initial_runtime(&self) -> Result<Arc<AgentHost>> {
            Ok(self.default_runtime.clone())
        }

        fn runtime_for_session(
            &self,
            session_context: Option<&SessionBootstrapContext>,
        ) -> Result<Arc<AgentHost>> {
            self.runtime_by_workspace(session_context.and_then(|ctx| ctx.workspace_root.as_deref()))
        }

        fn runtime_for_command(
            &self,
            command_context: Option<&CommandExecutionContext>,
        ) -> Result<Arc<AgentHost>> {
            self.runtime_by_workspace(command_context.and_then(|ctx| ctx.workspace_root.as_deref()))
        }
    }

    #[tokio::test]
    async fn session_context_selects_initial_runtime_for_active_conversation() -> Result<()> {
        let runtime_a = test_runtime("workspace-a")?;
        let runtime_b = test_runtime("workspace-b")?;
        runtime_a.mark_active_conversation("conversation-a").await?;
        runtime_b.mark_active_conversation("conversation-b").await?;

        let manager = Arc::new(TestRuntimeManager::new(
            runtime_a.clone(),
            HashMap::from([
                (workspace_string(&runtime_a), runtime_a.clone()),
                (workspace_string(&runtime_b), runtime_b.clone()),
            ]),
        ));

        let handle = start_in_process_with_runtime_manager(
            manager,
            Some(SessionBootstrapContext {
                session_id: Some("session-1".to_string()),
                source_domain: Some("local:cli".to_string()),
                workspace_root: Some(workspace_string(&runtime_b)),
                cwd: None,
                permission_mode: None,
                data_root_dir: None,
            }),
            None,
            false,
            true,
            Some("test".to_string()),
        );

        tokio::task::yield_now().await;
        let state = handle.state();
        let guard = state.lock().await;
        assert_eq!(guard.active_conversation_id(), "conversation-b");
        Ok(())
    }

    #[tokio::test]
    async fn command_context_switches_runtime_for_list_conversations() -> Result<()> {
        let runtime_a = test_runtime("workspace-cmd-a")?;
        let runtime_b = test_runtime("workspace-cmd-b")?;
        runtime_a.create_conversation("conversation-a").await?;
        runtime_b.create_conversation("conversation-b").await?;

        let workspace_a = workspace_string(&runtime_a);
        let workspace_b = workspace_string(&runtime_b);
        let manager = Arc::new(TestRuntimeManager::new(
            runtime_a.clone(),
            HashMap::from([
                (workspace_a.clone(), runtime_a.clone()),
                (workspace_b.clone(), runtime_b.clone()),
            ]),
        ));

        let mut handle = start_in_process_with_runtime_manager(
            manager,
            Some(SessionBootstrapContext {
                session_id: Some("session-2".to_string()),
                source_domain: Some("local:cli".to_string()),
                workspace_root: Some(workspace_a),
                cwd: None,
                permission_mode: None,
                data_root_dir: None,
            }),
            Some("default".to_string()),
            false,
            true,
            Some("test".to_string()),
        );

        handle.sender().send_command_with_context(
            AppClientCommand::ListConversations,
            Some(CommandExecutionContext {
                session_id: Some("session-2".to_string()),
                workspace_id: None,
                workspace_root: Some(workspace_b),
                cwd: None,
                permission_mode: None,
                data_root_dir: None,
            }),
        )?;

        let Some(message) = handle.next_message().await else {
            return Err(anyhow!("expected conversation list notification"));
        };

        match message {
            AppServerMessage::Notification(AppServerNotification::ConversationList {
                conversations,
                ..
            }) => {
                assert_eq!(conversations.len(), 1);
                assert_eq!(conversations[0].conversation_id, "conversation-b");
            }
            other => return Err(anyhow!("unexpected app server message: {other:?}")),
        }

        handle.shutdown().await?;
        Ok(())
    }

    fn test_runtime(label: &str) -> Result<Arc<AgentHost>> {
        let root = unique_temp_workspace(label);
        std::fs::create_dir_all(root.join("configs"))?;
        std::fs::create_dir_all(root.join("data").join("conversations"))?;
        std::fs::create_dir_all(root.join("data").join("state").join("memory"))?;
        let mut config = AgentConfig::load(root)?;
        config.llm.api_key = "test-key".to_string();
        config.llm.base_url = "https://example.invalid/v1".to_string();
        config.llm.model = "test-model".to_string();
        build_agent_host(config)
    }

    fn workspace_string(runtime: &Arc<AgentHost>) -> String {
        runtime
            .context()
            .workspace_root
            .to_string_lossy()
            .into_owned()
    }

    fn unique_temp_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cloudagent-app-server-{label}-{unique}"))
    }
}
