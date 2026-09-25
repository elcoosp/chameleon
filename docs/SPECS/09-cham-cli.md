# SPECS/09 — Crate `cham-cli` (bin: `chameleon`) — v1

Orchestration only. v1 fixes the review's hygiene list: subcommand count is exact (11), the justfile never calls a bare binary, `collect` exists (router datasets have an owner), `probe` has an owner (cham-eval orchestrates, cham-blueprint/cham-router compute), `replay` animation is cut, and depth flags are consistent with the depth-flexible loader.

---

## 1. Module tree

```
crates/cham-cli/src/
├── main.rs        (clap dispatch, anyhow contexts, exit codes)
├── cmd/
│   ├── verify.rs  ├── train_buckets.rs ├── train_bp.rs   ├── train_router.rs
│   ├── collect.rs ├── probe.rs         ├── ladder.rs     ├── ab.rs
│   ├── slumbot.rs ├── play.rs          ├── trace.rs      └── dashboard.rs
└── prompt.rs      (terminal I/O for play)
```

## 2. Subcommands — 11 exactly

```
chameleon verify [--perf] [--count-infosets] [--proofs]
    Tier 0: workspace invariant greps (00 §11), core fuzz (quick tier), golden vectors,
    cham-proofs suite (with --proofs: Kuhn/Leduc/Bayes/LP — SPECS/11 M-1), infoset estimates,
    perf gates via the criterion bench binary. Exit: 0 green, 1 failure, 2 budget refusal.

chameleon train-buckets                       # OFFLINE: flop/turn iso tables + k-means (SPECS/02 §3)
chameleon train-bp --mode exploit --opponent jitter:tag --seed 7 [--depth 100] [--iters N]
                  | --mode robust | --mode exploit-bayes | --status <run_dir>
chameleon train-router --rows artifacts/router_rows.rbin --out artifacts/routers/<hash>/
chameleon collect --out artifacts/router_rows.rbin [--max-rows 2000000]
    # Builds the binary router dataset from instrumented ladder sessions (SPECS/05 §4);
    # one row per hand, session-clustered splits, out-of-family rows labeled and excluded from A/B-dev.

chameleon probe [--agent <mode>]      # Tier 1; OWNED BY cham-eval::probe, computed by
                                      # cham-blueprint::lbr (BR proxy, G9) + cham-router metrics +
                                      # coverage from expert visit counters. Verdict line format:
                                      # "probe: PASS (lbr 87 mb/hand, cov 0.91, acc_b_dev 0.84)"

chameleon ladder [--fast | --full] [--agent <mode>] [--pool config/pool.toml] [--clusters 1]
chameleon ab --a full --b baseline --deals 25000 [--clusters 3] [--margin 0] [--sprt] [--promote]
chameleon slumbot --seatings 20000 [--real --yes-i-am-live] [--resume <session.jsonl>]
chameleon play [--agent <mode>] [--depth 100]
chameleon trace --run <run_id> [--top 20 --by fallback|search|lbr]    # textual; no animation (cut)
chameleon dashboard [--out artifacts/reports/index.html] [--last 50]
```

Count: **11**. (v1 claimed 12 with 11 listed and shipped a `replay` animation nobody asked for — both fixed.)

## 3. UX rules

Unchanged: live one-line progress, ledger entry ID printed, `--help` cites the governing spec section, exit codes 0/1/2/130 with clean snapshots on ctrl-C for train commands. New: every command that consumes artifacts prints their `artifact_hash` (blake3) so humans can cross-check the ledger.

## 4. `justfile` (root; **no bare `chameleon` invocations** — v1 bug)

```
CHAM := cargo run -q -p cham-cli --

test:      cargo nextest run --workspace
bench:     cargo bench --workspace
verify:    just test && just bench && $(CHAM) verify --perf --count-infosets --proofs
fast:      just verify && $(CHAM) probe && $(CHAM) ladder --fast && $(CHAM) dashboard
nightly:   $(CHAM) ladder --full && $(CHAM) slumbot --seatings 20000 && $(CHAM) dashboard
mutants:   cargo mutants -p cham-core -p cham-blueprint --in-diff-against HEAD
deny:      cargo deny check
```

Dev tools wired here: `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-deny` (install documented in README).

## 5. Tests (contractual)

| Test | Pins |
|---|---|
| `cli_parse_surface` | all 11 subcommands parse with documented flags; unknown flags rejected; count asserted == 11 |
| `verify_exit_codes` | tampered fixture → exit 1 with gate ID |
| `play_prompt_roundtrip` | scripted stdin drives a full hand; illegal input re-prompts |
| `ctrlc_snapshots` | simulated SIGINT during train-bp leaves a resumable snapshot |
| `artifact_hash_printed` | every artifact-consuming command prints blake3 hashes |

## 6. DoD

```
DoD — cham-cli
[ ] cargo nextest run -p cham-cli green; clippy clean; deny clean
[ ] Deps ⊆ {clap, anyhow, serde, serde_json}
[ ] 11 subcommands, no business logic here (grep: no CFR/tracker/stats code)
[ ] justfile uses $(CHAM) everywhere; just fast green end-to-end after M3
[ ] README present
```
