use crate::app::core::running_turn_restore::restore_running_turn_cells;
use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::turn::TurnState;
use agent_core::{RuntimeItem, RuntimeItemProgress, RuntimeItemSnapshot, TurnItemKind};

fn user(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::UserMessage {
        id: id.to_string(),
        content: vec![InputItem::Text {
            text: text.to_string(),
        }],
    }
}

fn reasoning(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::Reasoning {
        id: id.to_string(),
        title: "Reasoning".to_string(),
        text: text.to_string(),
    }
}

fn agent(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
    }
}

#[test]
fn running_turn_restore_keeps_last_tail_live() {
    let turn = ConversationTurn {
        id: "turn-1".to_string(),
        state: TurnState::Running,
        rollout_start_index: 0,
        rollout_end_index: 3,
        items: vec![
            user("u1", "hello"),
            reasoning("r1", "thinking"),
            agent("a1", "done"),
        ],
        runtime_items: Vec::new(),
    };

    let restored = restore_running_turn_cells(turn);
    let replay = restored
        .replay_cells
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    let live = restored
        .live_cells
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(replay, vec!["hello".to_string(), "thinking".to_string()]);
    assert_eq!(live, vec!["done".to_string()]);
    assert_eq!(restored.last_copyable_output.as_deref(), Some("done"));
}

#[test]
fn running_turn_restore_prefers_runtime_snapshot_for_live_tool_item() {
    let turn = ConversationTurn {
        id: "turn-1".to_string(),
        state: TurnState::Running,
        rollout_start_index: 0,
        rollout_end_index: 2,
        items: vec![
            user("u1", "hello"),
            TranscriptItem::ToolResult {
                id: "tool-1".to_string(),
                tool_name: "web_search".to_string(),
                content: String::new(),
                summary: "running".to_string(),
                structured: None,
            },
        ],
        runtime_items: vec![RuntimeItemSnapshot {
            item: RuntimeItem::started(
                "tool-1",
                Some("tool-1".to_string()),
                TurnItemKind::ToolResult,
                Some("web_search".to_string()),
            )
            .with_progress(RuntimeItemProgress::message("weather seattle")),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: "weather seattle".to_string(),
            patch_buffer: String::new(),
        }],
    };

    let restored = restore_running_turn_cells(turn);
    let replay = restored
        .replay_cells
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    let live = restored
        .live_cells
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(replay, vec!["hello".to_string()]);
    assert_eq!(live, vec!["weather seattle".to_string()]);
}
