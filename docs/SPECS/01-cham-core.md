# SPECS/01 — Crate `cham-core` — v1

Foundation crate: cards, **fast** hand evaluation, HUNL rules engine (allocation-free hot paths), RNG, observables, agent trait with **leak-proof public histories**. Zero policy logic. Zero dependency on `cham-rec`.

---

## 1. Module tree

```
crates/cham-core/src/
├── lib.rs          (facade, CoreError, #![forbid(unsafe_code)])
├── consts.rs       (epsilons, invariant registry I1–I9)
├── card.rs         (Card, Hand2, Deck, canonicalization)
├── eval/
│   ├── mod.rs      (facade: evaluate7, evaluate5, equity, Range — implementation swappable)
│   ├── impl_holdem/  (binding to `holdem-hand-evaluator` if gate P1 passes on M1)
│   ├── impl_bitmask/ (fallback: in-crate bitmask/perfect-hash evaluator, ~400 LOC)
│   └── vectors/    (golden vectors JSON)
├── rng.rs          (Rng alias, child derivation, weighted pick)
├── engine/
│   ├── mod.rs      (State — Copy, fixed arrays; Action; apply; legal_actions → ArrayVec)
│   ├── config.rs   (EngineConfig)
│   ├── history.rs  (HandHistory [FULL INFO — internal] and PublicHistory [agent-visible])
│   └── fuzz.rs     (property harness, proptest)
├── obs.rs          (Observables<'a> — borrowed view; LegalAction; Agent trait)
└── error.rs
```

## 2. Cards & hands — `card.rs`

Same contract as v1 (`Card(u8)` idx/rank/suit, `parse`, `ALL_CARDS`), plus:

```rust
pub struct Hand2(pub u16);
impl Hand2 {
    // ... as v1 (new, cards, parse, class_id over 169 classes) ...
    /// Suit-isomorphic canonical form (Waugh-style): maps the hand to the representative of its
    /// 4-suit orbit. This canonical form is the INDEX into the precomputed abstraction tables
    /// (SPECS/02 §3): flop/turn lookup requires canonical (hand, board) joint orbits, so the
    /// canonicalization of board+hand together is provided by cham-engine, not here.
    pub fn canonical(self) -> Hand2;
    pub fn class_id(self) -> u8;                 // 0..=168, pinned ordering
}
```

## 3. Hand evaluator — `eval/` (v1: fast, swappable, honestly gated)

**Gate P1: ≥ 100M `evaluate7`/s release, M1.** v1's 21×`evaluate5`-max at 1M/s is rejected — it would bless a design that cripples MCCFR downstream.

- Primary: bind `holdem-hand-evaluator` (pure Rust, perfect-hash) behind `cham_core::eval::evaluate7`. Adopt it **only** if the criterion bench passes P1 on M1.
- Fallback (if not): in-crate bitmask evaluator — 7 cards → two rank-multiset lookups + flush/straight fixups via precomputed u64 bit tricks (Dagli-style), ~400 LOC, property-tested against a naive `evaluate5`-max reference kept in `tests/` only.
- Public API (identical either way):

```rust
pub fn evaluate7(c: &[Card; 7]) -> u16;                  // 1..=7462, fixed ordering
pub fn best5(c: &[Card; 7]) -> ([Card; 5], u16);
/// Exact equity of hero vs a Range on a given board: enumeration over villain combos (no MC).
/// O(1326) evaluate7 calls ≈ 10 µs at P1 — this is the river-encode workhorse (SPECS/02 §5b).
pub fn equity_exact(hero: Hand2, villain: &Range, board: &[Card]) -> (f64, f64); // (win, tie)
/// MC equity — EXISTS ONLY FOR OFFLINE TABLE BUILDING (SPECS/02 §3) and nothing else.
pub fn equity_mc(hero: Hand2, villain: &Range, board: &[Card], iters: u32, rng: &mut Rng) -> (f64, f64);
pub struct Range(pub [u64; 21]);                          // 1326 bits = 21×u64. v1's [u64;3] was 192 bits — wrong.
impl Range { pub fn set(&mut self, combo: usize, on: bool); pub fn get(&self, combo: usize) -> bool;
             pub fn count(&self) -> u32; pub fn top(n: f64) -> Range; pub fn from_percent(p: f64) -> Range;
             pub fn iter(&self) -> impl Iterator<Item = usize>; pub fn remove_cards(&mut self, dead: &[Card]); }
```

`equity_mc` is grep-listed: only `cham-engine`'s offline table builder and `cham-opponents`' chart builder may call it.

## 4. RNG — `rng.rs`

Unchanged: `Rng = ChaCha8Rng`, `rng_from_seed`, `child(seed, label)`, `pick`, `weighted`. Thread model: per-worker `(base_seed, worker_id)` derivation — results independent of thread count **in Deterministic mode** (single worker); Hogwild training is interleaving-dependent by design (00 §3.5).

## 5. Rules engine — `engine/` (v1: allocation-free)

```rust
#[derive(Clone, Copy)] pub struct State { /* fixed arrays ONLY — no Vec, no String */ }
impl State {
    pub fn new(cfg: EngineConfig, deck: Deck) -> State;
    // Accessors as v1: street, to_act, stacks, pot, current_bet, min_raise_to, max_raise_to,
    // board (→ &[Card; 5] + len), hole, is_terminal, payoffs, is_all_in_runout
    /// Legal actions into a caller-provided ArrayVec (NO allocation): cap 12 slots.
    pub fn legal_actions(&self, out: &mut ArrayVec<LegalAction, 12>);
    pub fn apply(&mut self, a: Action) -> Result<ApplyOutcome, CoreError>;
}
pub enum Action { Fold, Check, Call, Bet { to: i64 }, Raise { to: i64 } }   // canonical; AllIn = Bet/Raise to cap (00 §4)
```

Semantics unchanged from v1 and each still has its named test: HU position order, legal ordering, min-raise progression, short all-in no-reopen, split odd chip to BB, uncalled bet return. `EngineConfig { start_stack, sb, bb }` — the v1 comment garbage (`start_stack: i64, // chips (100_000 = 1000bb? NO: ...)`) is gone; validate: `sb*2 == bb`, `20bb ≤ start_stack ≤ 1000bb`.

**Perf gate P2: ≥ 10M apply-actions/s** (v1's 500k was 20× too low and hid Vec churn). `State` is `Copy` (≤ 128 bytes), dealt board in a fixed `[Card; 5]`, `acted_mask: u8`, no heap anywhere in `apply`/`legal_actions`/accessors.

`history.rs` — **the leak fix (review A5):**

```rust
/// FULL information. For engine internals, eval bookkeeping, duplicate matching, replay tooling.
/// NEVER passed to an Agent.
pub struct HandHistory { pub seed: u64, pub actions: Vec<(Street, Player, Action)>, pub cfg: EngineConfig,
                         pub holes: [Hand2; 2], pub board: [Card; 5], pub result_sb: i64 }
impl HandHistory { pub fn replay(&self) -> Result<State, CoreError>; pub fn hash(&self) -> u64; }

/// What an agent may see after a hand: actions, showdown-revealed cards ONLY (both holes iff
/// showdown reached; folded holes remain hidden), net results. No seed, no replay().
pub struct PublicHistory { pub actions: Vec<(Street, Player, Action)>, pub board: [Card; 5],
                           pub showdown_holes: [Option<Hand2>; 2], pub nets: [i64; 2] }
impl PublicHistory { pub fn from(hh: &HandHistory) -> PublicHistory; }   // the ONLY constructor
```

`fuzz.rs`: proptest harness as v1, invariants I2–I7, 1M-hand release tier, plus invariant **I9**: serializing every `PublicHistory` produced from 10k fuzzed hands and grepping for hole cards of folded players must find zero (the leak test now covers the new type).

## 6. Observables & Agent trait — `obs.rs` (v1: borrowed views, no leak surface)

```rust
/// Borrowed view over a State for one seat — no ownership, no hidden info, no Vec.
pub struct Observables<'a> { /* fields as v1 but referencing 'a State internals; legal actions
                               handed out as ArrayVec by the engine; helper fns pot_bb, spr,
                               pot_odds, effective_stack_bb as v1 */ }
impl<'a> Observables<'a> { pub fn view(state: &'a State, p: Player) -> Observables<'a>; }

pub trait Agent: Send {
    fn name(&self) -> &str;
    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action;
    /// Probability oracle for TRAINING (opponent reach). Analytic by construction (SPECS/03 §4).
    /// Returns (action, p) pairs covering the agent's full intended distribution at this decision.
    /// Default: Err(NotProbabilistic) — only archetype scripts implement it.
    fn action_probs(&self, obs: &Observables<'_>) -> Result<ArrayVec<(Action, f64), 12>, AgentError> { Err(NotProbabilistic) }
    /// Public information only (v1 leak fix). Default no-op.
    fn on_hand_end(&mut self, _ph: &PublicHistory, _hero_net: i64) {}
}
```

`Observables::view` is the only constructor; a test asserts the villain's hole cards are unreachable through any `pub` API of `Observables` (serialization + type-system argument documented in the test).

## 7. Tests (contractual; v1 deltas bolded)

| Test | Pins |
|---|---|
| `card_parse_roundtrip`, `hand2_canonical_169`, `eval_golden_50`, `eval_flush_wheel_edges` | as v1 |
| **`eval_bitmask_vs_naive`** | 200k seeded hands: evaluator == naive 21×evaluate5 max (proptest) |
| **`range_bits_roundtrip`** | all 1326 combos set/clear/count over `[u64; 21]`; `remove_cards` correctness |
| `rng_child_determinism`, `hu_position_order`, `legal_order_pinned`, `short_allin_no_reopen`, `min_raise_progression`, `split_odd_chip`, `uncalled_return` | as v1 |
| **`state_is_copy_no_heap`** | `size_of::<State>()` ≤ 128; `legal_actions` into stack ArrayVec (compile-time + bench) |
| **`public_history_leak_proof`** | I9: folded-hole secrecy over 10k fuzzed hands; `PublicHistory` has no `seed` field |
| **`action_probs_exclusive`** | ArchetypeAgents implement `action_probs`; CallBot/RandomBot return `NotProbabilistic` |
| `fuzz_1m_release` (criterion gate) | I2–I7, P2 |
| **`eval_perf_100m`** (criterion) | P1 |

## 8. DoD

```
DoD — cham-core
[ ] cargo nextest run -p cham-core green (tests above, by name)
[ ] clippy clean; cargo deny clean; deps ⊆ {rand, rand_chacha, serde, arrayvec, holdem-hand-evaluator?}
    (+ proptest, insta, tempfile dev)
[ ] P1 ≥ 100M evals/s, P2 ≥ 10M actions/s (criterion benches committed under benches/)
[ ] Zero unsafe; zero unwrap outside tests; no Vec in engine hot paths
[ ] README present
```
