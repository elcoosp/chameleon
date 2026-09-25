//! Waugh-style suit-isomorphism joint (hand, board) orbit indexing (SPECS/02 §3).
//!
//! Canonicalization: apply all 24 suit permutations to (hole, board), sort hole and
//! board cards, take the lexicographic minimum of the packed representation. The
//! canonical packed value is the orbit key; the runtime table is a sorted key array
//! (binary-search lookup, byte-wise loads — alignment-proof under mmap) + parallel
//! bucket array, mmap'd read-only.
//!
//! Orbit counts (spec): flop ≈ 1,286,792; turn ≈ 55,190,538.

use cham_core::card::{Card, Hand2, SUIT_PERMS};

/// Encode (hole, board) as u64: hole[0] hole[1] board[0..], sorted within groups,
/// 8 bits per card, hole in the high bytes. Boards up to 4 cards (flop/turn).
#[inline]
pub fn pack(hole: &mut [Card; 2], board: &mut [Card]) -> u64 {
    hole.sort_unstable();
    board.sort_unstable();
    let mut v = 0u64;
    v |= (hole[0].idx() as u64) << 48;
    v |= (hole[1].idx() as u64) << 40;
    for (i, c) in board.iter().enumerate() {
        v |= (c.idx() as u64) << (32 - 8 * i as u32);
    }
    v
}

/// Canonical (hand, board) orbit key under the 24 suit permutations.
pub fn canonical_key(hole: Hand2, board: &[Card]) -> u64 {
    let mut best = u64::MAX;
    let mut hole_perm = [Card(0); 2];
    let mut board_perm = [Card(0); 5];
    for p in SUIT_PERMS {
        let [a, b] = hole.cards();
        hole_perm[0] = Card(a.rank() * 4 + p[a.suit() as usize]);
        hole_perm[1] = Card(b.rank() * 4 + p[b.suit() as usize]);
        for (i, c) in board.iter().enumerate() {
            board_perm[i] = Card(c.rank() * 4 + p[c.suit() as usize]);
        }
        let mut hp = hole_perm;
        let bp_slice = &mut board_perm[..board.len()];
        let k = pack(&mut hp, bp_slice);
        if k < best {
            best = k;
        }
    }
    best
}

/// A sorted orbit table. File format (all LE):
/// `magic u32 | version u32 | n u64 | default_bucket u16 | pad u16 | pad u32 |
///  keys: n × u64 (offset 24, ascending) | buckets: n × u16`
pub const TABLE_MAGIC: u32 = 0x3148_4343;
pub const TABLE_VERSION: u32 = 1;
pub const HEADER_LEN: usize = 24;

/// Serialize keys+payload into the canonical file bytes (sorts keys in place).
pub fn encode_table(keys: &mut Vec<u64>, buckets: &mut Vec<u16>, default_bucket: u16) -> Vec<u8> {
    assert_eq!(keys.len(), buckets.len());
    let mut pairs: Vec<(u64, u16)> = keys.iter().copied().zip(buckets.iter().copied()).collect();
    pairs.sort_unstable();
    let mut out = Vec::with_capacity(HEADER_LEN + pairs.len() * 10);
    out.extend_from_slice(&TABLE_MAGIC.to_le_bytes());
    out.extend_from_slice(&TABLE_VERSION.to_le_bytes());
    out.extend_from_slice(&(pairs.len() as u64).to_le_bytes());
    out.extend_from_slice(&default_bucket.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]); // pad to 24
    for (k, _b) in &pairs {
        out.extend_from_slice(&k.to_le_bytes());
    }
    for (_, b) in &pairs {
        out.extend_from_slice(&b.to_le_bytes());
    }
    keys.clear();
    buckets.clear();
    for (k, b) in pairs {
        keys.push(k);
        buckets.push(b);
    }
    out
}

/// View over table bytes (mmap-backed or owned). Lookups read bytes directly
/// (`u64::from_le_bytes`) — no alignment requirement, no unsafe.
pub struct TableView<'a> {
    pub n: u64,
    pub default_bucket: u16,
    bytes: &'a [u8],
}

impl<'a> TableView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<TableView<'a>, crate::EngineError> {
        let path = std::path::PathBuf::from("<table>");
        if bytes.len() < HEADER_LEN {
            return Err(crate::EngineError::Artifact {
                path,
                reason: "too short".into(),
            });
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().expect("4"));
        if magic != TABLE_MAGIC {
            return Err(crate::EngineError::Artifact {
                path,
                reason: format!("bad magic {magic:#x}"),
            });
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().expect("4"));
        if version != TABLE_VERSION {
            return Err(crate::EngineError::Artifact {
                path,
                reason: format!("bad version {version}"),
            });
        }
        let n = u64::from_le_bytes(bytes[8..16].try_into().expect("8"));
        let default_bucket = u16::from_le_bytes(bytes[16..18].try_into().expect("2"));
        let need = HEADER_LEN + (n as usize) * 10;
        if bytes.len() < need {
            return Err(crate::EngineError::Artifact {
                path,
                reason: format!("truncated: need {need}, have {}", bytes.len()),
            });
        }
        Ok(TableView {
            n,
            default_bucket,
            bytes,
        })
    }

    #[inline]
    fn key_at(&self, i: usize) -> u64 {
        let o = HEADER_LEN + i * 8;
        u64::from_le_bytes(self.bytes[o..o + 8].try_into().expect("8"))
    }

    #[inline]
    fn bucket_at(&self, i: usize) -> u16 {
        let o = HEADER_LEN + self.n as usize * 8 + i * 2;
        u16::from_le_bytes(self.bytes[o..o + 2].try_into().expect("2"))
    }

    /// Binary search: bucket for an orbit key, or `default_bucket` on a miss.
    #[inline]
    pub fn lookup(&self, key: u64) -> u16 {
        let n = self.n as usize;
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.key_at(mid) < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < n && self.key_at(lo) == key {
            self.bucket_at(lo)
        } else {
            self.default_bucket
        }
    }

    pub fn len(&self) -> u64 {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The orbit keys (streamed; used by `canon_index_orbits` verification).
    pub fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        (0..self.n as usize).map(move |i| self.key_at(i))
    }
}

/// Enumerate canonical orbit representatives for hands against boards of
/// `board_len` cards. A pair (hand, board) is an orbit REPRESENTATIVE iff it equals
/// its own canonicalization. Returns sorted keys.
pub fn enumerate_orbits(board_len: usize) -> Vec<u64> {
    let mut keys = Vec::new();
    for c1 in 0..52usize {
        for c2 in (c1 + 1)..52usize {
            let hand = Hand2::new(Card(c1 as u8), Card(c2 as u8));
            // iterate boards excluding the hole cards
            let mut board = [Card(0); 4];
            match board_len {
                3 => {
                    for b1 in 0..52usize {
                        for b2 in (b1 + 1)..52usize {
                            for b3 in (b2 + 1)..52usize {
                                let cs = [b1, b2, b3];
                                if cs.iter().any(|&c| c == c1 || c == c2) {
                                    continue;
                                }
                                for (i, &c) in cs.iter().enumerate() {
                                    board[i] = Card(c as u8);
                                }
                                let mut hp = [hand.cards()[0], hand.cards()[1]];
                                let mut bp = [Card(0); 4];
                                bp[..3].copy_from_slice(&board[..3]);
                                let bp = &mut bp[..3];
                                let k = pack(&mut hp, bp);
                                let ck = canonical_key(hand, bp);
                                if ck == k {
                                    keys.push(k);
                                }
                            }
                        }
                    }
                }
                4 => {
                    for b1 in 0..52usize {
                        for b2 in (b1 + 1)..52usize {
                            for b3 in (b2 + 1)..52usize {
                                for b4 in (b3 + 1)..52usize {
                                    let cs = [b1, b2, b3, b4];
                                    if cs.iter().any(|&c| c == c1 || c == c2) {
                                        continue;
                                    }
                                    for (i, &c) in cs.iter().enumerate() {
                                        board[i] = Card(c as u8);
                                    }
                                    let mut hp = [hand.cards()[0], hand.cards()[1]];
                                    let mut bp = [Card(0); 4];
                                    bp.copy_from_slice(&board);
                                    let k = pack(&mut hp, &mut bp);
                                    let ck = canonical_key(hand, &bp);
                                    if ck == k {
                                        keys.push(k);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => panic!("board_len must be 3 or 4"),
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}
