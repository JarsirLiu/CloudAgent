use crate::app::core::running_turn_restore::restore_running_turn_cells;
use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::turn::TurnState;

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
