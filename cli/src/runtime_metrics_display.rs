use agent_core::RuntimeItemMetrics;

pub(crate) fn format_runtime_metrics(metrics: &RuntimeItemMetrics) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(input_tokens) = metrics.input_tokens {
        parts.push(format!("{} input tok", compact_number(input_tokens)));
    }
    if let Some(output_tokens) = metrics.output_tokens {
        parts.push(format!("{} output tok", compact_number(output_tokens)));
    }
    if let Some(total_tokens) = metrics.total_tokens {
        parts.push(format!("{} total tok", compact_number(total_tokens)));
    }
    if let Some(file_count) = metrics.file_count {
        parts.push(format!("{file_count} files"));
    }
    if let Some(source_count) = metrics.source_count {
        parts.push(format!("{source_count} sources"));
    }
    if let Some(result_count) = metrics.result_count {
        parts.push(format!("{result_count} results"));
    }
    if let Some(elapsed_ms) = metrics.elapsed_ms {
        parts.push(format!("{elapsed_ms} ms"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
#[path = "runtime_metrics_display_tests.rs"]
mod tests;
