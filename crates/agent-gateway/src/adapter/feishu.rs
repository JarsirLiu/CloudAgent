use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::config::{FeishuConfig, GatewayConfig, LlmConfig};
use crate::gateway_outbound::{
    GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate, OutboundTarget,
};
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::platforms::feishu::{FeishuAdapter, FeishuAdapterOptions};
use crate::session::build_session_key;
use agent_app_server_client::AppServerClient;
use agent_core::{ServerRequestDecision, text_input_items};
use agent_protocol::{
    AppServerMessage, AppServerNotification, AppServerRequest, TurnPolicy, UserTurnInput,
};
use anyhow::Result;
use async_trait::async_trait;
use feishu_sdk::card::CardAction;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};
use tracing::info;

#[derive(Debug, Clone, Default)]
pub struct FeishuAdapterConfig {
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
    pub verification_token: Option<String>,
    pub encrypt_key: Option<String>,
    pub enable_cards: bool,
    pub thread_isolation: bool,
    pub reply_to_trigger: bool,
    pub group_only_mentioned: bool,
}

impl FeishuAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.app_id.trim().is_empty() {
            anyhow::bail!("missing app_id")
        }
        if self.app_secret.trim().is_empty() {
            anyhow::bail!("missing app_secret")
        }
        Ok(())
    }
}

pub struct PlatformRuntime {
    task: JoinHandle<Result<()>>,
}

impl PlatformRuntime {
    pub async fn wait(self) -> Result<()> {
        self.task.await?
    }
}

pub fn spawn_runtime(
    config: FeishuAdapterConfig,
    stream_client: AppServerClient,
    control_client: AppServerClient,
    turn_policy: TurnPolicy,
) -> Result<PlatformRuntime> {
    config.validate()?;
    let enable_cards = config.enable_cards;
    let gateway_config = GatewayConfig {
        log_filter: "info".to_string(),
        feishu: FeishuConfig {
            app_id: config.app_id,
            app_secret: config.app_secret,
            verification_token: config.verification_token,
            encrypt_key: config.encrypt_key,
            base_url: if config.domain.trim().is_empty() {
                "https://open.feishu.cn".to_string()
            } else {
                config.domain
            },
            group_only_mentioned: config.group_only_mentioned,
        },
        llm: LlmConfig {
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            temperature: 0.0,
            system_prompt: String::new(),
        },
    };
    let approvals = Arc::new(ApprovalCoordinator::new(control_client));
    let card_approvals = approvals.clone();
    let adapter = Arc::new(FeishuAdapter::new(
        gateway_config,
        FeishuAdapterOptions {
            enable_cards,
            on_card_action: if enable_cards {
                Some(Arc::new(move |action: CardAction| {
                    let approvals = card_approvals.clone();
                    Box::pin(async move { handle_card_action(action, approvals).await })
                }))
            } else {
                None
            },
        },
    )?);
    let platform_adapter: Arc<dyn PlatformAdapter> = adapter.clone();
    let handler = Arc::new(NodeBackedHandler {
        adapter: adapter.clone(),
        stream_client: Mutex::new(stream_client),
        approvals,
        enable_cards,
        turn_policy,
    });
    let task = tokio::spawn(async move { platform_adapter.run(handler).await });
    Ok(PlatformRuntime { task })
}

struct NodeBackedHandler {
    adapter: Arc<FeishuAdapter>,
    stream_client: Mutex<AppServerClient>,
    approvals: Arc<ApprovalCoordinator>,
    enable_cards: bool,
    turn_policy: TurnPolicy,
}

#[derive(Clone)]
struct PendingApprovalRequest {
    request_id: agent_protocol::RequestId,
    request: agent_core::ServerRequest,
    approval_id: Option<String>,
}

#[derive(Default)]
struct ApprovalState {
    next_approval_id: u64,
    pending_by_session: HashMap<String, PendingApprovalRequest>,
    session_by_approval_id: HashMap<String, String>,
}

struct ApprovalCoordinator {
    control_client: Mutex<AppServerClient>,
    state: Mutex<ApprovalState>,
}

impl ApprovalCoordinator {
    fn new(control_client: AppServerClient) -> Self {
        Self {
            control_client: Mutex::new(control_client),
            state: Mutex::new(ApprovalState::default()),
        }
    }

    async fn register_pending(
        &self,
        session_key: &str,
        request: &AppServerRequest,
        enable_cards: bool,
    ) -> PendingApprovalRequest {
        let AppServerRequest::ServerRequest {
            request_id,
            request,
            ..
        } = request;
        let mut state = self.state.lock().await;
        if let Some(previous) = state.pending_by_session.remove(session_key)
            && let Some(previous_id) = previous.approval_id
        {
            state.session_by_approval_id.remove(&previous_id);
        }

        let approval_id = if enable_cards {
            state.next_approval_id += 1;
            let approval_id = state.next_approval_id.to_string();
            state
                .session_by_approval_id
                .insert(approval_id.clone(), session_key.to_string());
            Some(approval_id)
        } else {
            None
        };

        let pending = PendingApprovalRequest {
            request_id: request_id.clone(),
            request: request.clone(),
            approval_id,
        };
        state
            .pending_by_session
            .insert(session_key.to_string(), pending.clone());
        pending
    }

    async fn has_pending(&self, session_key: &str) -> bool {
        let state = self.state.lock().await;
        state.pending_by_session.contains_key(session_key)
    }

    async fn resolve_by_session(
        &self,
        session_key: &str,
        decision: ServerRequestDecision,
    ) -> Result<Option<PendingApprovalRequest>> {
        self.resolve_inner(Some(session_key), None, decision).await
    }

    async fn resolve_by_approval_id(
        &self,
        approval_id: &str,
        decision: ServerRequestDecision,
    ) -> Result<Option<PendingApprovalRequest>> {
        self.resolve_inner(None, Some(approval_id), decision).await
    }

    async fn resolve_inner(
        &self,
        session_key: Option<&str>,
        approval_id: Option<&str>,
        decision: ServerRequestDecision,
    ) -> Result<Option<PendingApprovalRequest>> {
        let (resolved_session_key, pending) = {
            let mut state = self.state.lock().await;
            let resolved_session_key = if let Some(session_key) = session_key {
                session_key.to_string()
            } else if let Some(approval_id) = approval_id {
                match state.session_by_approval_id.get(approval_id) {
                    Some(session_key) => session_key.clone(),
                    None => return Ok(None),
                }
            } else {
                return Ok(None);
            };

            let pending = match state.pending_by_session.remove(&resolved_session_key) {
                Some(pending) => pending,
                None => return Ok(None),
            };
            if let Some(approval_id) = pending.approval_id.as_ref() {
                state.session_by_approval_id.remove(approval_id);
            }
            (resolved_session_key, pending)
        };

        let control_client = self.control_client.lock().await;
        control_client.resolve_server_request(
            resolved_session_key,
            pending.request_id.clone(),
            decision,
        )?;
        Ok(Some(pending))
    }
}

#[async_trait]
impl MessageHandler for NodeBackedHandler {
    async fn try_handle_session_command(&self, message: &InboundMessage) -> Result<bool> {
        if self.enable_cards {
            return Ok(false);
        }
        let Some(decision) = parse_approval_command(&message.text) else {
            return Ok(false);
        };
        let session_key = build_session_key(message);
        let pending = self
            .approvals
            .resolve_by_session(&session_key, decision.clone())
            .await?;
        let Some(pending) = pending else {
            return Ok(false);
        };

        let label = decision.label();
        info!(
            session_key = %session_key,
            request_id = ?pending.request_id,
            decision = label,
            "gateway.platform_runtime.server_request.resolving"
        );

        self.adapter
            .send_outbound(GatewayOutbound::Info {
                target: OutboundTarget {
                    conversation_id: session_key,
                    chat_id: message.chat_id.clone(),
                    reply_context: message.reply_context.clone(),
                },
                message: format!(
                    "审批已处理: {}",
                    render_request_resolution_label(&pending.request)
                ),
            })
            .await?;
        Ok(true)
    }

    async fn handle_message(&self, message: InboundMessage) -> Result<()> {
        let session_key = build_session_key(&message);
        let target = OutboundTarget {
            conversation_id: session_key.clone(),
            chat_id: message.chat_id.clone(),
            reply_context: message.reply_context.clone(),
        };
        info!(
            platform = %message.platform,
            session_key = %session_key,
            message_id = %message.message_id,
            "gateway.platform_runtime.inbound.accepted"
        );

        self.adapter
            .send_outbound(GatewayOutbound::Progress(GatewayProgressUpdate {
                target: target.clone(),
                kind: GatewayProgressKind::Reasoning,
                summary: "模型开始处理当前消息".to_string(),
                streaming: true,
            }))
            .await?;

        let mut stream_client = self.stream_client.lock().await;
        stream_client.send_command(agent_protocol::AppClientCommand::SubscribeConversation {
            conversation_id: session_key.clone(),
        })?;
        info!(
            session_key = %session_key,
            "gateway.platform_runtime.turn.submit.start"
        );
        stream_client.submit_turn(UserTurnInput {
            conversation_id: session_key.clone(),
            content: text_input_items(message.text.clone()),
            turn_policy: self.turn_policy.clone(),
        })?;
        info!(
            session_key = %session_key,
            "gateway.platform_runtime.turn.submit.ok"
        );

        let mut active_turn_id: Option<String> = None;
        loop {
            let wait_duration = if self.approvals.has_pending(&session_key).await {
                Duration::from_secs(600)
            } else {
                Duration::from_secs(30)
            };
            let maybe_event = timeout(wait_duration, stream_client.next_event()).await;
            let event = match maybe_event {
                Ok(Some(event)) => event,
                Ok(None) => {
                    info!(
                        session_key = %session_key,
                        "gateway.platform_runtime.event.stream_closed"
                    );
                    break;
                }
                Err(_) => {
                    info!(
                        session_key = %session_key,
                        "gateway.platform_runtime.event.timeout"
                    );
                    if self.approvals.has_pending(&session_key).await {
                        self.adapter
                            .send_outbound(GatewayOutbound::Info {
                                target: target.clone(),
                                message: "Agent 正在等待飞书审批卡片上的操作。".to_string(),
                            })
                            .await?;
                    } else {
                        self.adapter
                            .send_outbound(GatewayOutbound::Info {
                                target: target.clone(),
                                message: "消息已提交给 Agent，但后续事件返回超时。".to_string(),
                            })
                            .await?;
                    }
                    break;
                }
            };
            if event_conversation_id(&event) != Some(session_key.as_str()) {
                info!(
                    session_key = %session_key,
                    event_conversation_id = ?event_conversation_id(&event),
                    "gateway.platform_runtime.event.skipped_foreign_conversation"
                );
                continue;
            }
            let event_turn_id = event_turn_id(&event);
            if let Some(bound_turn_id) = active_turn_id.as_deref() {
                if let Some(event_turn_id) = event_turn_id
                    && event_turn_id != bound_turn_id
                {
                    info!(
                        session_key = %session_key,
                        active_turn_id = %bound_turn_id,
                        event_turn_id = %event_turn_id,
                        "gateway.platform_runtime.event.skipped_foreign_turn"
                    );
                    continue;
                }
            } else if let Some(event_turn_id) = event_turn_id {
                if matches!(
                    &event,
                    agent_app_server_client::AppServerEvent::Message(
                        AppServerMessage::Notification(AppServerNotification::TurnStarted { .. })
                    )
                ) {
                    active_turn_id = Some(event_turn_id.to_string());
                    info!(
                        session_key = %session_key,
                        turn_id = %event_turn_id,
                        "gateway.platform_runtime.turn.bound"
                    );
                } else {
                    info!(
                        session_key = %session_key,
                        event_turn_id = %event_turn_id,
                        "gateway.platform_runtime.event.skipped_until_turn_start"
                    );
                    continue;
                }
            }
            if let Some(request) = event_request(&event) {
                let pending = self
                    .approvals
                    .register_pending(&session_key, request, self.enable_cards)
                    .await;
                if self.enable_cards {
                    if let Some(approval_id) = pending.approval_id.as_deref() {
                        match self
                            .adapter
                            .send_approval_card(
                                target.chat_id.clone(),
                                target.reply_context.clone(),
                                build_approval_card(request, approval_id),
                            )
                            .await
                        {
                            Ok(()) => {
                                info!(
                                    session_key = %session_key,
                                    approval_id = %approval_id,
                                    request_kind = %approval_title(&pending.request),
                                    "gateway.platform_runtime.server_request.card_sent"
                                );
                            }
                            Err(error) => {
                                info!(
                                    session_key = %session_key,
                                    approval_id = %approval_id,
                                    ?error,
                                    "gateway.platform_runtime.server_request.card_failed"
                                );
                                self.adapter
                                    .send_outbound(GatewayOutbound::Error {
                                        target: target.clone(),
                                        message: "审批卡片发送失败，当前会话无法继续审批。请检查飞书卡片回调配置。".to_string(),
                                    })
                                    .await?;
                            }
                        }
                    }
                } else {
                    self.adapter
                        .send_outbound(GatewayOutbound::Info {
                            target: target.clone(),
                            message: "当前飞书平台未启用审批卡片，无法在 IM 内继续这次审批。"
                                .to_string(),
                        })
                        .await?;
                }
                continue;
            }
            match map_app_server_event(&target, event) {
                EventFlow::Continue(outbounds) => {
                    for outbound in outbounds {
                        self.adapter.send_outbound(outbound).await?;
                    }
                }
                EventFlow::Completed(outbounds) => {
                    for outbound in outbounds {
                        self.adapter.send_outbound(outbound).await?;
                    }
                    break;
                }
            }
        }

        Ok(())
    }
}

fn event_conversation_id(event: &agent_app_server_client::AppServerEvent) -> Option<&str> {
    match event {
        agent_app_server_client::AppServerEvent::Message(message) => message.conversation_id(),
        agent_app_server_client::AppServerEvent::Lagged { .. }
        | agent_app_server_client::AppServerEvent::Disconnected { .. } => None,
    }
}

fn event_request(event: &agent_app_server_client::AppServerEvent) -> Option<&AppServerRequest> {
    match event {
        agent_app_server_client::AppServerEvent::Message(AppServerMessage::Request(request)) => {
            Some(request)
        }
        _ => None,
    }
}

fn event_turn_id(event: &agent_app_server_client::AppServerEvent) -> Option<&str> {
    match event {
        agent_app_server_client::AppServerEvent::Message(AppServerMessage::Notification(
            notification,
        )) => notification_turn_id(notification),
        agent_app_server_client::AppServerEvent::Message(AppServerMessage::Request(request)) => {
            request_turn_id(request)
        }
        agent_app_server_client::AppServerEvent::Lagged { .. }
        | agent_app_server_client::AppServerEvent::Disconnected { .. } => None,
    }
}

fn notification_turn_id(notification: &AppServerNotification) -> Option<&str> {
    match notification {
        AppServerNotification::TurnStarted { turn_id, .. }
        | AppServerNotification::ItemStarted { turn_id, .. }
        | AppServerNotification::AgentMessageDelta { turn_id, .. }
        | AppServerNotification::PlanDelta { turn_id, .. }
        | AppServerNotification::ReasoningSummaryTextDelta { turn_id, .. }
        | AppServerNotification::ReasoningTextDelta { turn_id, .. }
        | AppServerNotification::CommandExecutionOutputDelta { turn_id, .. }
        | AppServerNotification::ToolOutputDelta { turn_id, .. }
        | AppServerNotification::FileChangeOutputDelta { turn_id, .. }
        | AppServerNotification::TokenUsageUpdated { turn_id, .. }
        | AppServerNotification::ModelRetrying { turn_id, .. }
        | AppServerNotification::ItemCompleted { turn_id, .. }
        | AppServerNotification::TurnCompleted { turn_id, .. }
        | AppServerNotification::TurnFailed { turn_id, .. }
        | AppServerNotification::TurnCancelled { turn_id, .. } => Some(turn_id.as_str()),
        _ => None,
    }
}

fn request_turn_id(request: &AppServerRequest) -> Option<&str> {
    match request {
        AppServerRequest::ServerRequest { request, .. } => match request {
            agent_core::ServerRequest::CommandApproval { request } => {
                Some(request.turn_id.as_str())
            }
            agent_core::ServerRequest::FileChangeApproval { request } => {
                Some(request.turn_id.as_str())
            }
        },
    }
}

fn approval_title(request: &agent_core::ServerRequest) -> &'static str {
    match request {
        agent_core::ServerRequest::CommandApproval { .. } => "command approval",
        agent_core::ServerRequest::FileChangeApproval { .. } => "file change approval",
    }
}

fn render_request_prompt(request: &AppServerRequest) -> String {
    let AppServerRequest::ServerRequest { request, .. } = request;
    match request {
        agent_core::ServerRequest::CommandApproval { request } => format!(
            "工具调用需要审批: {}\n原因: {}\n命令: {}",
            approval_title(&agent_core::ServerRequest::CommandApproval {
                request: request.clone()
            }),
            request.reason,
            request.command_preview
        ),
        agent_core::ServerRequest::FileChangeApproval { request } => format!(
            "工具调用需要审批: {}\n原因: {}\n变更: {}",
            approval_title(&agent_core::ServerRequest::FileChangeApproval {
                request: request.clone()
            }),
            request.reason,
            request.change_preview
        ),
    }
}

fn render_request_resolution_label(request: &agent_core::ServerRequest) -> &'static str {
    approval_title(request)
}

fn parse_approval_command(text: &str) -> Option<ServerRequestDecision> {
    let normalized = text.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "/approve" | "/allow" | "/yes" => Some(ServerRequestDecision::accept(Some(
            "approved from feishu im".to_string(),
        ))),
        "/approve-session" | "/approve session" | "/always" | "/session" => {
            Some(ServerRequestDecision::accept_for_session(Some(
                "approved for session from feishu im".to_string(),
            )))
        }
        "/deny" | "/reject" | "/no" => Some(ServerRequestDecision::decline(Some(
            "denied from feishu im".to_string(),
        ))),
        "/cancel" => Some(ServerRequestDecision::cancel(Some(
            "cancelled from feishu im".to_string(),
        ))),
        _ => None,
    }
}

async fn handle_card_action(action: CardAction, approvals: Arc<ApprovalCoordinator>) -> Result<()> {
    let Some(value) = action
        .action
        .as_ref()
        .and_then(|action| action.value.as_ref())
        .and_then(|value| value.as_object())
    else {
        return Ok(());
    };

    let Some(action_name) = value
        .get("cloudagent_action")
        .and_then(|value| value.as_str())
    else {
        return Ok(());
    };
    if action_name != "approval" {
        return Ok(());
    }

    let Some(approval_id) = value.get("approval_id").and_then(|value| value.as_str()) else {
        return Ok(());
    };
    let Some(decision_name) = value.get("decision").and_then(|value| value.as_str()) else {
        return Ok(());
    };
    let Some(decision) = parse_approval_decision_name(decision_name) else {
        return Ok(());
    };

    let resolved = approvals
        .resolve_by_approval_id(approval_id, decision.clone())
        .await?;
    if let Some(pending) = resolved {
        info!(
            approval_id = %approval_id,
            request_id = ?pending.request_id,
            decision = decision.label(),
            actor_open_id = ?action.open_id,
            "gateway.platform_runtime.server_request.card_resolved"
        );
    } else {
        info!(
            approval_id = %approval_id,
            actor_open_id = ?action.open_id,
            "gateway.platform_runtime.server_request.card_already_resolved"
        );
    }
    Ok(())
}

fn parse_approval_decision_name(value: &str) -> Option<ServerRequestDecision> {
    match value {
        "approve_once" => Some(ServerRequestDecision::accept(Some(
            "approved from feishu card".to_string(),
        ))),
        "approve_session" => Some(ServerRequestDecision::accept_for_session(Some(
            "approved for session from feishu card".to_string(),
        ))),
        "deny" => Some(ServerRequestDecision::decline(Some(
            "denied from feishu card".to_string(),
        ))),
        "cancel" => Some(ServerRequestDecision::cancel(Some(
            "cancelled from feishu card".to_string(),
        ))),
        _ => None,
    }
}

fn build_approval_card(request: &AppServerRequest, approval_id: &str) -> serde_json::Value {
    let prompt = render_request_prompt(request);
    serde_json::json!({
        "config": {
            "wide_screen_mode": true
        },
        "header": {
            "template": "orange",
            "title": {
                "tag": "plain_text",
                "content": "工具调用需要审批"
            }
        },
        "elements": [
            {
                "tag": "markdown",
                "content": prompt
            },
            {
                "tag": "markdown",
                "content": "请直接点击下方按钮继续当前会话。"
            },
            {
                "tag": "action",
                "actions": [
                    approval_button("批准一次", approval_id, "approve_once", "primary"),
                    approval_button("本会话允许", approval_id, "approve_session", "default"),
                    approval_button("拒绝", approval_id, "deny", "danger"),
                    approval_button("取消", approval_id, "cancel", "default")
                ]
            }
        ]
    })
}

fn approval_button(
    label: &str,
    approval_id: &str,
    decision: &str,
    button_type: &str,
) -> serde_json::Value {
    serde_json::json!({
        "tag": "button",
        "type": button_type,
        "text": {
            "tag": "plain_text",
            "content": label
        },
        "value": {
            "cloudagent_action": "approval",
            "approval_id": approval_id,
            "decision": decision
        }
    })
}
