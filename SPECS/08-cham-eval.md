# SPECS/08 — Crate `cham-eval` — v2

Everything that produces a number with a defensible interval. v2 fixes the statistics and compute-budget inconsistencies (review C): correct duplicate formula, session-clustered CIs, SPRT early stopping, Holm correction, AIVAT-style variance reduction, honest wall-clock budgets, per-opponent σ calibration, the real Slumbot dialect with a verify-first gate, and **Glicko ELO is cut** (replaced by the exploitation-vs-exploitability frontier). House rule unchanged: every promotable number carries a CI; everything else is labeled diagnostic.

---

## 1. Module tree

```
crates/cham-eval/src/
├── lib.rs          (EvalError, facade)
├── matcheng.rs     (MatchRunner: duplicate deals, threading, identical opponent streams)
├── stats.rs        (mean/SE/bootstrap/Welch/SPRT/session-cluster/paired — no deps)
├── vr.rs           (variance reduction: all-in-EV split + known-opponent AIVAT-style baseline)
├── ceiling.rs      (exploitation ceilings via cham-blueprint::lbr; frontier data)
├── slumbot.rs      (client per the PUBLISHED dialect; mock server; verify-first gate)
├── ab.rs           (A/B runner, SPRT, verdict rules, promotion)
├── ledger.rs       (append-only ledger.jsonl; baseline pointer)
├── ingest.rs       (events.jsonl aggregates)
└── dashboard.rs    (trimmed: ledger + winrate table + frontier chart; static HTML + inline SVG)
```

## 2. `matcheng.rs` — duplicate-deck engine (formula fixed)

```rust
pub struct MatchSpec { pub hero: Box<dyn Agent>, pub opponent: OpponentSpec,
                       pub deals: u64 /* DEALS — see definition */, pub depth_bb: i64,
                       pub base_seed: u64, pub label: String }
pub struct MatchResult { pub mb_per_seating: f64, pub se_mb: f64, pub seatings: u64,
                         pub vr_factor: f64, pub per_deal_profits: Option<Vec<f64>> }
impl MatchRunner {
    /// Duplicate (AIV): deal d is played TWICE with seats swapped. The per-deal profit is
    ///   profit(d) = (net_hero_seatA(d) + net_hero_seatB(d)) / 2
    /// — the SUM of the two seatings' nets divided by two (v1 wrote "(net1 − net2)/2": garbled —
    /// in duplicate play the hero takes both sides of the same deal, so seat advantages CANCEL by
    /// adding). σ is computed over per-deal profits AND clustered by session (§3).
    /// Opponent policy stream per deal: child(base_seed, &format!("d{d}")) — identical across hero
    /// configs, which is what makes A/B paired.
    pub fn run(&self, spec: &MatchSpec, rec: &mut Recorder) -> Result<MatchResult, EvalError>;
    pub fn run_pool(&self, hero_factory: &dyn Fn() -> Box<dyn Agent>, pool: &[OpponentSpec],
                    deals_per_opp: u64, base_seed: u64, rec: &mut Recorder) -> Result<PoolResult, EvalError>;
}
```

**"Hands" definition (normative, 00 §4):** a *deal* = one shuffle; a *seating* = one seat-side play of a deal; all sample sizes in this spec and the protocol are **seatings**; duplicate matches report per-deal profits (2 seatings per deal). Illegal action aborts the match (never skip). Gate P6 ≥ 60k seatings/min aggregate, recalibrated at M1.

## 3. `stats.rs` — session-clustered inference (review C1)

```rust
pub fn mean/se(v: &[f64]) -> f64;
pub fn bootstrap_ci(v, conf, resamples, rng) -> (f64, f64);            // resample DEALS
pub fn session_cluster_ci(per_deal: &[f64], session_of_deal: &[u32], conf, rng) -> (f64, f64);
    // CLUSTER bootstrap over SESSIONS (the opponent draw is per session — sessions are the
    // independent units; hand/deal-level bootstrap understates uncertainty for absolute numbers).
    // Used for absolute winrates. Paired A/B diffs cancel session effects → deal-level paired CI.
pub fn paired_ci(diffs: &[f64], conf, rng) -> (f64, f64);
pub fn welch_t(a, b) -> (f64, f64);
pub fn sprrt(diffs: &[f64], delta0: f64, delta1: f64, alpha: f64, beta: f64) -> SprrtState;
    // Wald SPRT over accumulating paired diffs: H0: Δ ≤ delta0 vs H1: Δ ≥ delta1
    // (defaults 0 vs +25 mb/seating, α = 0.05, β = 0.10) → Continue | AcceptH0 | AcceptH1.
    // Screening runs stop early on a boundary — clear losers die cheap (review C2).
pub fn holm(pvals: &[f64], alpha: f64) -> Vec<bool>;                   // familywise correction
pub fn required_seatings(sigma_pair: f64, delta_mb: f64, conf: f64) -> u64;
```

σ calibration: per opponent, from the M1 pilot (SPECS/11), committed to `config/pool.toml` as `sigma_pair[opp]`. Budget table (SPECS/10 §3) is computed from these — never asserted from vibes.

## 4. `vr.rs` — variance reduction beyond duplicate (review C4)

1. **All-in-EV adjustment** (always on): replace all-in runout outcomes by showdown-equity EV (both seatings), standard all-in-equity variance split; factor recorded.
2. **Known-opponent baseline (AIVAT-style)** vs scripted opponents only: the opponent's policy is known (`action_probs`), so a per-infoset baseline value `b(z) = E[π_opp · u]` is computable by a sampling pre-pass and applied as the control variate; **validated** on fixed data: `vr_factor` (variance ratio vs duplicate-only) reported per match; gate `aivat_variance_reduction` ≥ 1.5× on the pilot suite or the baseline auto-disables (records why).
3. Duplicate always on. Combined typical target 2–4× (documented as target, not gate).

## 5. `slumbot.rs` — the real dialect, verify-first (review A10)

v1's `/api/init` + `/api/bet/<token>` + `"r"<size>` was invented. v2 pins the **published public-client dialect** and refuses to ship until verified:

- Endpoints: `POST /api/login`, `POST /api/new_hand`, `POST /api/act`; actions encoded as `k` (check/call — per their convention), `c`, `f`, `b<amount>`; responses carry game state and `winnings` on completed hands.
- **Gate `slumbot_dialect_verified`:** before any long run, the implementer fetches Slumbot's published sample client and runs a 50-hand `--real --yes-i-am-live` session; the observed request/response shapes are diffed against `mock/` and any mismatch updates BOTH mock and client in one commit. The mock exists so tests never touch the network; the mock is only as good as that one verification — recorded in `decisions.jsonl`.
- Client rules: serial requests, ≥ 1 s spacing, ×3 exponential backoff on 5xx/network, session persisted after EVERY action (resumable), errored hands counted and excluded (never silently dropped).
- **Honest precision (review C3):** Slumbot σ ≥ 10 bb/seating at 200 bb and duplicate does not apply — 5k seatings ≈ ±280 mb/seating. Weekly anchor = 20k seatings ≈ ±140 mb. **Diagnostic only** (G8); it can never gate a promotion.

## 6. `ab.rs` — A/B with SPRT + Holm (review C2, C3)

```rust
pub struct AbSpec { pub a: AgentMode, pub b: AgentMode, pub pool: Vec<OpponentSpec>,
                    pub deals_per_opp: u64, pub seeds: Vec<u64>, pub conf: f64,
                    pub margin_mb: f64, pub sprt: Option<SprtParams> }
pub struct AbVerdict { pub delta_mb: f64, pub ci: (f64, f64), pub per_opp: Vec<PerOppDelta>,
                       pub sprt: Option<SprrtState>, pub promote: bool, pub rule: String }
```

- Paired diffs per deal (identical opponent streams); verdict rule: `promote ⟺ paired CI lower > margin_mb` **AND** the gate family's Holm correction passes (SPECS/10 §4: one preregistered primary endpoint; the rest Holm-corrected).
- SPRT optional for screening arms (early stop on boundaries; the stopped run is labeled as such in the ledger).
- `--promote` writes the ledger entry and atomically updates `config/baseline.toml`. Nothing else writes it.
- Per-opponent deltas with their own CIs — a global win hiding a losing archetype surfaces here.

## 7. `ledger.rs` — as v1 (append-only, corruption = stop), one schema edit: `family` field on pool entries and `vr_factor` + `sprt` fields on verdicts.

## 8. `ceiling.rs` + `dashboard.rs` — frontier replaces ELO (review C7, F5)

- **Exploitation ceiling** per point script: `cham-blueprint::lbr` long run vs that script at the training abstraction = ceiling EV. **Exploitation efficiency** = achieved winrate / ceiling. **G1 gates on efficiency ≥ 0.70** (v1's "+250 bb/100" was a guess).
- **Frontier plot:** x = LBR vs our robust blueprint (exploitability), y = winrate vs pool — the standard exploitation-vs-exploitability picture, replacing Glicko ELO (cut; a fixed pool makes ELO theater).
- Dashboard (trimmed per F5): (1) headline card + baseline verdicts; (2) per-opponent winrate table with session-clustered CIs; (3) frontier chart; (4) ledger table. Router-health/search-health/training-curve sections move to `chameleon trace` textual output and the ledger — v1's six-section dashboard was scope creep.

## 9. Compute-budget honesty (review C — the numbers v1 got wrong)

At P6 = 60k seatings/min (recalibrate at M1), one thread-hour ≈ 3.6M seatings:

| Tier | Scope | Seatings | Wall (est.) |
|---|---|---|---|
| smoke | 4 archetypes × 2.5k deals ×2 | 20k | ≤ 1 min |
| screening (`ladder --fast`) | 4 archetypes + 2 baselines × 10k deals ×2, 1 session cluster | 160k | ≤ 30 min (P7) |
| promotion (`ab`) | 2 arms × (4 archetypes × 25k deals ×2) | 800k | ≤ 30–60 min |
| headline (G1/G2) | 100k seatings/opp × 4 opps, 3 session clusters | 1.2M+ | 2–4 h |

v1's "~96 min for ladder --fast at 15k/min" was the honest number for v1's too-slow P6; v2 fixes P6 itself (fast engine, §2) and the tier table above is derived from it. Slumbot: ~10 h wall at 20k seatings (rate-limited), run overnight.

## 10. Tests (contractual)

| Test | Pins |
|---|---|
| **`duplicate_profit_formula`** | hand-built symmetric scenario: identical hero both seats → profit(d) = 0 exactly; asymmetric scripted case → hand-computed (netA+netB)/2 |
| `identical_streams_ab` | identical hero configs → every paired diff exactly 0 |
| **`session_cluster_ci_covers`** | synthetic clustered data with between-session variance: deal-level CI undercovers (< 90%), cluster CI covers ≥ 94% over 200 resamples |
| **`sprrt_boundaries`** | synthetic drift streams hit AcceptH1/AcceptH0/Continue at hand-computed likelihood ratios |
| **`holm_correction`** | known p-value vectors → expected rejections |
| `stats_golden`, `required_seatings_formula` | as v1 (per-opponent σ input) |
| **`aivat_variance_reduction`** | scripted-opponent pilot: vr_factor ≥ 1.5 else auto-disable recorded |
| `slumbot_mock_flow`, `slumbot_rate_limit_retry` | as v1, real dialect |
| **`slumbot_dialect_verified`** | fixture: recorded 50-hand real session shapes match mock byte-wise (the verify-first gate, manual trigger committed) |
| `ab_verdict_rule`, `baseline_promotion_atomic`, `ledger_append_only` | as v1 + Holm/SPRT fields |
| `dashboard_renders_from_fixtures` | 4 sections present (trimmed set), frontier SVG valid |
| `match_throughput` (criterion) | P6 |

## 11. DoD

```
DoD — cham-eval
[ ] cargo nextest run -p cham-eval green (tests above, by name)
[ ] clippy + cargo deny clean; deps ⊆ {serde, serde_json, ureq, rand} (+ dev)
[ ] Slumbot dialect verified once against the real API and committed as a fixture
[ ] No ELO code (grep); frontier + ceiling wired to cham-blueprint::lbr
[ ] Tier budget table recomputed from measured P6/σ and committed to config/pool.toml
[ ] P6 pass; ladder --fast ≤ 30 min end-to-end (P7, measured once); README present
```
