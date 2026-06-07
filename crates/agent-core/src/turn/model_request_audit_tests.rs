use super::{CompactionContinuation, build_model_request_shape_audit};
use crate::conversation::ResponseItem;
use crate::text_input_items;

#[test]
fn audit_reports_summary_and_raw_tail_shape() {
    let audit = build_model_request_shape_audit(
        &[
            ResponseItem::System {
                content: "system".to_string(),
            },
            ResponseItem::User {
                content: text_input_items("latest"),
            },
            ResponseItem::Assistant {
                content: Some("raw".to_string()),
                reasoning: None,
                tool_calls: Vec::new(),
            },
            ResponseItem::User {
                content: text_input_items("[Context Summary]\nsummary"),
            },
        ],
        2,
        Some(CompactionContinuation::MidTurn),
    );

    assert_eq!(audit.compaction_phase, Some("mid_turn"));
    assert_eq!(audit.summary_index, Some(3));
    assert_eq!(audit.latest_real_user_index, Some(1));
    assert_eq!(audit.raw_assistant_messages_before_summary, 1);
    assert_eq!(audit.raw_tool_messages_before_summary, 0);
}
