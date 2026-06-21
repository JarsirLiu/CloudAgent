use super::format_runtime_metrics;
use agent_core::RuntimeItemMetrics;

#[test]
fn formats_tokens_and_counts_in_stable_order() {
    let metrics = RuntimeItemMetrics {
        input_tokens: Some(1_250),
        output_tokens: Some(42),
        total_tokens: Some(1_292),
        elapsed_ms: Some(480),
        bytes_read: None,
        bytes_written: None,
        file_count: Some(3),
        source_count: Some(2),
        result_count: Some(6),
    };

    assert_eq!(
        format_runtime_metrics(&metrics).as_deref(),
        Some(
            "1.2k input tok · 42 output tok · 1.3k total tok · 3 files · 2 sources · 6 results · 480 ms"
        )
    );
}

#[test]
fn returns_none_when_no_renderable_metrics_exist() {
    let metrics = RuntimeItemMetrics {
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        elapsed_ms: None,
        bytes_read: None,
        bytes_written: None,
        file_count: None,
        source_count: None,
        result_count: None,
    };

    assert_eq!(format_runtime_metrics(&metrics), None);
}
