use crate::GatewayOutbound;
use crate::adapter::GatewayAdapter;
use crate::direct::{app_server_message_to_outbound, gateway_message_to_command};
use agent_protocol::{AppClientCommand, AppServerMessage, TurnPolicy};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub enum DirectNodeEvent {
    Message(AppServerMessage),
    Lagged {
        conversation_id: Option<String>,
        skipped: usize,
    },
    Disconnected {
        conversation_id: Option<String>,
        message: String,
    },
}

#[async_trait]
pub trait DirectNodeClient: Send {
    async fn send_command(&mut self, command: AppClientCommand) -> Result<()>;

    async fn next_event(&mut self) -> Result<Option<DirectNodeEvent>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpStatus {
    Active,
    AdapterClosed,
    NodeClosed,
}

pub struct DirectGatewaySession<A, C> {
    adapter: A,
    node_client: C,
    turn_policy: TurnPolicy,
    active_conversation_id: Option<String>,
}

impl<A, C> DirectGatewaySession<A, C> {
    pub fn new(adapter: A, node_client: C, turn_policy: TurnPolicy) -> Self {
        Self {
            adapter,
            node_client,
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

    pub fn node_client(&self) -> &C {
        &self.node_client
    }

    pub fn node_client_mut(&mut self) -> &mut C {
        &mut self.node_client
    }
}

impl<A, C> DirectGatewaySession<A, C>
where
    A: GatewayAdapter,
    C: DirectNodeClient,
{
    pub async fn pump_adapter_once(&mut self) -> Result<PumpStatus> {
        let Some(message) = self.adapter.next_message().await? else {
            return Ok(PumpStatus::AdapterClosed);
        };
        let command = gateway_message_to_command(message, self.turn_policy.clone());
        self.active_conversation_id = command.conversation_id().map(ToOwned::to_owned);
        self.node_client.send_command(command).await?;
        Ok(PumpStatus::Active)
    }

    pub async fn pump_node_once(&mut self) -> Result<PumpStatus> {
        let Some(event) = self.node_client.next_event().await? else {
            return Ok(PumpStatus::NodeClosed);
        };

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
}

#[cfg(test)]
mod tests {
    use super::{DirectGatewaySession, DirectNodeClient, DirectNodeEvent, PumpStatus};
    use crate::adapter::GatewayAdapter;
    use crate::{GatewayMessage, GatewayOutbound};
    use agent_core::{ApprovalPolicy, InputItem, PermissionProfile};
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, TurnPolicy, UserTurnInput,
    };
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

    struct FakeNodeClient {
        sent_commands: Vec<AppClientCommand>,
        events: VecDeque<DirectNodeEvent>,
    }

    #[async_trait]
    impl DirectNodeClient for FakeNodeClient {
        async fn send_command(&mut self, command: AppClientCommand) -> Result<()> {
            self.sent_commands.push(command);
            Ok(())
        }

        async fn next_event(&mut self) -> Result<Option<DirectNodeEvent>> {
            Ok(self.events.pop_front())
        }
    }

    fn turn_policy() -> TurnPolicy {
        TurnPolicy {
            permission_profile: PermissionProfile::ReadOnly,
            approval_policy: ApprovalPolicy::OnRequest,
        }
    }

    #[tokio::test]
    async fn adapter_messages_are_forwarded_to_node_client() {
        let adapter = FakeAdapter {
            inbound: VecDeque::from([GatewayMessage::new(
                "conversation-1",
                "sender-1",
                vec![InputItem::Text {
                    text: "hello".to_string(),
                }],
            )]),
            outbound: Vec::new(),
        };
        let node_client = FakeNodeClient {
            sent_commands: Vec::new(),
            events: VecDeque::new(),
        };
        let mut session = DirectGatewaySession::new(adapter, node_client, turn_policy());

        let status = session.pump_adapter_once().await.expect("pump inbound");

        assert_eq!(status, PumpStatus::Active);
        assert_eq!(session.node_client().sent_commands.len(), 1);
        match &session.node_client().sent_commands[0] {
            AppClientCommand::SubmitTurn(UserTurnInput {
                conversation_id,
                content,
                ..
            }) => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(
                    content,
                    &vec![InputItem::Text {
                        text: "hello".to_string()
                    }]
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn node_messages_are_forwarded_to_adapter() {
        let adapter = FakeAdapter {
            inbound: VecDeque::new(),
            outbound: Vec::new(),
        };
        let node_client = FakeNodeClient {
            sent_commands: Vec::new(),
            events: VecDeque::from([DirectNodeEvent::Message(AppServerMessage::Notification(
                AppServerNotification::AgentMessageDelta {
                    conversation_id: "conversation-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    item_id: "assistant:1".to_string(),
                    delta: "hello".to_string(),
                },
            ))]),
        };
        let mut session = DirectGatewaySession::new(adapter, node_client, turn_policy());

        let status = session.pump_node_once().await.expect("pump outbound");

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
    async fn node_disconnect_surfaces_error_and_closes_session() {
        let adapter = FakeAdapter {
            inbound: VecDeque::new(),
            outbound: Vec::new(),
        };
        let node_client = FakeNodeClient {
            sent_commands: Vec::new(),
            events: VecDeque::from([DirectNodeEvent::Disconnected {
                conversation_id: Some("conversation-1".to_string()),
                message: "node closed".to_string(),
            }]),
        };
        let mut session = DirectGatewaySession::new(adapter, node_client, turn_policy());

        let status = session.pump_node_once().await.expect("pump disconnect");

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
}
