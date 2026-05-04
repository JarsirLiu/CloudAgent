use super::{
    ServerRequestHandler, TurnHost, TurnOutcome, conversation_busy_error, execute_regular_turn,
};
use crate::{EventMsg, TurnState};
use crate::{RolloutItem, ToolCall, emit_event, next_turn_id};
use anyhow::{Result, bail};

pub async fn run_turn_with_approval<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    user_input: &str,
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
    approval: &dyn ServerRequestHandler,
) -> Result<TurnOutcome> {
    let mut event_sink = |event: &EventMsg| {
        host.append_conversation_event(conversation_id, event.clone());
        if let Err(err) =
            host.record_rollout_items(conversation_id, &[RolloutItem::from(event.clone())])
        {
            tracing::error!("failed to queue rollout event: {err:#}");
        }
        on_event(event);
    };

    let turn_id = next_turn_id();
    let Some(active_turn) = host
        .start_turn(conversation_id.to_string(), turn_id.clone())
        .await
    else {
        bail!(conversation_busy_error());
    };

    let mut history = host.load_history(conversation_id).await?;
    let user_item = history.push_user_message(user_input);
    host.save_history(history.clone()).await?;
    host.persist_rollout_items(conversation_id, &[RolloutItem::from(user_item)])
        .await?;

    let mut events = Vec::new();
    emit_event(
        &mut events,
        &mut event_sink,
        EventMsg::TurnStarted {
            turn_id: turn_id.clone(),
            conversation_id: conversation_id.to_string(),
            user_input: user_input.to_string(),
        },
    );
    let result = if active_turn.is_cancelled() {
        emit_event(
            &mut events,
            &mut event_sink,
            EventMsg::TurnCancelled {
                turn_id: turn_id.clone(),
                reason: "interrupted by client".to_string(),
            },
        );
        Ok(TurnOutcome {
            turn_id: turn_id.clone(),
            events,
            history,
            model_name: None,
            state: TurnState::Cancelled,
        })
    } else {
        execute_regular_turn(
            host,
            conversation_id,
            &turn_id,
            permission_profile,
            approval_policy,
            active_turn.cancellation_token.clone(),
            history,
            &mut event_sink,
            approval,
        )
        .await
    };

    host.finish_turn(conversation_id).await;

    match result {
        Ok(mut outcome) => {
            ensure_terminal_event(&mut outcome, &turn_id, &mut event_sink);
            if host.should_persist_memory(&outcome.history) {
                host.persist_memory_from_history(&outcome.history);
            }
            host.save_history(outcome.history.clone()).await?;
            host.flush_rollout().await?;
            Ok(outcome)
        }
        Err(err) => {
            if err.to_string().contains(host.turn_interrupted_error()) {
                let mut events = Vec::new();
                emit_event(
                    &mut events,
                    &mut event_sink,
                    EventMsg::TurnCancelled {
                        turn_id: turn_id.clone(),
                        reason: "interrupted by client".to_string(),
                    },
                );
                host.flush_rollout().await?;
                let mut interrupted_history = host.history_from_rollout(conversation_id).await?;
                interrupted_history.ensure_tool_outputs_present();
                host.save_history(interrupted_history.clone()).await?;
                host.flush_rollout().await?;
                let outcome = TurnOutcome {
                    turn_id: turn_id.clone(),
                    events,
                    history: interrupted_history,
                    model_name: None,
                    state: TurnState::Cancelled,
                };
                host.audit_turn_cancelled(conversation_id, &turn_id, "interrupted by client");
                return Ok(outcome);
            }
            let mut history = host.load_history(conversation_id).await?;
            let failed_item = history.push_assistant_message(
                Some(format!("Turn failed: {err:#}")),
                Vec::<ToolCall>::new(),
            );
            host.persist_rollout_items(conversation_id, &[RolloutItem::from(failed_item)])
                .await?;
            let error_text = format!("{err:#}");
            let mut events = Vec::new();
            emit_event(
                &mut events,
                &mut event_sink,
                EventMsg::TurnFailed {
                    turn_id: turn_id.clone(),
                    error: error_text.clone(),
                },
            );
            host.save_history(history.clone()).await?;
            host.flush_rollout().await?;
            let outcome = TurnOutcome {
                turn_id: turn_id.clone(),
                events,
                history,
                model_name: None,
                state: TurnState::Failed,
            };
            host.audit_turn_failed(conversation_id, &turn_id, &error_text);
            Ok(outcome)
        }
    }
}

fn ensure_terminal_event(
    outcome: &mut TurnOutcome,
    turn_id: &str,
    event_sink: &mut impl FnMut(&EventMsg),
) {
    let has_terminal = outcome.events.iter().any(|event| {
        matches!(
            event,
            EventMsg::TurnCompleted { .. }
                | EventMsg::TurnFailed { .. }
                | EventMsg::TurnCancelled { .. }
        )
    });
    if has_terminal {
        return;
    }

    let event = match outcome.state {
        TurnState::Completed => EventMsg::TurnCompleted {
            turn_id: turn_id.to_string(),
        },
        TurnState::Cancelled => EventMsg::TurnCancelled {
            turn_id: turn_id.to_string(),
            reason: "turn cancelled".to_string(),
        },
        TurnState::Failed => EventMsg::TurnFailed {
            turn_id: turn_id.to_string(),
            error: "turn failed".to_string(),
        },
        TurnState::Idle | TurnState::Running | TurnState::WaitingForServerRequest => return,
    };
    let mut scratch_events = Vec::new();
    emit_event(&mut scratch_events, event_sink, event.clone());
    outcome.events.push(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConversationHistory;

    #[test]
    fn ensure_terminal_event_appends_completed_when_missing() {
        let mut outcome = TurnOutcome {
            turn_id: "turn-1".to_string(),
            events: Vec::new(),
            history: ConversationHistory::new("conv-1".to_string(), "system".to_string()),
            model_name: None,
            state: TurnState::Completed,
        };
        let mut delivered = Vec::new();
        ensure_terminal_event(&mut outcome, "turn-1", &mut |event| {
            delivered.push(event.clone());
        });

        assert!(matches!(
            outcome.events.as_slice(),
            [EventMsg::TurnCompleted { turn_id }] if turn_id == "turn-1"
        ));
        assert!(matches!(
            delivered.as_slice(),
            [EventMsg::TurnCompleted { turn_id }] if turn_id == "turn-1"
        ));
    }
}
