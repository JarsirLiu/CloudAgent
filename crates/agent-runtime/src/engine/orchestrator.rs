use crate::tasks::{RegularTurnTask, RuntimeTask, TaskContext, TurnOutcome};
use crate::{AgentRuntime, emit_event, is_turn_interrupted_error, next_turn_id};
use agent_core::{RolloutItem, ToolCall};
use agent_protocol::{EventMsg, ServerRequest, ServerRequestDecision, TurnState};
use anyhow::Result;

pub(crate) async fn run_turn_with_approval<E, F, Fut>(
    runtime: &AgentRuntime,
    conversation_id: &str,
    user_input: &str,
    on_event: &mut E,
    approval: F,
) -> Result<TurnOutcome>
where
    E: FnMut(&EventMsg) + Send,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    let mut event_sink = |event: &EventMsg| {
        runtime
            .state
            .append_conversation_event(conversation_id, event.clone());
        if let Err(err) =
            runtime.record_rollout_items(conversation_id, &[RolloutItem::from(event.clone())])
        {
            tracing::error!("failed to queue rollout event: {err:#}");
        }
        on_event(event);
    };

    let turn_id = next_turn_id();
    let active_turn = runtime
        .state
        .start_turn(conversation_id.to_string(), turn_id.clone())
        .await;

    let mut history = runtime.load_history(conversation_id).await?;
    let user_item = history.push_user_message(user_input);
    runtime.state.save_history(history.clone()).await;
    runtime
        .persist_rollout_items(conversation_id, &[RolloutItem::from(user_item)])
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
        RegularTurnTask
            .run(
                TaskContext {
                    runtime,
                    conversation_id,
                    turn_id: &turn_id,
                    cancellation_token: active_turn.cancellation_token.clone(),
                    on_event: &mut event_sink,
                },
                history,
                approval,
            )
            .await
    };

    runtime.state.finish_turn(conversation_id).await;

    match result {
        Ok(outcome) => {
            drop(event_sink);
            runtime.save_history(outcome.history.clone()).await?;
            runtime.rollout_recorder.flush().await?;
            Ok(outcome)
        }
        Err(err) => {
            if is_turn_interrupted_error(&err) {
                let mut events = Vec::new();
                emit_event(
                    &mut events,
                    &mut event_sink,
                    EventMsg::TurnCancelled {
                        turn_id: turn_id.clone(),
                        reason: "interrupted by client".to_string(),
                    },
                );
                drop(event_sink);
                runtime.rollout_recorder.flush().await?;
                let mut interrupted_history = runtime.history_from_rollout(conversation_id).await?;
                interrupted_history.ensure_tool_outputs_present();
                runtime.save_history(interrupted_history.clone()).await?;
                runtime.rollout_recorder.flush().await?;
                let outcome = TurnOutcome {
                    turn_id: turn_id.clone(),
                    events,
                    history: interrupted_history,
                    model_name: None,
                    state: TurnState::Cancelled,
                };
                return Ok(outcome);
            }
            let mut history = runtime.load_history(conversation_id).await?;
            let failed_item = history.push_assistant_message(
                Some(format!("Turn failed: {err:#}")),
                Vec::<ToolCall>::new(),
            );
            runtime
                .persist_rollout_items(conversation_id, &[RolloutItem::from(failed_item)])
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
            drop(event_sink);
            runtime.save_history(history.clone()).await?;
            runtime.rollout_recorder.flush().await?;
            let outcome = TurnOutcome {
                turn_id: turn_id.clone(),
                events,
                history,
                model_name: None,
                state: TurnState::Failed,
            };
            Ok(outcome)
        }
    }
}
