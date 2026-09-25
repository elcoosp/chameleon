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

/// A node of the action tree. Infosets are identified by the ACTION SEQUENCE
/// (public information); strategies are shared across classes (abstraction).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// terminal: (hero_wins_bb if hero class stronger, pot already inside stacks)
    Terminal {
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
                hero_invested,
                villain_invested,
            };
        }
        let mut actions = vec!["fold".to_string(), "call".to_string()];
        let mut children = vec![
            Node::Terminal {
                hero_invested,
                villain_invested: hero_invested,
            }, // fold: hero takes invested
            self.showdown(hero_invested, villain_invested),
        ];
        // villain raise = 2.2× the bet (capped by jam)
        let hero_bet = hero_invested - villain_invested;
        let raise = (hero_bet * 2.2).min(self.stack_bb - hero_invested);
        let _ = hero_invested;
        if raise > hero_bet && villain_invested + raise <= self.stack_bb {
            actions.push("raise".to_string());
            children.push(self.hero_face_raise(hero_invested, villain_invested + raise));
        }
        Node::Decision {
            player: 1,
            actions,
            children,
        }
    }

    fn hero_face_raise(&self, hero_invested: f64, villain_invested: f64) -> Node {
        let actions = vec!["fold".to_string(), "call".to_string()];
        let children = vec![
            Node::Terminal {
                hero_invested: villain_invested,
                villain_invested,
            },
            self.showdown(hero_invested, villain_invested),
        ];
        Node::Decision {
            player: 0,
            actions,
            children,
        }
    }

    fn showdown(&self, hero_invested: f64, villain_invested: f64) -> Node {
        Node::Terminal {
            hero_invested,
            villain_invested,
        }
    }

    /// Showdown value for hero (bb) given class pair: deterministic strength order.
    pub fn showdown_value(
        &self,
        hero: &Class,
        villain: &Class,
        hero_invested: f64,
        villain_invested: f64,
    ) -> f64 {
        let pot = hero_invested + villain_invested;
        if hero.strength > villain.strength {
            villain_invested // villain's share flows to hero
        } else if hero.strength < villain.strength {
            -hero_invested
        } else {
            pot / 2.0 - hero_invested
        }
    }

    pub fn n_leaves(&self, node: &Node) -> u64 {
        match node {
            Node::Terminal { .. } => 1,
            Node::Decision { children, .. } => children.iter().map(|c| self.n_leaves(c)).sum(),
        }
    }
}
