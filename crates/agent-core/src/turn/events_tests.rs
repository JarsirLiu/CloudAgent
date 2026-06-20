use super::*;

#[test]
fn turn_started_preserves_structured_user_input() {
    let value = serde_json::json!({
        "type": "turn_started",
        "turn_id": "turn-1",
        "conversation_id": "default",
        "user_input": [
            { "type": "text", "text": "look at this" },
            {
                "type": "image",
                "source": {
                    "type": "remote_url",
                    "url": "https://example.com/diagram.png"
                },
                "detail": "high",
                "alt": "diagram"
            }
        ]
    });

    let parsed: EventMsg = serde_json::from_value(value.clone()).expect("parse event");
    let reserialized = serde_json::to_value(parsed).expect("serialize event");

    assert_eq!(reserialized, value);
}
