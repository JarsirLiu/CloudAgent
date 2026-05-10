use crate::GatewayOutbound;
use crate::adapter::GatewayAdapter;
use crate::direct::{app_server_message_to_outbound, gateway_message_to_command};
use agent_app_server_client::{AppServerClient, AppServerEvent};
use agent_protocol::{AppServerMessage, TurnPolicy};
use anyhow::Result;

#[derive(Clone, Debug)]
pub enum DirectNodeEvent {
    Message(Box<AppServerMessage>),
    Lagged {
        conversation_id: Option<String>,
        skipped: usize,
    },
    Disconnected {
        conversation_id: Option<String>,
        message: String,
    },
}

impl From<AppServerEvent> for DirectNodeEvent {
    fn from(event: AppServerEvent) -> Self {
        match event {
            AppServerEvent::Message(message) => Self::Message(Box::new(message)),
            AppServerEvent::Lagged { skipped } => Self::Lagged {
                conversation_id: None,
                skipped,
            },
            AppServerEvent::Disconnected { message } => Self::Disconnected {
                conversation_id: None,
                message,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpStatus {
    Active,
    AdapterClosed,
    NodeClosed,
}

pub struct DirectGatewaySession<A> {
    adapter: A,
    node_client: Option<AppServerClient>,
    turn_policy: TurnPolicy,
    active_conversation_id: Option<String>,
}

impl<A> DirectGatewaySession<A> {
    pub fn new(adapter: A, node_client: AppServerClient, turn_policy: TurnPolicy) -> Self {
        Self {
            adapter,
            node_client: Some(node_client),
            turn_policy,
            active_conversation_id: None,
        }
    }

    pub fn adapter(&self) -> &A {
        &self.adapter
    }

    pub fn adapter_mut(&mut self) -> &mut A {
        &mut self.adapter
    }

    pub fn node_client(&self) -> &AppServerClient {
        self.node_client
            .as_ref()
            .expect("direct gateway session node client is not configured")
    }

    pub fn node_client_mut(&mut self) -> &mut AppServerClient {
        self.node_client
            .as_mut()
            .expect("direct gateway session node client is not configured")
    }

    async fn next_node_event(&mut self) -> Result<Option<DirectNodeEvent>> {
        Ok(self
            .node_client_mut()
            .next_event()
            .await
            .map(DirectNodeEvent::from))
    }
}

impl<A> DirectGatewaySession<A>
where
    A: GatewayAdapter,
{
    async fn handle_node_event(&mut self, event: DirectNodeEvent) -> Result<PumpStatus> {
        match event {
            DirectNodeEvent::Message(message) => {
                if let Some(conversation_id) = message.conversation_id() {
                    self.active_conversation_id = Some(conversation_id.to_string());
                }
                if let Some(outbound) = app_server_message_to_outbound(&message) {
                    self.adapter.send_outbound(outbound).await?;
                }
                Ok(PumpStatus::Active)
            }
            DirectNodeEvent::Lagged {
                conversation_id,
                skipped,
            } => {
                self.adapter
                    .send_outbound(GatewayOutbound::Info {
                        conversation_id: conversation_id
                            .or_else(|| self.active_conversation_id.clone())
                            .unwrap_or_default(),
                        message: format!("node event stream lagged by {skipped} events"),
                    })
                    .await?;
                Ok(PumpStatus::Active)
            }
            DirectNodeEvent::Disconnected {
                conversation_id,
                message,
            } => {
                self.adapter
                    .send_outbound(GatewayOutbound::Error {
                        conversation_id: conversation_id
                            .or_else(|| self.active_conversation_id.clone())
                            .unwrap_or_default(),
                        message,
                    })
                    .await?;
                Ok(PumpStatus::NodeClosed)
            }
        }
    }

    pub async fn pump_adapter_once(&mut self) -> Result<PumpStatus> {
        let Some(message) = self.adapter.next_message().await? else {
            return Ok(PumpStatus::AdapterClosed);
        };
        let command = gateway_message_to_command(message, self.turn_policy.clone());
        self.active_conversation_id = command.conversation_id().map(ToOwned::to_owned);
        self.node_client()
            .send_command(command)?;
        Ok(PumpStatus::Active)
    }

    pub async fn pump_node_once(&mut self) -> Result<PumpStatus> {
        let Some(event) = self.next_node_event().await? else {
            return Ok(PumpStatus::NodeClosed);
        };
        self.handle_node_event(event).await
    }
}

#[cfg(test)]
mod tests {
    use super::{DirectGatewaySession, DirectNodeEvent, PumpStatus};
    use crate::adapter::GatewayAdapter;
    use crate::{GatewayMessage, GatewayOutbound};
    use agent_app_server_client::AppServerEvent;
    use agent_core::{ApprovalPolicy, PermissionProfile};
    use agent_protocol::{AppServerMessage, AppServerNotification, TurnPolicy};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::collections::VecDeque;

    struct FakeAdapter {
        inbound: VecDeque<GatewayMessage>,
        outbound: Vec<GatewayOutbound>,
    }

    #[async_trait]
    impl GatewayAdapter for FakeAdapter {
        async fn next_message(&mut self) -> Result<Option<GatewayMessage>> {
            Ok(self.inbound.pop_front())
        }

        async fn send_outbound(&mut self, outbound: GatewayOutbound) -> Result<()> {
            self.outbound.push(outbound);
            Ok(())
        }
    }

    fn turn_policy() -> TurnPolicy {
        TurnPolicy {
            permission_profile: PermissionProfile::ReadOnly,
            approval_policy: ApprovalPolicy::OnRequest,
        }
    }

    fn disconnected_event(message: &str) -> DirectNodeEvent {
        DirectNodeEvent::from(AppServerEvent::Disconnected {
            message: message.to_string(),
        })
    }

    #[test]
    fn app_server_disconnect_maps_to_direct_disconnect() {
        match disconnected_event("node closed") {
            DirectNodeEvent::Disconnected {
                conversation_id,
                message,
            } => {
                assert!(conversation_id.is_none());
                assert_eq!(message, "node closed");
            }
            other => panic!("unexpected direct node event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn node_messages_are_forwarded_to_adapter() {
        let mut session = test_session();
        let status = session
            .handle_node_event_for_test(DirectNodeEvent::Message(Box::new(
                AppServerMessage::Notification(AppServerNotification::AgentMessageDelta {
                    conversation_id: "conversation-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    item_id: "assistant:1".to_string(),
                    delta: "hello".to_string(),
                }),
            )))
            .await
            .expect("pump outbound");

        assert_eq!(status, PumpStatus::Active);
        assert_eq!(session.adapter().outbound.len(), 1);
        match &session.adapter().outbound[0] {
            GatewayOutbound::TextDelta {
                conversation_id,
                delta,
            } => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(delta, "hello");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[tokio::test]
    async fn lagged_events_are_forwarded_to_adapter() {
        let mut session = test_session();
        let status = session
            .handle_node_event_for_test(DirectNodeEvent::Lagged {
                conversation_id: None,
                skipped: 3,
            })
            .await
            .expect("pump lagged");

        assert_eq!(status, PumpStatus::Active);
        match &session.adapter().outbound[0] {
            GatewayOutbound::Info {
                conversation_id,
                message,
            } => {
                assert_eq!(conversation_id, "");
                assert!(message.contains("lagged by 3 events"));
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[tokio::test]
    async fn node_disconnect_surfaces_error_and_closes_session() {
        let mut session = test_session();
        let status = session
            .handle_node_event_for_test(DirectNodeEvent::Disconnected {
                conversation_id: Some("conversation-1".to_string()),
                message: "node closed".to_string(),
            })
            .await
            .expect("pump disconnect");

        assert_eq!(status, PumpStatus::NodeClosed);
        assert_eq!(session.adapter().outbound.len(), 1);
        match &session.adapter().outbound[0] {
            GatewayOutbound::Error {
                conversation_id,
                message,
            } => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(message, "node closed");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    fn test_session() -> TestDirectGatewaySession {
        DirectGatewaySession {
            adapter: FakeAdapter {
                inbound: VecDeque::new(),
                outbound: Vec::new(),
            },
            node_client: None,
            turn_policy: turn_policy(),
            active_conversation_id: None,
        }
    }

    type TestDirectGatewaySession = DirectGatewaySession<FakeAdapter>;

    impl TestDirectGatewaySession {
        async fn handle_node_event_for_test(&mut self, event: DirectNodeEvent) -> Result<PumpStatus> {
            self.handle_node_event(event).await
        }
    }
}
