mod app;
mod projection;
mod routing;
mod server_request;
mod session;
pub mod transport;
mod turn;

use agent_core::AgentHost;
use agent_protocol::{
    AppClientCommandEnvelope, AppServerMessageEnvelope, CommandExecutionContext, JsonRpcError,
    JsonRpcErrorPayload, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, SessionBootstrapContext,
};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

pub use app::in_process::{
    InProcessClientHandle, InProcessClientSender, InProcessServer, start_in_process,
    start_in_process_with_runtime_manager,
};
pub use app::runtime_manager::{AppRuntimeManager, FixedRuntimeManager};

pub async fn run_stdio_server(
    runtime: Arc<AgentHost>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    run_stdio_server_with_runtime_manager(
        Arc::new(FixedRuntimeManager::new(runtime)),
        None,
        auto_approve,
        auto_approve_reason,
    )
    .await
}

pub async fn run_stdio_server_with_runtime_manager(
    runtime_manager: Arc<dyn AppRuntimeManager>,
    session_context: Option<SessionBootstrapContext>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    let mut client = start_in_process_with_runtime_manager(
        runtime_manager.clone(),
        session_context,
        None,
        true,
        auto_approve,
        auto_approve_reason,
    );
    let sender = client.sender();
    let state = client.state();
    let view = client.view();
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<JsonRpcMessage>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<JsonRpcMessage>();
    let event_tx_for_events = event_tx.clone();

    let read_task = tokio::spawn(async move { transport::stdio::read_messages(command_tx).await });
    let write_task = tokio::spawn(async move { transport::stdio::write_messages(event_rx).await });
    let forward_events = tokio::spawn(async move {
        let mut seq_by_conversation: HashMap<String, u64> = HashMap::new();
        while let Some(message) = client.next_message().await {
            let event_seq = message.conversation_id().map(|conversation_id| {
                let next = seq_by_conversation
                    .entry(conversation_id.to_string())
                    .or_insert(0);
                *next += 1;
                *next
            });
            if event_tx_for_events
                .send(JsonRpcMessage::from(AppServerMessageEnvelope {
                    message,
                    event_seq,
                }))
                .is_err()
            {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let forward_commands = tokio::spawn(async move {
        while let Some(message) = command_rx.recv().await {
            match message {
                JsonRpcMessage::Request(request) => {
                    handle_stdio_request(
                        runtime_manager.as_ref(),
                        &sender,
                        &event_tx,
                        &state,
                        &view,
                        request,
                    )
                    .await?;
                }
                JsonRpcMessage::Notification(notification) => {
                    let envelope = AppClientCommandEnvelope::try_from(
                        JsonRpcMessage::Notification(notification),
                    )?;
                    sender.send_command_with_context(envelope.command, envelope.context)?;
                }
                JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {}
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    read_task.await??;
    forward_commands.await??;
    forward_events.await??;
    write_task.await??;
    Ok(())
}

async fn handle_stdio_request(
    runtime_manager: &dyn AppRuntimeManager,
    sender: &app::in_process::InProcessClientSender,
    event_tx: &mpsc::UnboundedSender<JsonRpcMessage>,
    state: &Arc<tokio::sync::Mutex<routing::command_router::ServerState>>,
    view: &session::conversation_watch::ConversationWatchManager,
    request: JsonRpcRequest,
) -> Result<()> {
    let runtime = runtime_for_request(runtime_manager, &request)?;
    let request_id = request.id.clone();
    let response = match request.method.as_str() {
        "conversation/list" => {
            let result = session::service::read_conversation_list(&runtime, state).await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/listPage" => {
            let params = request.params.clone().unwrap_or(serde_json::Value::Null);
            let cursor = optional_value_field::<String>(&params, "cursor")?;
            let limit = optional_value_field::<usize>(&params, "limit")?.unwrap_or(25);
            let result =
                session::service::read_conversation_list_page(&runtime, state, cursor, limit)
                    .await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "skills/list" => {
            let result = session::service::read_skills_list(&runtime, state).await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/view" => {
            let conversation_id = required_string_param(&request, "conversation_id")?;
            let result =
                session::service::read_conversation_view(&runtime, view, conversation_id).await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/history" => {
            let conversation_id = required_string_param(&request, "conversation_id")?;
            let result =
                session::service::read_conversation_history(&runtime, state, conversation_id)
                    .await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/historyPage" => {
            let params = request.params.clone().unwrap_or(serde_json::Value::Null);
            let conversation_id = value_field::<String>(&params, "conversation_id")?;
            let before_turn_id = optional_value_field::<String>(&params, "before_turn_id")?;
            let limit = optional_value_field::<usize>(&params, "limit")?.unwrap_or(30);
            let result = session::service::read_conversation_history_page(
                &runtime,
                state,
                conversation_id,
                before_turn_id,
                limit,
            )
            .await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "hub/node/list" | "hub/node/select" => JsonRpcMessage::Error(JsonRpcError {
            id: request_id,
            error: JsonRpcErrorPayload {
                code: -32601,
                message: format!(
                    "{} is not available for the current direct target",
                    request.method
                ),
                data: None,
            },
        }),
        _ => match AppClientCommandEnvelope::try_from(JsonRpcMessage::Request(request)) {
            Ok(envelope) => {
                sender.send_command_with_context(envelope.command, envelope.context)?;
                return Ok(());
            }
            Err(error) => {
                let code = if error.to_string().contains("unsupported request method") {
                    -32601
                } else {
                    -32602
                };
                JsonRpcMessage::Error(JsonRpcError {
                    id: request_id,
                    error: JsonRpcErrorPayload {
                        code,
                        message: error.to_string(),
                        data: None,
                    },
                })
            }
        },
    };

    let _ = event_tx.send(response);
    Ok(())
}

fn runtime_for_request(
    runtime_manager: &dyn AppRuntimeManager,
    request: &JsonRpcRequest,
) -> Result<Arc<AgentHost>> {
    let context = request_command_context(request);
    runtime_manager
        .runtime_for_command(context.as_ref())
        .or_else(|_| runtime_manager.initial_runtime())
}

fn request_command_context(request: &JsonRpcRequest) -> Option<CommandExecutionContext> {
    AppClientCommandEnvelope::try_from(JsonRpcMessage::Request(request.clone()))
        .ok()
        .and_then(|envelope| envelope.context)
}

fn required_string_param(request: &JsonRpcRequest, field: &str) -> Result<String> {
    let params = request.params.clone().unwrap_or(serde_json::Value::Null);
    value_field::<String>(&params, field)
}

fn value_field<T>(params: &serde_json::Value, field: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let value = params
        .get(field)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing required field `{field}`"))?;
    Ok(serde_json::from_value(value)?)
}

fn optional_value_field<T>(params: &serde_json::Value, field: &str) -> Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = params.get(field).cloned() else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(value)?))
}

#[cfg(test)]
mod tests {
    use super::{request_command_context, runtime_for_request};
    use crate::AppRuntimeManager;
    use agent_core::{AgentHost, AgentHostExt};
    use agent_protocol::{
        AppClientCommand, AppClientCommandEnvelope, CommandExecutionContext, JsonRpcMessage,
        JsonRpcRequest, RequestId,
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

        fn runtime_for_command(
            &self,
            command_context: Option<&CommandExecutionContext>,
        ) -> Result<Arc<AgentHost>> {
            self.runtime_by_workspace(command_context.and_then(|ctx| ctx.workspace_root.as_deref()))
        }
    }

    #[test]
    fn extracts_command_context_from_typed_request() {
        let request = typed_request_with_context(RequestId::Integer(7), "D:/repo-b".to_string());

        let context = request_command_context(&request).expect("context");
        assert_eq!(context.workspace_root.as_deref(), Some("D:/repo-b"));
        assert_eq!(context.session_id.as_deref(), Some("session-9"));
    }

    #[tokio::test]
    async fn typed_request_runtime_selection_prefers_request_context() -> Result<()> {
        let runtime_a = test_runtime("request-runtime-a")?;
        let runtime_b = test_runtime("request-runtime-b")?;
        let workspace_a = workspace_string(&runtime_a);
        let workspace_b = workspace_string(&runtime_b);
        let manager = TestRuntimeManager {
            default_runtime: runtime_a.clone(),
            by_workspace: HashMap::from([
                (workspace_a.clone(), runtime_a.clone()),
                (workspace_b.clone(), runtime_b.clone()),
            ]),
        };

        let request = typed_request_with_context(RequestId::Integer(9), workspace_b.clone());
        let runtime = runtime_for_request(&manager, &request)?;

        assert_eq!(workspace_string(&runtime), workspace_b);
        Ok(())
    }

    fn typed_request_with_context(request_id: RequestId, workspace_root: String) -> JsonRpcRequest {
        let rpc = JsonRpcMessage::from(AppClientCommandEnvelope {
            request_id,
            command: AppClientCommand::ListConversations,
            context: Some(CommandExecutionContext {
                session_id: Some("session-9".to_string()),
                workspace_id: None,
                workspace_root: Some(workspace_root),
                cwd: Some("D:/repo-b/subdir".to_string()),
                permission_mode: Some("WorkspaceWrite".to_string()),
                data_root_dir: Some("D:/repo-b/data".to_string()),
            }),
        });
        let JsonRpcMessage::Request(request) = rpc else {
            panic!("expected request");
        };
        request
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
        std::env::temp_dir().join(format!("cloudagent-stdio-request-{label}-{unique}"))
    }
}
