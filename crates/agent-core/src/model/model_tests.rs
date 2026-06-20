use super::ModelUsage;

#[test]
fn total_output_tokens_includes_reasoning_tokens() {
    let usage = ModelUsage {
        input_tokens: 1,
        cached_input_tokens: 2,
        output_tokens: 3,
        reasoning_output_tokens: 4,
        total_tokens: 10,
    };

    assert_eq!(usage.total_output_tokens(), 7);
}

#[test]
fn total_consumed_tokens_includes_reasoning_and_input() {
    let usage = ModelUsage {
        input_tokens: 11,
        cached_input_tokens: 2,
        output_tokens: 3,
        reasoning_output_tokens: 4,
        total_tokens: 99,
    };

    assert_eq!(usage.total_consumed_tokens(), 18);
}
