use super::auto_compact_window::{AutoCompactWindow, AutoCompactWindowSnapshot};
use crate::model::ModelUsage;

#[test]
fn starts_with_first_window_without_prefill() {
    let window = AutoCompactWindow::new();

    assert_eq!(
        window.snapshot(),
        AutoCompactWindowSnapshot {
            ordinal: 1,
            prefill_input_tokens: None,
        }
    );
}

#[test]
fn estimated_prefill_is_replaced_by_first_server_observed_usage() {
    let mut window = AutoCompactWindow::new();

    window.set_estimated_prefill(150);
    window.ensure_server_observed_prefill_from_usage(&ModelUsage {
        input_tokens: 120,
        total_tokens: 170,
        ..ModelUsage::default()
    });

    assert_eq!(
        window.snapshot(),
        AutoCompactWindowSnapshot {
            ordinal: 1,
            prefill_input_tokens: Some(120),
        }
    );
}

#[test]
fn server_observed_prefill_is_sticky() {
    let mut window = AutoCompactWindow::new();

    window.ensure_server_observed_prefill_from_usage(&ModelUsage {
        input_tokens: 120,
        total_tokens: 170,
        ..ModelUsage::default()
    });
    window.ensure_server_observed_prefill_from_usage(&ModelUsage {
        input_tokens: 130,
        total_tokens: 180,
        ..ModelUsage::default()
    });
    window.set_estimated_prefill(90);

    assert_eq!(
        window.snapshot(),
        AutoCompactWindowSnapshot {
            ordinal: 1,
            prefill_input_tokens: Some(120),
        }
    );
}

#[test]
fn start_next_advances_ordinal_and_clears_prefill() {
    let mut window = AutoCompactWindow::new();

    window.set_estimated_prefill(150);
    window.start_next();

    assert_eq!(
        window.snapshot(),
        AutoCompactWindowSnapshot {
            ordinal: 2,
            prefill_input_tokens: None,
        }
    );
}

#[test]
fn restores_snapshot_as_estimated_prefill() {
    let window = AutoCompactWindow::from_snapshot(AutoCompactWindowSnapshot {
        ordinal: 3,
        prefill_input_tokens: Some(42),
    });

    assert_eq!(
        window.snapshot(),
        AutoCompactWindowSnapshot {
            ordinal: 3,
            prefill_input_tokens: Some(42),
        }
    );
}
