# SPECS/12 — Crate `cham-rec` — v2 (NEW: v1 had no spec for this crate and mis-pointed its registry at SPECS/08 §7)

The flight recorder: a leaf crate (depends only on `serde`/`serde_json`) that owns ALL structured event writing in the workspace. No crate re-implements JSONL writing; no crate embeds recorder logic.

---

## 1. Module tree

```
crates/cham-rec/src/
├── lib.rs          (RecError, Recorder, facade)
├── schema.rs       (RecordKind enum — the registry; serde tagged enum "kind")
└── validate.rs     (offline validator used by `chameleon verify`)
```

## 2. `Recorder`

```rust
pub struct Recorder { /* run dir, buffer, bytes_written, seq */ }
impl Recorder {
    /// artifacts/runs/<run_id>/events.jsonl ; run_id = "<unix_secs>-<kind>-<blake3_8>"
    pub fn open(run_dir: &Path, kind: &str) -> Result<Recorder, RecError>;
    pub fn record(&mut self, kind: RecordKind, data: serde_json::Value) -> Result<(), RecError>;
    /// Flush + fsync every 1000 records or 2 s (callers poll `should_flush`); explicit flush on drop.
    /// Append-only: the file is opened with append; a corrupted tail STOPS the run (error up),
    /// never truncation. Loss of the buffered tail on hard crash is acceptable; silent mid-run
    /// loss is not (fsync on flush is mandatory).
    pub fn flush(&mut self) -> Result<(), RecError>;
    pub fn should_flush(&self) -> bool;
    pub fn seq(&self) -> u64;
}
```

Line format (every record):

```json
{"ts": 1690000000, "run": "1758-<kind>-<h8>", "kind": "<kind>", "seq": 42, "data": { ... }}
```

Rules: `ts` = unix seconds (the only wall-clock in the workspace besides search budget); `seq` monotonic per run; one JSON object per line; no NaN/Infinity in `data` (validator rejects; serde_json forbids by default).

## 3. Record-kind registry (the single source of truth; validator enforces payloads)

| Kind | Producer | Payload (required fields) |
|---|---|---|
| `opp_session` | cham-opponents via eval | spec_id, family ("A"\|"B"\|"PN"\|"noise"), arch, seed, params{} |
| `match` | cham-eval | label, spec ids, seeds, deals, seatings, mb_per_seating, se_mb, vr_factor, wall_s |
| `decision` | cham-agent | hand_idx, street, slot, action, weights_frozen[5], argmax_k?, search{solver,triggered,source,iters,truncated,lbr_gap_ours}?, expert_visits[4], fallback_used, abstraction_hash |
| `bp_snapshot` | cham-blueprint | iters, infosets, bytes, wall_s, thread_mode, threads |
| `bp_probe` | cham-blueprint | lbr_mb, coverage, iters |
| `warmstart_step` | cham-blueprint | src_artifact, depth_bb, keys_transferred |
| `router_train` | cham-router | rows, top1_b_dev, top1_b_test, ece_b_test, ece_family_c, per_class_recall[4], gates_passed |
| `search_decision` | cham-search | triggered, solver, iters, truncated, lbr_gap (ours, theirs) |
| `agent_load` | cham-agent | mode, artifact hashes (blake3 each), depth_bb, experts[5] provenance |
| `collect_rowset` | cham-eval | rows, sessions, family_counts{}, abstraction_hash |
| `ledger_entry` | cham-eval | type (ab\|ladder\|slumbot\|probe), a{}, b?, delta_mb?, ci?, sprt?, promote, hands/seatings, notes |
| `probe_summary` | cham-eval | lbr_mb, coverage, router acc/ece, verdict PASS/FAIL |
| `fallback_uniform` | cham-agent | hand_idx, street, reason |

Unknown kinds are a validator error (prevents quiet schema drift); all producers must extend this table in the same commit that adds a kind.

## 4. Tests (contractual)

| Test | Pins |
|---|---|
| `line_format_stable` | insta golden: one record of each kind serializes to the documented shape |
| `append_only_fsync` | simulated corruption mid-file → open/append errors, file length unchanged |
| `flush_cadence` | 2500 records → ≥ 2 flushes; drop() flushes the tail |
| `no_nan_payloads` | NaN injection → error |
| `validator_rejects_unknown_kind` | handwritten bogus kind → validate fails |
| `concurrent_runs_separate_files` | two Recorders, one process → distinct run dirs, no interleaving |

## 5. DoD

```
DoD — cham-rec
[ ] cargo nextest run -p cham-rec green (tests above, by name)
[ ] clippy + deny clean; deps ⊆ {serde, serde_json}
[ ] Validator wired into `chameleon verify` (validates all runs/*/events.jsonl present)
[ ] This file is the registry: grep-proof that no other doc lists record kinds
[ ] README present
```
