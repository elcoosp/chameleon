//! Cards, two-card hands, decks (SPECS/01 §2).
//!
//! `Card(u8)` idx 0..=51: rank = idx/4, suit = idx%4. Rank 0 = deuce … rank 12 = ace.
//! Suits: 0 = s, 1 = h, 2 = d, 3 = c.

use serde::{Deserialize, Serialize};

use crate::CoreError;

pub const RANK_CHARS: [char; 13] = [
    '2', '3', '4', '5', '6', '7', '8', '9', 'T', 'J', 'Q', 'K', 'A',
];
pub const SUIT_CHARS: [char; 4] = ['s', 'h', 'd', 'c'];

/// One playing card. idx 0..=51, rank = idx/4, suit = idx%4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Card(pub u8);

impl Card {
    pub const fn idx(self) -> u8 {
        self.0
    }
    pub const fn rank(self) -> u8 {
        self.0 / 4
    }
    pub const fn suit(self) -> u8 {
        self.0 % 4
    }
    /// Parse e.g. "As", "Td", "2c".
    pub fn parse(s: &str) -> Result<Card, CoreError> {
        let b = s.as_bytes();
        if b.len() != 2 {
            return Err(CoreError::InvalidCard(s.to_string()));
        }
        let rank = RANK_CHARS
            .iter()
            .position(|&c| c.eq_ignore_ascii_case(&(b[0] as char)))
            .ok_or_else(|| CoreError::InvalidCard(s.to_string()))?;
        let suit = SUIT_CHARS
            .iter()
            .position(|&c| c.eq_ignore_ascii_case(&(b[1] as char)))
            .ok_or_else(|| CoreError::InvalidCard(s.to_string()))? as u8;
        Ok(Card(rank as u8 * 4 + suit))
    }
    pub fn to_str(self) -> String {
        let r = RANK_CHARS[(self.0 / 4) as usize];
        let s = SUIT_CHARS[(self.0 % 4) as usize];
        format!("{r}{s}")
    }
    /// Bit flag `1 << rank` for rank-bitset arithmetic.
    pub const fn rank_bit(self) -> u16 {
        1u16 << (self.0 / 4)
    }
}

/// All 52 cards, ordered by idx.
pub const ALL_CARDS: [Card; 52] = {
    let mut cards = [Card(0); 52];
    let mut i = 0usize;
    while i < 52 {
        cards[i] = Card(i as u8);
        i += 1;
    }
    cards
};

/// A two-card hand stored as `(a << 8) | b` with `a < b` card indices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hand2(pub u16);

impl Hand2 {
    /// Build from two cards (order-independent; stores sorted).
    pub fn new(c1: Card, c2: Card) -> Hand2 {
        let (a, b) = if c1.0 < c2.0 { (c1.0, c2.0) } else { (c2.0, c1.0) };
        Hand2(((a as u16) << 8) | b as u16)
    }
    /// From a raw combo id (0..1326) used by `Range`.
    pub fn from_combo(combo: usize) -> Hand2 {
        // inverse of Hand2::combo_id
        let mut c1 = 0usize;
        while hand2_offset(c1 + 1) <= combo {
            c1 += 1;
        }
        let c2 = c1 + 1 + (combo - hand2_offset(c1));
        Hand2(((c1 as u16) << 8) | c2 as u16)
    }
    pub fn cards(self) -> [Card; 2] {
        [Card((self.0 >> 8) as u8), Card((self.0 & 0xff) as u8)]
    }
    /// Compact combo id in 0..1326 (triangular indexing over sorted card idx).
    pub fn combo_id(self) -> usize {
        let c1 = (self.0 >> 8) as usize;
        let c2 = (self.0 & 0xff) as usize;
        hand2_offset(c1) + (c2 - c1 - 1)
    }
    /// Suit-isomorphic canonical form (Waugh-style): the minimum over all 24 suit
    /// permutations. Index form for precomputed abstraction tables; joint
    /// (hand, board) orbits are canonicalized by cham-engine on top of this.
    pub fn canonical(self) -> Hand2 {
        let [a, b] = self.cards();
        let mut best = self;
        for p in SUIT_PERMS {
            let na = Card(a.rank() * 4 + p[a.suit() as usize]);
            let nb = Card(b.rank() * 4 + p[b.suit() as usize]);
            let cand = Hand2::new(na, nb);
            if cand < best {
                best = cand;
            }
        }
        best
    }
    /// Preflop class id in 0..=168 (pinned ordering: pairs by rank desc, then suited
    /// by (high, low) desc, then offsuit likewise).
    pub fn class_id(self) -> u8 {
        let [a, b] = self.cards();
        let (hi, lo) = if a.rank() >= b.rank() { (a.rank(), b.rank()) } else { (b.rank(), a.rank()) };
        if hi == lo {
            // pairs: AA=0 .. 22=12
            12 - hi
        } else {
            // non-pair rank pairs ordered by (hi desc, lo desc); 78 of them.
            // t = classes with higher high rank (Σ j for j in h+1..=12) + within offset
            let h = hi as usize;
            let t = 78 - h * (h + 1) / 2 + (h - 1 - lo as usize);
            if a.suit() == b.suit() {
                (13 + t) as u8
            } else {
                (91 + t) as u8
            }
        }
    }
    pub fn is_pair(self) -> bool {
        let [a, b] = self.cards();
        a.rank() == b.rank()
    }
    pub fn is_suited(self) -> bool {
        let [a, b] = self.cards();
        a.suit() == b.suit()
    }
}

/// offset of the first combo with first card = c1 (triangular indexing).
const fn hand2_offset(c1: usize) -> usize {
    c1 * 52 - c1 * (c1 + 1) / 2
}

/// The 24 suit permutations, as `new_suit = perm[old_suit]`.
pub const SUIT_PERMS: [[u8; 4]; 24] = build_perms();

const fn build_perms() -> [[u8; 4]; 24] {
    let mut out = [[0u8; 4]; 24];
    let mut n = 0;
    let mut a = 0;
    while a < 4 {
        let mut b = 0;
        while b < 4 {
            if b == a {
                b += 1;
                continue;
            }
            let mut c = 0;
            while c < 4 {
                if c == a || c == b {
                    c += 1;
                    continue;
                }
                let mut d = 0;
                while d < 4 {
                    if d == a || d == b || d == c {
                        d += 1;
                        continue;
                    }
                    out[n] = [a as u8, b as u8, c as u8, d as u8];
                    n += 1;
                    d += 1;
                }
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    out
}

/// A deck: 52 cards in shuffle order. The caller shuffles it with the project RNG;
/// the engine consumes it front-to-back (holes then board; no burns — documented).
#[derive(Clone, Copy, Debug)]
pub struct Deck {
    pub(crate) cards: [Card; 52],
    pos: u8,
}

impl Deck {
    /// Fresh ordered deck.
    pub fn ordered() -> Deck {
        Deck { cards: ALL_CARDS, pos: 0 }
    }
    /// Fisher–Yates shuffle under the project RNG (ChaCha8 — SPECS/00 §3.1).
    pub fn shuffled(rng: &mut crate::rng::Rng) -> Deck {
        use rand::Rng as _;
        let mut d = Deck::ordered();
        for i in (1..52).rev() {
            let j = rng.gen_range(0..=i);
            d.cards.swap(i, j);
        }
        d
    }
    /// Construct a deck with a specific prefix (used by replay: holes then board).
    pub fn with_prefix(prefix: &[Card]) -> Deck {
        let mut d = Deck::ordered();
        for (i, c) in prefix.iter().enumerate() {
            d.cards[i] = *c;
        }
        // remove duplicates from the tail by rebuilding: simplest correct approach —
        // collect used flags and fill the remainder with unused cards in idx order.
        let mut used = [false; 52];
        for c in prefix {
            used[c.idx() as usize] = true;
        }
        let mut k = prefix.len();
        for c in ALL_CARDS {
            if !used[c.idx() as usize] {
                d.cards[k] = c;
                k += 1;
            }
        }
        d
    }
    pub fn deal(&mut self) -> Result<Card, CoreError> {
        if self.pos >= 52 {
            return Err(CoreError::DeckExhausted { pos: self.pos });
        }
        let c = self.cards[self.pos as usize];
        self.pos += 1;
        Ok(c)
    }
    pub fn remaining(&self) -> u8 {
        52 - self.pos
    }
}
