use super::*;
use crate::runtime_item::RuntimeItem;
use crate::tool::{CommandExecutionStatus, StructuredToolResult};
use crate::turn::TurnItemKind;
use crate::{input_items_to_plain_text, text_input_items};

fn started(
    turn_id: &str,
    item_id: &str,
    call_id: Option<&str>,
    kind: TurnItemKind,
    title: Option<&str>,
) -> EventMsg {
    EventMsg::ItemStarted {
        turn_id: turn_id.to_string(),
        item: RuntimeItem::started(
            item_id,
            call_id.map(str::to_string),
            kind,
            title.map(str::to_string),
        ),
    }
}

fn completed(turn_id: &str, item: TranscriptItem, call_id: Option<&str>) -> EventMsg {
    let runtime_item = RuntimeItem::completed(&item, call_id.map(str::to_string));
    EventMsg::ItemCompleted {
        turn_id: turn_id.to_string(),
        runtime_item,
        transcript_item: item,
    }
}

#[test]
fn transcript_builder_projects_rollout_facts_without_duplicate_messages() {
    let assistant = TranscriptItem::AgentMessage {
        id: "assistant-1".to_string(),
        text: "hello".to_string(),
    };
    let items = vec![
        RolloutItem::from(ResponseItem::User {
            content: crate::text_input_items("hi"),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(completed("turn-1", assistant.clone(), None)),
        RolloutItem::from(ResponseItem::Assistant {
            content: Some("hello".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let transcript = transcript_items_from_rollout_items(&items);

    assert_eq!(transcript.len(), 2);
    assert!(matches!(transcript[0], TranscriptItem::UserMessage { .. }));
    assert!(matches!(
        &transcript[1],
        TranscriptItem::AgentMessage { text, .. } if text == "hello"
    ));
}

#[test]
fn active_turn_snapshot_projects_started_delta_before_completion() {
    let mut builder = ConversationHistoryBuilder::new();

    for item in [
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(started(
            "turn-1",
            "assistant:turn-1:0",
            None,
            TurnItemKind::AssistantMessage,
            Some("assistant_message"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:turn-1:0".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "partial".to_string(),
        }),
    ] {
        builder.push_rollout_item(&item);
    }

    let snapshot = builder.active_turn_snapshot().expect("active turn");

    assert!(matches!(
        &snapshot.items[..],
        [
            TranscriptItem::UserMessage { content: user, .. },
            TranscriptItem::AgentMessage { text: assistant, .. },
        ] if input_items_to_plain_text(user) == "hi" && assistant == "partial"
    ));
}

#[test]
fn failed_turn_preserves_streamed_partial_assistant_message() {
    let turns = build_turns_from_rollout_items(&[
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(started(
            "turn-1",
            "assistant:turn-1:0",
            None,
            TurnItemKind::AssistantMessage,
            Some("assistant_message"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:turn-1:0".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "partial answer".to_string(),
        }),
        RolloutItem::from(EventMsg::TurnFailed {
            turn_id: "turn-1".to_string(),
            error: "provider protocol error".to_string(),
        }),
    ]);

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].state, TurnState::Failed);
    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { content: user, .. },
            TranscriptItem::AgentMessage { text: assistant, .. },
            TranscriptItem::SystemMessage { text: error, .. },
        ] if input_items_to_plain_text(user) == "hi"
            && assistant == "partial answer"
            && error == "provider protocol error"
    ));
}

#[test]
fn item_completed_replaces_streamed_delta_projection() {
    let turns = build_turns_from_rollout_items(&[
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(started(
            "turn-1",
            "assistant:turn-1:0",
            None,
            TurnItemKind::AssistantMessage,
            Some("assistant_message"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:turn-1:0".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "partial".to_string(),
        }),
        RolloutItem::from(completed(
            "turn-1",
            TranscriptItem::AgentMessage {
                id: "assistant:turn-1:0".to_string(),
                text: "final".to_string(),
            },
            None,
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ]);

    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { .. },
            TranscriptItem::AgentMessage { text, .. },
        ] if text == "final"
    ));
}

#[test]
fn transcript_items_ignore_compaction_rollout_items() {
    let items = vec![
        RolloutItem::EventMsg {
            event: EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: crate::text_input_items("hi"),
            },
        },
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("hi"),
        }),
        RolloutItem::Compacted {
            summary: crate::context::CompactionSummary::from_model_output(
                "Current Task:\n- hidden",
            )
            .ensure_defaults(),
            rendered_summary: "[Context Summary]\nhidden".to_string(),
            trigger: crate::turn::CompactionTrigger::Auto,
            reason: crate::turn::CompactionReason::ContextLimit,
            phase: crate::turn::CompactionPhase::PreTurn,
            replacement_history: vec![],
        },
        RolloutItem::EventMsg {
            event: EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            },
        },
    ];

    let transcript = transcript_items_from_rollout_items(&items);

    assert_eq!(transcript.len(), 1);
    assert!(matches!(
        &transcript[0],
        TranscriptItem::UserMessage { content, .. } if input_items_to_plain_text(content) == "hi"
    ));
}

#[test]
fn transcript_builder_keeps_rich_tool_projection() {
    let item = transcript_item_from_response_item(&ResponseItem::Tool {
        tool_call_id: "call-1".to_string(),
        name: "exec_command".to_string(),
        content: "D:\\learn\\gifti\\cloudagent".to_string(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: "pwd".to_string(),
            current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
            session_id: None,
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            success: Some(true),
            output: Some("D:\\learn\\gifti\\cloudagent".to_string()),
            duration_ms: Some(1),
            original_token_count: Some(8),
            max_output_tokens: Some(10_000),
        }),
    })
    .expect("tool response should project");

    assert!(matches!(
        item,
        TranscriptItem::CommandExecution {
            command,
            status: CommandExecutionStatus::Completed,
            ..
        } if command == "pwd"
    ));
}

#[test]
fn lifecycle_only_events_do_not_create_transcript_items() {
    let items = vec![
        RolloutItem::from(started(
            "turn-1",
            "item-1",
            None,
            TurnItemKind::AssistantMessage,
            None,
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
        RolloutItem::from(EventMsg::TurnFailed {
            turn_id: "turn-2".to_string(),
            error: String::new(),
        }),
    ];

    assert!(transcript_items_from_rollout_items(&items).is_empty());
}

#[test]
fn conversation_history_builder_preserves_turn_boundaries() {
    let items = vec![
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("first"),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("first"),
        }),
        RolloutItem::from(completed(
            "turn-1",
            TranscriptItem::AgentMessage {
                id: "assistant-1".to_string(),
                text: "one".to_string(),
            },
            None,
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("second"),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-2".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("second"),
        }),
        RolloutItem::from(completed(
            "turn-2",
            TranscriptItem::AgentMessage {
                id: "assistant-2".to_string(),
                text: "two".to_string(),
            },
            None,
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-2".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "turn-1");
    assert_eq!(turns[0].state, TurnState::Completed);
    assert_eq!(turns[1].id, "turn-2");
    assert_eq!(turns[1].state, TurnState::Completed);
    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { content: first, .. },
            TranscriptItem::AgentMessage { text: one, .. }
        ] if input_items_to_plain_text(first) == "first" && one == "one"
    ));
    assert!(matches!(
        &turns[1].items[..],
        [
            TranscriptItem::UserMessage { content: second, .. },
            TranscriptItem::AgentMessage { text: two, .. }
        ] if input_items_to_plain_text(second) == "second" && two == "two"
    ));
}

#[test]
fn explicit_turn_restores_response_items_without_item_completed_events() {
    let items = vec![
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("hi"),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(ResponseItem::Assistant {
            content: Some("hello".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].id, "turn-1");
    assert_eq!(turns[0].state, TurnState::Completed);
    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { content, .. },
            TranscriptItem::AgentMessage { text, .. },
        ] if input_items_to_plain_text(content) == "hi" && text == "hello"
    ));
}

#[test]
fn explicit_turn_restores_tool_response_items_without_item_completed_events() {
    let items = vec![
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("inspect"),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("inspect"),
        }),
        RolloutItem::from(ResponseItem::Tool {
            tool_call_id: "call-1".to_string(),
            name: "read_file".to_string(),
            content: "Summary: read file".to_string(),
            structured: Some(StructuredToolResult::ToolError {
                tool_name: "read_file".to_string(),
                message: "example".to_string(),
            }),
        }),
        RolloutItem::from(ResponseItem::Assistant {
            content: Some("done".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { .. },
            TranscriptItem::ToolResult { tool_name, .. },
            TranscriptItem::AgentMessage { text, .. },
        ] if tool_name == "read_file" && text == "done"
    ));
}

#[test]
fn jsonl_rollout_restore_keeps_assistant_response_items() {
    let jsonl = [
        r#"{"type":"response_item","item":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
        r#"{"type":"event_msg","event":{"type":"turn_started","turn_id":"turn-1","conversation_id":"default","user_input":[{"type":"text","text":"hi"}]}}"#,
        r#"{"type":"response_item","item":{"role":"assistant","content":"hello","reasoning":null,"tool_calls":[]}}"#,
        r#"{"type":"event_msg","event":{"type":"turn_completed","turn_id":"turn-1"}}"#,
    ];
    let items = jsonl
        .iter()
        .map(|line| serde_json::from_str::<RolloutItem>(line).expect("valid rollout jsonl"))
        .collect::<Vec<_>>();

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { content, .. },
            TranscriptItem::AgentMessage { text, .. },
        ] if input_items_to_plain_text(content) == "hi" && text == "hello"
    ));
}

#[test]
fn explicit_turn_skips_duplicate_response_user_item() {
    let items = vec![
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("hi"),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(ResponseItem::User {
            content: text_input_items("hi"),
        }),
        RolloutItem::from(ResponseItem::Assistant {
            content: Some("hello".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(
        turns[0]
            .items
            .iter()
            .filter(|item| matches!(item, TranscriptItem::UserMessage { .. }))
            .count(),
        1
    );
    assert!(matches!(
        turns[0].items.last(),
        Some(TranscriptItem::AgentMessage { text, .. }) if text == "hello"
    ));
}

#[test]
fn filter_history_ui_turns_drops_reasoning_items_and_empty_turns() {
    let turns = vec![
        ConversationTurn {
            id: "turn-1".to_string(),
            state: TurnState::Completed,
            items: vec![
                TranscriptItem::Reasoning {
                    id: "reasoning:1".to_string(),
                    title: "reasoning".to_string(),
                    text: "thinking".to_string(),
                },
                TranscriptItem::AgentMessage {
                    id: "assistant:1".to_string(),
                    text: "answer".to_string(),
                },
            ],
            runtime_items: Vec::new(),
            rollout_start_index: 0,
            rollout_end_index: 1,
        },
        ConversationTurn {
            id: "turn-2".to_string(),
            state: TurnState::Completed,
            items: vec![TranscriptItem::Reasoning {
                id: "reasoning:2".to_string(),
                title: "reasoning".to_string(),
                text: "only reasoning".to_string(),
            }],
            runtime_items: Vec::new(),
            rollout_start_index: 2,
            rollout_end_index: 2,
        },
    ];

    let filtered = filter_history_ui_turns(turns);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "turn-1");
    assert!(matches!(
        &filtered[0].items[..],
        [TranscriptItem::AgentMessage { text, .. }] if text == "answer"
    ));
}

#[test]
fn same_assistant_text_keeps_distinct_items_by_id() {
    let items = vec![
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("repeat"),
        }),
        RolloutItem::from(completed(
            "turn-1",
            TranscriptItem::AgentMessage {
                id: "assistant-1".to_string(),
                text: "same".to_string(),
            },
            None,
        )),
        RolloutItem::from(completed(
            "turn-1",
            TranscriptItem::AgentMessage {
                id: "assistant-2".to_string(),
                text: "same".to_string(),
            },
            None,
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0]
            .items
            .iter()
            .filter(
                |item| matches!(item, TranscriptItem::AgentMessage { text, .. } if text == "same")
            )
            .count(),
        2
    );
}

#[test]
fn late_item_completed_updates_original_turn_by_turn_id() {
    let items = vec![
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("first"),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-2".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("second"),
        }),
        RolloutItem::from(completed(
            "turn-1",
            TranscriptItem::AgentMessage {
                id: "assistant-late".to_string(),
                text: "late answer".to_string(),
            },
            None,
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-2".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 2);
    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { content: first, .. },
            TranscriptItem::AgentMessage { text: answer, .. }
        ] if input_items_to_plain_text(first) == "first" && answer == "late answer"
    ));
    assert!(matches!(
        &turns[1].items[..],
        [TranscriptItem::UserMessage { content, .. }] if input_items_to_plain_text(content) == "second"
    ));
}

#[test]
fn same_tool_summary_keeps_distinct_items_by_id() {
    let command_item = |id: &str| TranscriptItem::CommandExecution {
        id: id.to_string(),
        tool_name: "exec_command".to_string(),
        command: "pwd".to_string(),
        current_directory: "D:\\work".to_string(),
        status: CommandExecutionStatus::Completed,
        exit_code: Some(0),
        output: Some("D:\\work".to_string()),
        duration_ms: Some(1),
        summary: "D:\\work".to_string(),
    };
    let items = vec![
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("twice"),
        }),
        RolloutItem::from(completed("turn-1", command_item("tool-1"), Some("call-1"))),
        RolloutItem::from(completed("turn-1", command_item("tool-2"), Some("call-2"))),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0]
            .items
            .iter()
            .filter(|item| matches!(item, TranscriptItem::CommandExecution { summary, .. } if summary == "D:\\work"))
            .count(),
        2
    );
}

#[test]
fn late_reasoning_stays_after_assistant_in_arrival_order() {
    let turns = build_turns_from_rollout_items(&[
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(started(
            "turn-1",
            "assistant:1",
            None,
            TurnItemKind::AssistantMessage,
            Some("assistant_message"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "final".to_string(),
        }),
        RolloutItem::from(started(
            "turn-1",
            "reasoning:1",
            None,
            TurnItemKind::Reasoning,
            Some("reasoning"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningSummary,
            segment_index: None,
            delta: "thinking".to_string(),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ]);

    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { .. },
            TranscriptItem::AgentMessage { text: assistant, .. },
            TranscriptItem::Reasoning { text: reasoning, .. },
        ] if reasoning == "thinking" && assistant == "final"
    ));
}

#[test]
fn late_reasoning_and_tools_preserve_arrival_order_after_assistant() {
    let turns = build_turns_from_rollout_items(&[
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(started(
            "turn-1",
            "assistant:1",
            None,
            TurnItemKind::AssistantMessage,
            Some("assistant_message"),
        )),
        RolloutItem::from(started(
            "turn-1",
            "tool:1",
            Some("call-1"),
            TurnItemKind::CommandExecution,
            Some("pwd"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "D:\\work".to_string(),
        }),
        RolloutItem::from(started(
            "turn-1",
            "reasoning:1",
            None,
            TurnItemKind::Reasoning,
            Some("reasoning"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningSummary,
            segment_index: None,
            delta: "thinking".to_string(),
        }),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ]);

    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { .. },
            TranscriptItem::AgentMessage { .. },
            TranscriptItem::CommandExecution { .. },
            TranscriptItem::Reasoning { .. },
        ]
    ));
}

#[test]
fn later_reasoning_preserves_arrival_order_relative_to_existing_blocks() {
    let turns = build_turns_from_rollout_items(&[
        RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "default".to_string(),
            user_input: crate::text_input_items("hi"),
        }),
        RolloutItem::from(started(
            "turn-1",
            "reasoning:1",
            None,
            TurnItemKind::Reasoning,
            Some("reasoning"),
        )),
        RolloutItem::from(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::ReasoningSummary,
            segment_index: None,
            delta: "thinking 1".to_string(),
        }),
        RolloutItem::from(started(
            "turn-1",
            "tool:1",
            Some("call-1"),
            TurnItemKind::CommandExecution,
            Some("pwd"),
        )),
        RolloutItem::from(started(
            "turn-1",
            "assistant:1",
            None,
            TurnItemKind::AssistantMessage,
            Some("assistant_message"),
        )),
        RolloutItem::from(started(
            "turn-1",
            "reasoning:2",
            None,
            TurnItemKind::Reasoning,
            Some("reasoning"),
        )),
        RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ]);

    assert!(matches!(
        &turns[0].items[..],
        [
            TranscriptItem::UserMessage { .. },
            TranscriptItem::Reasoning { id: first_reasoning, .. },
            TranscriptItem::CommandExecution { .. },
            TranscriptItem::AgentMessage { .. },
            TranscriptItem::Reasoning { id: second_reasoning, .. },
        ] if first_reasoning == "reasoning:1" && second_reasoning == "reasoning:2"
    ));
}
