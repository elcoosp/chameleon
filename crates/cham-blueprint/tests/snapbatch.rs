//! Direct unit tests for `SnapBatchSink` (PERF-PLAN T3 / V2 Phase 1.2.1).
//!
//! Closes the mutant gaps documented in
//! `docs/reports/mutants-traversal-20260926.md`: every method on
//! `SnapBatchSink` was a missed mutant before these tests. Each test
//! exercises one or more of the previously-undetected methods directly
//! through the public API and the `RegretSink` trait interface.

use cham_blueprint::table::{RegretTable, ThreadMode};
use cham_blueprint::traversal::{RegretSink, SnapBatchSink};

/// `with_discount` must STORE the discount — not fall back to the default
/// `SnapBatchSink::new()` value of 1.0. Kills the
/// `SnapBatchSink::with_discount -> Default::default()` mutant.
#[test]
fn with_discount_stores_discount() {
    let s = SnapBatchSink::with_discount(0.5);
    assert!(
        (s.regret_discount - 0.5).abs() < 1e-9,
        "with_discount(0.5) must store 0.5, got {}",
        s.regret_discount
    );
    let s = SnapBatchSink::with_discount(1.0);
    assert!(
        (s.regret_discount - 1.0).abs() < 1e-9,
        "with_discount(1.0) must store 1.0, got {}",
        s.regret_discount
    );
    // also check the default constructor path
    let s = SnapBatchSink::new();
    assert!((s.regret_discount - 1.0).abs() < 1e-9);
    assert_eq!(s.pending(), 0);
}

/// `pending` must reflect the number of buffered deltas across all four
/// lanes. Kills `pending -> 0` and `pending -> 1`.
#[test]
fn pending_tracks_buffer_length() {
    let mut s = SnapBatchSink::with_discount(1.0);
    assert_eq!(s.pending(), 0, "fresh sink has empty buffer");
    s.buf.push_regret(100, 0, 1.0);
    assert_eq!(s.pending(), 1);
    s.buf.push_regret(100, 1, 2.0);
    assert_eq!(s.pending(), 2);
    s.buf.push_strat(100, 3, 0, 0.5);
    assert_eq!(s.pending(), 3);
    s.buf.push_weight(100, 3, 0.25);
    assert_eq!(s.pending(), 4);
    s.buf.push_visit(100, 3);
    assert_eq!(s.pending(), 5);
}

/// The four `RegretSink` methods must actually write through to the table
/// on flush. Kills `add_regret with ()`, `add_strat with ()`,
/// `add_weight with ()`, `add_visit with ()`, and `flush with ()`.
#[test]
fn regret_sink_methods_write_through_flush() {
    let mut table = RegretTable::new(ThreadMode::Deterministic);
    let key: u64 = 0xABCD_1234_5678_9ABC | (1 << 63);
    let w = 3usize;
    let (off, _) = table.entry_or_insert(key, w);
    for a in 0..w {
        assert_eq!(table.regret(off, w, a), 0.0);
        assert_eq!(table.strat_sum(off, w, a), 0.0);
    }
    assert_eq!(table.avg_weight(off, w), 0.0);
    assert_eq!(table.visits(off, w), 0);

    let mut sink = SnapBatchSink::with_discount(1.0);
    sink.add_regret(&table, off, 0, 0.7);
    sink.add_regret(&table, off, 1, -0.3); // floored at 0 on flush
    sink.add_regret(&table, off, 2, 0.2);
    sink.add_strat(&table, off, w, 0, 0.25);
    sink.add_strat(&table, off, w, 1, 0.5);
    sink.add_strat(&table, off, w, 2, 0.25);
    sink.add_weight(&table, off, w, 0.4);
    sink.add_visit(&table, off, w);
    sink.add_visit(&table, off, w);

    // Buffer holds the deltas — the table is untouched.
    assert_eq!(table.regret(off, w, 0), 0.0);
    assert_eq!(table.strat_sum(off, w, 0), 0.0);
    assert_eq!(table.avg_weight(off, w), 0.0);
    assert_eq!(table.visits(off, w), 0);
    assert_eq!(sink.pending(), 9);

    sink.flush(&table);
    assert_eq!(sink.pending(), 0);
    assert!((table.regret(off, w, 0) - 0.7).abs() < 1e-6);
    assert_eq!(table.regret(off, w, 1), 0.0, "negative regret floored at 0");
    assert!((table.regret(off, w, 2) - 0.2).abs() < 1e-6);
    assert!((table.strat_sum(off, w, 0) - 0.25).abs() < 1e-6);
    assert!((table.strat_sum(off, w, 1) - 0.5).abs() < 1e-6);
    assert!((table.strat_sum(off, w, 2) - 0.25).abs() < 1e-6);
    assert!((table.avg_weight(off, w) - 0.4).abs() < 1e-6);
    assert_eq!(table.visits(off, w), 2);
}
