//! `FrozenAgent` (v3 §6, M6 Self-Exploit Audit): a frozen `full`-agent snapshot
//! as an analytic opponent, same shape as `ArchetypeAgent::action_probs`.
//!
//! The snapshot's average strategy is MATERIALIZED as infoset-key →
//! distribution rows (`FrozenRows`, exported from a `BlueprintPolicy` by the
//! `self-exploit` command — this crate never touches artifacts, keeping the
//! crate DAG acyclic: opponents → engine → core). The agent owns its `Encoder`
//! (interior mutability: `key()` takes `&mut`) and maintains the canonical
//! `ActionSeq` across the hand via the exact `ChameleonAgent` hook contract:
//! own actions recorded in `act()`, everyone else's in `on_public_action`
//! (skipping our own feed), seq reset in `on_hand_end`. Seq recording is
//! view-independent (street + public stack fractions + action class), so any
//! seat's feed produces the identical canonical history.
//!
//! `action_probs` is the analytic oracle trainers sample from: the EXACT
//! distribution `act` draws from (same vector, no re-sampling noise), which
//! is what makes the static self-exploit number honest. Uncovered infosets
//! (snapshot miss) fall back to uniform — a documented diagnostic Tat: a high
//! miss rate means the snapshot's abstraction mismatches the live encoder
//! (see `misses()`), not that the victim is weak.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::{AgentError, Observables, Player};
use cham_core::rng::{Rng, next_f64};
use cham_engine::encoder::{ActionSeq, Encoder};

/// Materialized frozen policy: infoset key (`enc.key(obs, seq).0`) →
/// normalized distribution over the infoset's slots (positional, same order
/// as `enc.slots(obs, seq)`).
#[derive(Clone, Debug, Default)]
pub struct FrozenRows(pub BTreeMap<u64, Vec<f64>>);

pub struct FrozenAgent {
    label: String,
    encoder: RefCell<Encoder>,
    rows: FrozenRows,
    seq: ActionSeq,
    queries: Cell<u64>,
    misses: Cell<u64>,
}

// `Agent: Send` — the encoder travels across threads inside `thread::scope`
// (one agent per thread). Encoder is Send (Mmap + maps); RefCell preserves it.
fn _assert_send() {
    fn assert_send<T: Send>() {}
    assert_send::<FrozenAgent>();
}

impl FrozenAgent {
    pub fn new(label: String, encoder: Encoder, rows: FrozenRows) -> FrozenAgent {
        FrozenAgent {
            label,
            encoder: RefCell::new(encoder),
            rows,
            seq: ActionSeq::default(),
            queries: Cell::new(0),
            misses: Cell::new(0),
        }
    }

    /// Sync the current-path action history (v3 §6, M6): the trainer calls
    /// this before each `action_probs` query so the frozen keys match the
    /// victim's live keys exactly. Matches drive the equivalent state via
    /// `act`/`on_public_action` instead — both paths maintain the same
    /// canonical history.
    pub fn set_seq(&mut self, seq: ActionSeq) {
        self.seq = seq;
    }

    /// Miss rate since session start (diagnostic: abstraction mismatch alarm).
    pub fn miss_rate(&self) -> f64 {
        let q = self.queries.get();
        if q == 0 {
            0.0
        } else {
            self.misses.get() as f64 / q as f64
        }
    }

    fn dist(&self, obs: &Observables<'_>) -> ArrayVec<(Action, f64), 12> {
        self.queries.set(self.queries.get() + 1);
        let mut enc = self.encoder.borrow_mut();
        let slots = enc.slots(obs, &self.seq);
        let key = enc.key_for(obs, &self.seq, &slots).0;
        let mut out: ArrayVec<(Action, f64), 12> = ArrayVec::new();
        match self.rows.0.get(&key) {
            Some(probs) => {
                for (i, s) in slots.iter().enumerate() {
                    out.push((s.action, probs.get(i).copied().unwrap_or(0.0)));
                }
            }
            None => {
                // snapshot miss → uniform (diagnostic fallback, counted)
                self.misses.set(self.misses.get() + 1);
                let n = slots.len().max(1) as f64;
                for s in slots.iter() {
                    out.push((s.action, 1.0 / n));
                }
            }
        }
        out
    }

    fn sample(dist: &ArrayVec<(Action, f64), 12>, rng: &mut Rng) -> Action {
        let u = next_f64(rng);
        let mut acc = 0.0;
        for (a, p) in dist {
            acc += p;
            if u <= acc {
                return *a;
            }
        }
        dist[dist.len() - 1].0
    }
}

impl cham_core::obs::Agent for FrozenAgent {
    fn name(&self) -> &str {
        // borrow a stable str: leak-free via label storage is impossible here
        // (&str must outlive self) — Box::leak would grow per call. Instead
        // return a 'static descriptor; the label rides in Debug, not name().
        "frozen"
    }

    fn act(&mut self, obs: &Observables<'_>, rng: &mut Rng) -> Action {
        let dist = self.dist(obs);
        let a = Self::sample(&dist, rng);
        // own action: record (mirrors ChameleonAgent::act)
        let player = obs.player;
        self.encoder.borrow().record(obs, player, a, &mut self.seq);
        a
    }

    fn action_probs(
        &self,
        obs: &Observables<'_>,
    ) -> Result<ArrayVec<(Action, f64), 12>, AgentError> {
        Ok(self.dist(obs))
    }

    fn on_public_action(&mut self, obs: &Observables<'_>, player: Player, action: Action) {
        // our own actions are recorded in act(); skip re-feeding them here
        // (identical contract to ChameleonAgent::on_public_action).
        if player == obs.player {
            return;
        }
        self.encoder
            .borrow()
            .record(obs, player, action, &mut self.seq);
    }

    fn on_hand_end(&mut self, _ph: &cham_core::engine::PublicHistory, _hero_net: i64) {
        self.seq = ActionSeq::default();
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl std::fmt::Debug for FrozenAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrozenAgent")
            .field("label", &self.label)
            .field("rows", &self.rows.0.len())
            .field("miss_rate", &self.miss_rate())
            .finish()
    }
}

// ---- EXP-016 shadow registry (v5-deepdive-audit item 5) ----
//
// Lets the EXP-016 shadow gauntlet (`cham-cli shadow gauntlet`, and the
// `--promote` gate in `cmd::ab`) resolve `frozen:<label>` specs to REAL
// snapshot rows at match time. The factory crate never touches artifacts
// (DAG: opponents → engine → core), so the CLI registers rows it loaded
// from `artifacts/shadow/` here before running matches; `build()` consults
// the registry and falls back to the empty diagnostic when absent.
//
// The encoder is rebuilt per `build()` call from the registered
// buckets+config paths (mmap-backed, ~µs) because `Encoder` is neither
// Clone nor cheap to share across the per-opponent/per-thread agents that
// `MatchRunner` constructs. Keys only match when the registered paths are
// the snapshot champion's own abstraction (same buckets dir + config the
// rows were exported under) — a mismatch surfaces as a high miss rate,
// not silent wrongness (see `dist()`).
#[derive(Clone, Debug)]
pub struct ShadowEntry {
    pub rows: FrozenRows,
    pub buckets_dir: String,
    pub config_path: String,
}

fn shadow_registry() -> &'static std::sync::Mutex<std::collections::HashMap<String, ShadowEntry>> {
    static REGISTRY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, ShadowEntry>>,
    > = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Register snapshot rows for `frozen:<label>` resolution. Overwrites any
/// prior entry under the same label (snapshots are content-addressed, so a
/// re-registration under an existing label carries identical rows).
pub fn register_shadow(label: &str, rows: FrozenRows, buckets_dir: &str, config_path: &str) {
    if let Ok(mut reg) = shadow_registry().lock() {
        reg.insert(
            label.to_string(),
            ShadowEntry {
                rows,
                buckets_dir: buckets_dir.to_string(),
                config_path: config_path.to_string(),
            },
        );
    }
}

/// Look up a registered shadow (cloned; snapshots are small enough that one
/// clone per match build is negligible next to the match itself).
pub fn registered_shadow(label: &str) -> Option<ShadowEntry> {
    shadow_registry()
        .lock()
        .ok()
        .and_then(|reg| reg.get(label).cloned())
}

/// Build a `FrozenAgent` from a registry entry: encoder from the snapshot's
/// own buckets+config, falling back to the tiny diagnostic encoder when the
/// artifacts are unreadable (miss rate will then expose the mismatch).
pub fn build_registered(label: String, entry: &ShadowEntry) -> FrozenAgent {
    let cfg = std::fs::read_to_string(&entry.config_path)
        .ok()
        .and_then(|t| cham_engine::config::parse_config(&t).ok())
        .unwrap_or_else(cham_engine::config::AbstractionConfig::tiny);
    let encoder =
        cham_engine::Encoder::from_artifacts_dir(std::path::Path::new(&entry.buckets_dir), cfg.clone())
            .or_else(|_| cham_engine::Encoder::cfg_only(cfg))
            .expect("tiny encoder");
    FrozenAgent::new(label, encoder, entry.rows.clone())
}
