use crate::InputItem;
use crate::turn::{
    AgentTurnOutput, EventMsg, ManualCompactionOutcome, ServerRequest, ServerRequestDecision,
    TurnHost, run_manual_compaction, run_turn_with_approval,
};
use anyhow::Result;

pub async fn compact_conversation<H>(
    host: &H,
    conversation_id: &str,
    minimum_history_tokens: usize,
) -> Result<ManualCompactionOutcome>
where
    H: TurnHost,
{
    host.flush_rollout().await?;
    run_manual_compaction(host, conversation_id, minimum_history_tokens).await
}

pub async fn chat<H>(
    host: &H,
    conversation_id: &str,
    user_input: &[InputItem],
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
) -> Result<AgentTurnOutput>
where
    H: TurnHost,
{
    chat_with_approval_and_events(
        host,
        conversation_id,
        user_input,
        permission_profile,
        approval_policy,
        |_event| {},
        |_request| async move {
            Ok(ServerRequestDecision::decline(Some(
                "Mutating tools require an approval-capable client. Use the interactive cli."
                    .to_string(),
            )))
        },
    )
    .await
}

pub async fn chat_with_approval<H, F, Fut>(
    host: &H,
    conversation_id: &str,
    user_input: &[InputItem],
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
    approval: F,
) -> Result<AgentTurnOutput>
where
    H: TurnHost,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    chat_with_approval_and_events(
        host,
        conversation_id,
        user_input,
        permission_profile,
        approval_policy,
        |_event| {},
        approval,
    )
    .await
}

pub async fn chat_with_approval_and_events<H, E, F, Fut>(
    host: &H,
    conversation_id: &str,
    user_input: &[InputItem],
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
    mut on_event: E,
    approval: F,
) -> Result<AgentTurnOutput>
where
    H: TurnHost,
    E: FnMut(&EventMsg) + Send,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    host.audit_turn_started(conversation_id, user_input);
    let outcome = run_turn_with_approval(
        host,
        conversation_id,
        user_input,
        permission_profile,
        approval_policy,
        &mut on_event,
        &approval,
    )
    .await?;
    host.audit_turn_completed(
        conversation_id,
        &outcome.turn_id,
        &format!("{:?}", outcome.state),
        outcome.events.len(),
        outcome.model_name.as_deref(),
    );
    Ok(crate::projection::agent_turn_output_from_events(
        outcome.turn_id,
        outcome.events,
        &outcome.history,
        outcome.model_name,
        outcome.state,
    ))
}
