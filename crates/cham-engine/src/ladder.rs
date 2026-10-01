//! Action ladder + pseudo-harmonic off-tree mapping (SPECS/02 §4).

use arrayvec::ArrayVec;

use cham_core::engine::Action;
use cham_core::obs::Observables;

use crate::config::AbstractionConfig;
use crate::encoder::{ActionClass, ActionSeq};

/// One abstract ladder slot: a concrete engine-legal action with its pot fraction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbstractAction {
    pub action: Action,
    /// is_all_in flag from the engine legality (canonicalization: all-in is
    /// Bet/Raise{to: cap}, never a variant — SPECS/00 §4).
    pub is_all_in: bool,
    /// Pot fraction this slot targets (for harmonic weights); Call/Fold/Check = 0.
    pub frac: f64,
}

/// Canonical slot order (SPECS/02 §4):
///
/// - facing no bet: `[Check, Bet(f1..fk), Jam]`
/// - facing a bet:  `[Fold, Call, Raise(f1..fk), Jam]`
///
/// Raises per street capped at `raises_per_street_cap` (count read from the seq);
/// beyond the cap the Raise slots are absent. Amounts: pot-after-call fractions,
/// floored to chips, clamped to `[min_to, max_to]`, deduped ascending.
#[derive(Clone, Debug)]
pub struct ActionLadder {
    pub cfg: AbstractionConfig,
}

impl ActionLadder {
    pub fn new(cfg: &AbstractionConfig) -> ActionLadder {
        ActionLadder { cfg: cfg.clone() }
    }

    fn fracs(&self, street: cham_core::engine::Street) -> &[f64] {
        match street {
            cham_core::engine::Street::Preflop => &self.cfg.ladder.raise_fracs, // completions/raises
            cham_core::engine::Street::Flop => &self.cfg.ladder.flop_bet_fracs,
            cham_core::engine::Street::Turn => &self.cfg.ladder.turn_bet_fracs,
            cham_core::engine::Street::River => &self.cfg.ladder.river_bet_fracs,
        }
    }

    /// Raises already made this street (from the deterministic action seq).
    pub fn raises_this_street(street: cham_core::engine::Street, seq: &ActionSeq) -> u32 {
        // F6a (2026-10-01, competitiveness report): cap on RE-RAISES only.
        // The previous version counted `Bet` toward the cap, so with
        // `raises_per_street_cap = 1` a postflop Bet immediately exhausted
        // the budget and the responder's only aggressive option was an
        // all-in jam. That is not a poker tree. Counting only `Raise`
        // (re-raises) restores the normal sequence: bet, raise, 3-bet, ...
        // Preflop has no `Bet` action, so this is behaviour-neutral there.
        seq.count_class(street, ActionClass::Raise)
    }

    pub fn slots(&self, obs: &Observables<'_>, seq: &ActionSeq) -> ArrayVec<AbstractAction, 12> {
        let mut out: ArrayVec<AbstractAction, 12> = ArrayVec::new();
        let facing = obs.to_call;
        let pot_after_call = obs.pot + facing;
        if facing == 0 {
            out.push(AbstractAction {
                action: Action::Check,
                is_all_in: false,
                frac: 0.0,
            });
            let max_to = obs.current_bet + obs.stack; // = stack (facing 0)
            // Short-stack: when stack < min bet, only an all-in for less is
            // legal. Mirror the engine's `min_full_level().min(max_to)` so
            // the ladder's lower bound is <= max_to (clamp() panics otherwise).
            let lower = obs.min_raise_to.min(max_to).max(1);
            for &f in self.fracs(obs.street) {
                let to = (f * pot_after_call as f64).floor() as i64;
                let to = to.clamp(lower, max_to);
                if !out.iter().any(|s| s.action == (Action::Bet { to })) {
                    out.push(AbstractAction {
                        action: Action::Bet { to },
                        is_all_in: to >= max_to,
                        frac: f,
                    });
                }
            }
            if self.cfg.ladder.all_in_always && max_to > obs.current_bet {
                let to = max_to;
                if !out
                    .iter()
                    .any(|s| matches!(s.action, Action::Bet { to: t } if t == to))
                {
                    out.push(AbstractAction {
                        action: Action::Bet { to },
                        is_all_in: true,
                        frac: f64::INFINITY,
                    });
                }
            }
        } else {
            out.push(AbstractAction {
                action: Action::Fold,
                is_all_in: false,
                frac: 0.0,
            });
            out.push(AbstractAction {
                action: Action::Call,
                is_all_in: facing >= obs.stack,
                frac: 0.0,
            });
            let raises = Self::raises_this_street(obs.street, seq);
            let can_raise = obs.stack > facing
                && obs.max_raise_to > obs.current_bet
                && raises < self.cfg.ladder.raises_per_street_cap;
            if can_raise {
                let min_to = (obs.current_bet + cham_min_raise(obs)).min(obs.max_raise_to);
                let max_to = obs.max_raise_to;
                for &f in self.cfg.ladder.raise_fracs.iter() {
                    let raise_by = f * pot_after_call as f64;
                    let to = (obs.current_bet as f64 + raise_by).floor() as i64;
                    let to = to.clamp(min_to, max_to);
                    if !out
                        .iter()
                        .any(|s| matches!(s.action, Action::Raise { to: t } if t == to))
                    {
                        out.push(AbstractAction {
                            action: Action::Raise { to },
                            is_all_in: to >= max_to,
                            frac: f,
                        });
                    }
                }
            }
            if self.cfg.ladder.all_in_always
                && obs.stack > facing
                && raises < self.cfg.ladder.raises_per_street_cap
            {
                let to = obs.max_raise_to;
                if !out
                    .iter()
                    .any(|s| matches!(s.action, Action::Raise { to: t } if t == to))
                {
                    out.push(AbstractAction {
                        action: Action::Raise { to },
                        is_all_in: true,
                        frac: f64::INFINITY,
                    });
                }
            } else if self.cfg.ladder.all_in_always && obs.stack > facing {
                // raise cap reached but a jam is still the only aggressive option
                let to = obs.max_raise_to;
                if to > obs.current_bet
                    && !out
                        .iter()
                        .any(|s| matches!(s.action, Action::Raise { to: t } if t == to))
                {
                    out.push(AbstractAction {
                        action: Action::Raise { to },
                        is_all_in: true,
                        frac: f64::INFINITY,
                    });
                }
            }
        }
        out
    }

    /// Real action for a slot index.
    pub fn to_real(&self, obs: &Observables<'_>, seq: &ActionSeq, slot: usize) -> Action {
        self.slots(obs, seq)[slot].action
    }

    /// Pot fraction of a real action (for size buckets + harmonic weights).
    pub fn frac_of(&self, obs: &Observables<'_>, a: Action) -> f64 {
        match a {
            Action::Fold | Action::Check | Action::Call => 0.0,
            Action::Bet { to } => to as f64 / obs.pot.max(1) as f64,
            Action::Raise { to } => {
                (to - obs.current_bet) as f64 / (obs.pot + obs.to_call).max(1) as f64
            }
        }
    }

    /// Fraction of EFFECTIVE STACK (depth-free sizing quantization for the key seq).
    pub fn stack_frac_of(&self, obs: &Observables<'_>, a: Action) -> f64 {
        let denom = obs.effective_stack.max(1) as f64;
        match a {
            Action::Fold | Action::Check | Action::Call => 0.0,
            Action::Bet { to } => (to - obs.current_bet) as f64 / denom,
            Action::Raise { to } => (to - obs.current_bet) as f64 / denom,
        }
    }

    /// Nearest-slot mapping — deterministic; used ONLY for infoset encoding needs
    /// where a slot index is required (e.g., off-tree size attribution).
    pub fn nearest_slot(&self, obs: &Observables<'_>, seq: &ActionSeq, a: Action) -> usize {
        let slots = self.slots(obs, seq);
        if let Some(i) = slots.iter().position(|s| s.action == a) {
            return i;
        }
        let f = self.frac_of(obs, a);
        let mut best = 0usize;
        let mut best_d = f64::INFINITY;
        for (i, s) in slots.iter().enumerate() {
            let sf = if s.frac.is_infinite() {
                f64::MAX
            } else {
                s.frac
            };
            let d = (sf - f).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// PSEUDO-HARMONIC off-tree weights (review D7): for a real off-tree size with
    /// pot fraction `f_real`, `w_i ∝ 1/(ε + (f_real − f_i)²)` over the top-2 nearest
    /// same-class slots, ε = 0.01, normalized. Never used for key encoding.
    pub fn harmonic_weights(
        &self,
        obs: &Observables<'_>,
        seq: &ActionSeq,
        a: Action,
    ) -> [(usize, f64); 2] {
        const EPS: f64 = 0.01;
        let slots = self.slots(obs, seq);
        let f_real = self.frac_of(obs, a);
        // same-class aggressive slots
        let class = match a {
            Action::Bet { .. } => 0usize,
            Action::Raise { .. } => 1usize,
            _ => 2usize,
        };
        let mut cands: ArrayVec<(usize, f64), 12> = ArrayVec::new();
        for (i, s) in slots.iter().enumerate() {
            #[allow(clippy::match_like_matches_macro)]
            let same = match (class, s.action) {
                (0, Action::Bet { .. }) | (1, Action::Raise { .. }) => true,
                (2, _) => true,
                _ => false,
            };
            if same {
                // ACTUAL pot fraction of the slot's action (the field `frac` is the
                // CONFIG fraction; clamping/dedupe shifts the realized size)
                let sf = self.frac_of(obs, s.action);
                cands.push((i, sf));
            }
        }
        if cands.is_empty() {
            // degenerate (no aggressive slots): dump weight on the first slot twice
            return [(0, 1.0), (0, 0.0)];
        }
        cands.sort_by(|x, y| {
            let dx = (x.1 - f_real).abs();
            let dy = (y.1 - f_real).abs();
            dx.partial_cmp(&dy).unwrap_or(std::cmp::Ordering::Equal)
        });
        let w = |f: f64| 1.0 / (EPS + (f_real - f) * (f_real - f));
        if cands.len() == 1 {
            return [(cands[0].0, 1.0), (cands[0].0, 0.0)];
        }
        let (i1, f1) = cands[0];
        let (i2, f2) = cands[1];
        let (w1, w2) = (w(f1), w(f2));
        let s = w1 + w2;
        [(i1, w1 / s), (i2, w2 / s)]
    }
}

/// Engine min-raise increment visible from observables: current + last full raise —
/// the observables expose `min_raise_to` (level) so use it directly.
fn cham_min_raise(obs: &Observables<'_>) -> i64 {
    obs.min_raise_to - obs.current_bet
}

/// Deterministic seq recording: append `(actor, class, size_bucket)` for `action`
/// given the pre-action observables. Pure function of inputs (depth-free).
pub fn record_action(
    ladder: &ActionLadder,
    obs_before: &Observables<'_>,
    actor: cham_core::obs::Player,
    a: Action,
    seq: &mut ActionSeq,
) {
    let class = match a {
        Action::Fold => ActionClass::Fold,
        Action::Check => ActionClass::Check,
        Action::Call => ActionClass::Call,
        Action::Bet { .. } => ActionClass::Bet,
        Action::Raise { .. } => ActionClass::Raise,
    };
    let sf = ladder.stack_frac_of(obs_before, a);
    let bucket = if matches!(a, Action::Fold | Action::Check | Action::Call) {
        0u8
    } else {
        ((sf * 12.0).round() as i64).clamp(1, 15) as u8
    };
    seq.push(
        obs_before.street,
        SeqEntryRaw {
            actor: actor.as_usize() as u8,
            class,
            size_bucket: bucket,
        },
    );
}

/// Raw seq entry before encoding.
pub struct SeqEntryRaw {
    pub actor: u8,
    pub class: ActionClass,
    pub size_bucket: u8,
}
