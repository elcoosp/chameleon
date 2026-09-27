//! The extensive-form river subgame (SPECS/06 §3).
//!
//! Ranges: the routed blueprint's reach over its abstraction, weighted by the
//! pseudo-harmonic mapping of off-tree sizes, then visit-confidence flattened
//! (SPECS/06 §3). Classes: each player's range collapses to weighted strength
//! classes (D-012) — deterministic strength ordering decides showdowns, which
//! makes every solver exactly LP-verifiable.

use serde::{Deserialize, Serialize};

use crate::SearchError;

/// One strength class: (weight, strength value). Showdown: higher value wins;
/// equal values split.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Class {
    pub weight: f64,
    pub strength: f64, // 0..1 river-equity rank
}

/// Extensive-form river subgame: hero acts first (check / bet fracs / jam),
/// villain responds (check-behind? call / fold / raise), then showdown or jam.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subgame {
    pub hero_classes: Vec<Class>,
    pub villain_classes: Vec<Class>,
    pub pot_bb: f64,
    pub stack_bb: f64,
    /// hero bet sizes as pot fractions (≤ 2 + jam; reduced tree)
    pub bet_fracs: Vec<f64>,
}

/// How a terminal was reached. C-3 fix (2026-09-27): a fold must be a
/// distinct outcome — its payoff is CLASS-INDEPENDENT. Before this fix,
/// both fold and showdown terminals went through `showdown_value`, so a
/// successful bluff with the weaker class LOST money and folding to a
/// raise could pay positive EV. That is a different game, not just a
/// rounding error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalKind {
    /// Reached showdown; strength ordering decides.
    Showdown,
    /// Hero forfeited (villain raised and hero folded).
    HeroFolds,
    /// Villain forfeited (hero bet and villain folded).
    VillainFolds,
}

/// A node of the action tree. Infosets are identified by the ACTION SEQUENCE
/// (public information); strategies are shared across classes (abstraction).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// Terminal outcome. `hero_invested` / `villain_invested` are RIVER
    /// money only (bb), starting from 0 at the root; the pre-river pot
    /// lives on `Subgame::pot_bb` and is accounted for in the payoff
    /// functions (`showdown_value`, `fold_value`), never in these fields.
    Terminal {
        kind: TerminalKind,
        hero_invested: f64,
        villain_invested: f64,
    },
    Decision {
        player: u8, // 0 hero, 1 villain
        /// action labels for children (parallel arrays)
        actions: Vec<String>,
        children: Vec<Node>,
    },
}

impl Subgame {
    pub fn build(
        hero_classes: Vec<Class>,
        villain_classes: Vec<Class>,
        pot_bb: f64,
        stack_bb: f64,
        bet_fracs: &[f64],
    ) -> Result<Subgame, SearchError> {
        if hero_classes.is_empty() || villain_classes.is_empty() {
            return Err(SearchError::Subgame("empty ranges".into()));
        }
        let w: f64 = hero_classes.iter().map(|c| c.weight).sum();
        if (w - 1.0).abs() > 1e-6 {
            return Err(SearchError::Subgame("hero weights must sum to 1".into()));
        }
        let w: f64 = villain_classes.iter().map(|c| c.weight).sum();
        if (w - 1.0).abs() > 1e-6 {
            return Err(SearchError::Subgame("villain weights must sum to 1".into()));
        }
        let mut fracs = bet_fracs.to_vec();
        fracs.retain(|f| f.is_finite() && *f > 0.0);
        fracs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        fracs.dedup();
        Ok(Subgame {
            hero_classes,
            villain_classes,
            pot_bb,
            stack_bb,
            bet_fracs: fracs,
        })
    }

    /// Build the full action tree: hero (check / bets / jam) → villain (fold / call
    /// / raise) → hero (call/fold vs raise) → showdown.
    pub fn tree(&self) -> Node {
        self.hero_node(0.0, 0.0)
    }

    fn hero_node(&self, hero_invested: f64, villain_invested: f64) -> Node {
        let mut actions = vec!["check".to_string()];
        let mut children = vec![self.villain_node(hero_invested, villain_invested, false)];
        let max_invest = self.pot_bb + 2.0 * self.stack_bb;
        for &f in &self.bet_fracs {
            let bet = (f * (self.pot_bb + 2.0 * villain_invested))
                .floor()
                .min(self.stack_bb);
            let bet = bet.max(0.5);
            if hero_invested + bet < max_invest && bet > villain_invested - hero_invested {
                actions.push(format!("bet{f}"));
                children.push(self.villain_node(hero_invested + bet, villain_invested, true));
            }
        }
        // jam
        let jam = self.stack_bb - hero_invested;
        if jam > villain_invested - hero_invested && hero_invested + jam < max_invest {
            actions.push("jam".to_string());
            children.push(self.villain_node(hero_invested + jam, villain_invested, true));
        }
        Node::Decision {
            player: 0,
            actions,
            children,
        }
    }

    fn villain_node(&self, hero_invested: f64, villain_invested: f64, facing_bet: bool) -> Node {
        if !facing_bet {
            // checked through → showdown
            return Node::Terminal {
                kind: TerminalKind::Showdown,
                hero_invested,
                villain_invested,
            };
        }
        // C-3 fix (2026-09-27): fold is a distinct terminal kind. Villain
        // folds: hero wins by forfeit — a class-independent payoff of
        // +pot_bb/2, never `showdown_value`.
        // C-2 fix (2026-09-27): villain's CALL invests to match hero's bet.
        // Before, the call child passed `villain_invested` through unchanged
        // (0 at the root), so getting called paid the same as checking —
        // value-betting the nuts yielded exactly what checking through
        // yielded. `showdown_value` reads the investments, so the fix is to
        // record villain's matched investment: `self.showdown(hero_invested,
        // hero_invested)`.
        let mut actions = vec!["fold".to_string(), "call".to_string()];
        let mut children = vec![
            Node::Terminal {
                kind: TerminalKind::VillainFolds,
                hero_invested,
                villain_invested, // villain's pre-call river money (0 at root)
            },
            self.showdown(hero_invested, hero_invested), // C-2: call matches bet
        ];
        // M-1 fix (2026-09-27): villain's raise is capped by VILLAIN's
        // remaining stack, not hero's. When hero bet more than stack/3.2 the
        // old cap (`stack_bb - hero_invested`) made `raise` smaller than the
        // bet and silently DROPPED the raise action instead of clamping to
        // a jam. Cap by villain's remaining; keep the action whenever the
        // raise-to exceeds hero's current investment (i.e., it is a legal
        // raise or a jam).
        let hero_bet = hero_invested - villain_invested;
        let raise_amount = (hero_bet * 2.2).min(self.stack_bb - villain_invested);
        let raise_to = villain_invested + raise_amount;
        if raise_to > hero_invested && raise_amount > 0.0 {
            actions.push("raise".to_string());
            children.push(self.hero_face_raise(hero_invested, raise_to));
        }
        Node::Decision {
            player: 1,
            actions,
            children,
        }
    }

    fn hero_face_raise(&self, hero_invested: f64, villain_invested: f64) -> Node {
        // C-3 fix (2026-09-27): hero fold is a distinct terminal kind with a
        // class-independent payoff of -(pot_bb/2 + hero's committed river
        // money). The previous code set `hero_invested: villain_invested` on
        // the fold child — falsely recording hero as having matched the
        // raise — and then routed through `showdown_value`, which could even
        // return a POSITIVE value for the fold. Both bugs are gone.
        let actions = vec!["fold".to_string(), "call".to_string()];
        let children = vec![
            Node::Terminal {
                kind: TerminalKind::HeroFolds,
                hero_invested, // what hero actually committed on the river
                villain_invested,
            },
            // On call, hero matches the raise: hero_invested → villain_invested
            self.showdown(villain_invested, villain_invested),
        ];
        Node::Decision {
            player: 0,
            actions,
            children,
        }
    }

    fn showdown(&self, hero_invested: f64, villain_invested: f64) -> Node {
        Node::Terminal {
            kind: TerminalKind::Showdown,
            hero_invested,
            villain_invested,
        }
    }

    /// Showdown value for hero (bb). C-4 fix (2026-09-27): the pre-river pot
    /// `pot_bb` MUST be included in the win/lose differences. Before this
    /// fix, `hero_invested`/`villain_invested` started at 0 and only tracked
    /// RIVER money, so a checked-through winner netted 0 instead of
    /// `+pot_bb/2`. The solver was playing a zero-pot game with
    /// pot-derived bet sizes — every bluff/value frequency derived from it
    /// was for a different game. Now:
    ///   hero wins:   +pot_bb/2 + villain_invested
    ///   hero loses:  -(pot_bb/2 + hero_invested)
    ///   split:       (villain_invested - hero_invested) / 2
    pub fn showdown_value(
        &self,
        hero: &Class,
        villain: &Class,
        hero_invested: f64,
        villain_invested: f64,
    ) -> f64 {
        let half_pot = self.pot_bb / 2.0;
        if hero.strength > villain.strength {
            half_pot + villain_invested
        } else if hero.strength < villain.strength {
            -(half_pot + hero_invested)
        } else {
            (villain_invested - hero_invested) / 2.0
        }
    }

    /// Fold value for hero (bb). C-3 fix (2026-09-27): a fold must NOT be
    /// evaluated through `showdown_value` — the payoff is class-independent.
    ///   villain folds: hero wins villain's pre-river pot share (+pot_bb/2)
    ///                  regardless of hero's hole strength
    ///   hero folds:    hero loses own pre-river pot share and own river
    ///                  money (-(pot_bb/2 + hero_invested))
    pub fn fold_value(&self, kind: TerminalKind, hero_invested: f64) -> f64 {
        match kind {
            TerminalKind::VillainFolds => self.pot_bb / 2.0,
            TerminalKind::HeroFolds => -(self.pot_bb / 2.0 + hero_invested),
            TerminalKind::Showdown => {
                unreachable!("cham-search: fold_value called on a showdown terminal")
            }
        }
    }

    pub fn n_leaves(&self, node: &Node) -> u64 {
        match node {
            Node::Terminal { .. } => 1,
            Node::Decision { children, .. } => children.iter().map(|c| self.n_leaves(c)).sum(),
        }
    }
}
