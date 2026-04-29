mod regular;

use crate::AgentRuntime;
use agent_core::ConversationHistory;
use agent_protocol::{EventMsg, ServerRequest, ServerRequestDecision};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(crate) use regular::{RegularTurnTask, TurnOutcome};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskKind {
    Regular,
    Monitor,
    Wakeup,
}

pub(crate) struct TaskContext<'a, E> {
    pub(crate) runtime: &'a AgentRuntime,
    pub(crate) conversation_id: &'a str,
    pub(crate) turn_id: &'a str,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) on_event: &'a mut E,
}

pub(crate) trait RuntimeTask<E, F, Fut>
where
    E: FnMut(&EventMsg) + Send,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    #[allow(dead_code)]
    fn kind(&self) -> TaskKind;

    fn run(
        self,
        ctx: TaskContext<'_, E>,
        history: ConversationHistory,
        approval: F,
    ) -> impl std::future::Future<Output = Result<TurnOutcome>> + Send;
}
