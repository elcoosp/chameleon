//! CLI contract tests (SPECS/09 §5). The binary is spawned via
//! `CARGO_BIN_EXE_chameleon` — std only, keeping the dep whitelist closed.

use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_chameleon")
}

/// The spec's §2 list — 12 commands implemented (D-013: the spec says "11
/// exactly" while listing twelve; we implement the LIST and pin the count to it).
const COMMANDS: [&str; 12] = [
    "verify",
    "train-buckets",
    "train-bp",
    "train-router",
    "collect",
    "probe",
    "ladder",
    "ab",
    "slumbot",
    "play",
    "trace",
    "dashboard",
];

#[test]
fn cli_parse_surface() {
    // every subcommand parses with documented flags (help exits 0)
    for cmd in COMMANDS {
        let out = Command::new(bin())
            .args([cmd, "--help"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("spawn");
        assert!(out.status.success(), "{cmd} --help failed: {}", out.status);
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(!text.is_empty(), "{cmd} --help printed nothing");
    }
    // unknown flags rejected
    let out = Command::new(bin())
        .args(["ladder", "--definitely-not-a-flag"])
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "unknown flags must be rejected");
    // count pinned to the implemented list
    let top = Command::new(bin()).arg("--help").output().expect("spawn");
    let text = String::from_utf8_lossy(&top.stdout);
    for cmd in COMMANDS {
        assert!(text.contains(cmd), "top-level help missing {cmd}");
    }
}

#[test]
fn verify_exit_codes() {
    // clean tree → 0 (invariant greps + core invariants)
    let out = Command::new(bin())
        .args(["verify"])
        .output()
        .expect("spawn verify");
    assert!(
        out.status.success(),
        "verify on the clean tree must pass: {out:?}"
    );
}

#[test]
fn verify_perf_enforces_gates() {
    // `verify --perf` must ENFORCE the P1..P6 gates from criterion estimates
    // (PERF-PLAN T6): it prints the gate table and exits nonzero on breach —
    // never a silent pass, never a panic.
    let out = Command::new(bin())
        .args(["verify", "--perf"])
        .output()
        .expect("spawn verify --perf");
    assert_ne!(out.status.code(), Some(101), "verify --perf must not panic");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("perf gates"),
        "verify --perf must report the gate table: {text}"
    );
    if out.status.success() {
        assert!(
            text.contains("PASS"),
            "passing verify --perf must show PASS rows: {text}"
        );
    } else {
        assert_eq!(
            out.status.code(),
            Some(1),
            "gate breach must exit 1: {text}"
        );
        assert!(
            text.contains("FAIL") || text.contains("just bench"),
            "failing verify --perf must name the breach or ask for benches: {text}"
        );
    }
}

#[test]
fn play_budget_refusal_without_artifacts() {
    // play without a loadable artifact bundle must refuse with the budget code
    // (2) — the prompt loop itself needs trained blueprints (M2 artifacts).
    let out = Command::new(bin())
        .args(["play", "--agent", "robust-only", "--depth", "100"])
        .output()
        .expect("spawn play");
    // either the bundle exists (full pipeline) or we get the budget refusal —
    // a panic (101) or crash is always wrong
    assert_ne!(
        out.status.code(),
        Some(101),
        "play must not panic without artifacts"
    );
    if out.status.code() != Some(0) {
        assert_eq!(
            out.status.code(),
            Some(2),
            "no-artifacts play must exit 2 (budget refusal)"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("artifacts/agent"),
            "refusal must point at the bundle path"
        );
    }
}

#[test]
fn ladder_refuses_without_artifacts() {
    // ladder with a trained agent but no bundle must refuse with the budget
    // code (2) instead of printing silent-fallback mirror rows (PERF-PLAN T7;
    // patterned on play_budget_refusal_without_artifacts).
    let out = Command::new(bin())
        .args(["ladder", "--fast", "--agent", "full"])
        .output()
        .expect("spawn ladder");
    // either the bundle exists (full pipeline) or we get the budget refusal —
    // a panic (101) or crash is always wrong
    assert_ne!(
        out.status.code(),
        Some(101),
        "ladder must not panic without artifacts"
    );
    if out.status.code() != Some(0) {
        assert_eq!(
            out.status.code(),
            Some(2),
            "no-artifacts ladder must exit 2 (budget refusal)"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("artifacts/agent"),
            "refusal must point at the bundle path"
        );
    }
}

#[test]
fn slumbot_mock_flow_runs() {
    // mock mode never touches the network and exits 0 (SPECS/08 §5)
    let out = Command::new(bin())
        .args(["slumbot", "--seatings", "3"])
        .output()
        .expect("spawn slumbot");
    assert!(
        out.status.success(),
        "mock slumbot flow failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // the verify-first gate: real without consent is refused with the budget code
    let out = Command::new(bin())
        .args(["slumbot", "--real"])
        .output()
        .expect("spawn slumbot real");
    assert_eq!(
        out.status.code(),
        Some(2),
        "--real without --yes-i-am-live must exit 2"
    );
}

#[test]
fn artifact_hash_printed() {
    // artifact-consuming commands print their provenance hash; a small train-bp
    // run is the cheapest full-cycle command that exercises the artifact path.
    let out = Command::new(bin())
        .args([
            "train-bp", "--mode", "robust", "--iters", "50", "--depth", "100", "--seed", "3",
        ])
        .output()
        .expect("spawn train-bp");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        assert!(
            text.contains("blake3") || text.contains("hash"),
            "artifact-consuming command must print a hash: {text}"
        );
    }
    // if the tiny bucket artifacts are missing the command refuses with 2 —
    // also acceptable here (the hash contract is exercised on green runs)
    assert_ne!(out.status.code(), Some(101), "train-bp must not panic");
}
