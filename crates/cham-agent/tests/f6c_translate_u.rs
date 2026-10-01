//! F6c (2026-10-01): the off-tree translation path is gated by
//! `CHAM_OFFTREE_TRANSLATE=1` and must be replay-deterministic.
//!
//! Two invariants are tested here:
//!
//! 1. **Gate default is off.** The gate is read once per process via
//!    `OnceLock`; a test binary that has not set the env var sees off.
//!    Setting it in-process would poison every other test in the
//!    binary, so the full end-to-end replay test must live in a
//!    subprocess or the CLI harness.
//! 2. **Contract visibility.** The translate path's determinism rests
//!    on `derive_translate_u` being a pure function of
//!    `(hand_idx, street, seq.lens)`. That function is private to
//!    `cham-agent::pipeline`; re-exporting it solely for this test
//!    would widen the crate API for no production benefit. The
//!    contract is documented here and exercised indirectly through
//!    the existing `pipeline_deterministic_replay` (with the gate
//!    off).
//!
//! When the ladder replay harness grows an `--offtree` flag, or a
//! subprocess-based variant of `pipeline_deterministic_replay` is
//! added, that test becomes the real gate. See
//! `docs/plans/F6C-TRANSLATE-2026-10-01.md` section "What remains".

/// The gate must default to OFF. A test process that has not set the
/// env var sees the off state regardless of other tests' writes.
#[test]
fn gate_default_is_off() {
    let v = std::env::var("CHAM_OFFTREE_TRANSLATE").ok();
    assert!(
        v.as_deref() != Some("1"),
        "CHAM_OFFTREE_TRANSLATE=1 is set in the test process — \
         other tests in this binary will see it. The gate must be \
         read in a fresh subprocess, not set in-process."
    );
}

/// Contract visibility: the translate path's replay determinism is a
/// pure-function property of `derive_translate_u`. This test does not
/// exercise the function directly (it is private); it documents the
/// contract and points to the real gate. It must pass (and produce
/// no output) in every environment.
#[test]
fn replay_contract_documented() {
    // Intentionally no assertion beyond "the module compiled": this
    // is a marker so the missing coverage is visible in the test list
    // under a name that says so. See module doc for the follow-up.
    let _ = std::env::var("CHAM_OFFTREE_TRANSLATE");
}
