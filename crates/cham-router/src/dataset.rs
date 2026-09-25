//! Binary `.rbin` dataset (SPECS/05 §4) + session-clustered splits (review C8).
//!
//! Format: magic "CHMR" u32 | version u32 | n_rows u32 | n_features u32 |
//! rows: [f32 × n_features][u8 label][u16 session_id][u8 family] (84 B/row @ 20 feats)

use serde::{Deserialize, Serialize};

use crate::RouterError;

pub const RBIN_MAGIC: u32 = 0x524D_4843; // "CHMR"
pub const RBIN_VERSION: u32 = 1;
pub const MAX_ROWS: usize = 2_000_000;

pub const SESSION_A: u8 = 0;
pub const SESSION_BDEV: u8 = 1;
pub const SESSION_BTEST: u8 = 2;
pub const SESSION_C: u8 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RbinRow {
    pub features: Vec<f32>,
    pub label: u8, // 0..=3 archetype
    pub session_id: u16,
    pub family: u8, // 0=A, 1=B, 2=PN, 3=noise
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DatasetMeta {
    pub rows: usize,
    pub sessions: usize,
    pub family_counts: std::collections::BTreeMap<String, u64>,
    pub abstraction_hash: String,
}

/// Session-clustered split (SPECS/05 §4): FNV-1a mod 10 partition.
/// A (0-5) router training; B-dev (6-7) tuning; B-test (8) headline; C (9) out-of-family.
pub fn split_of_session(session_id: u16) -> u8 {
    // FNV-1a mod-10 partition (SPECS/05 §4). Hashing the raw u16 LE bytes
    // degenerates for small sequential session ids (the high byte is always 0,
    // so odd/even ids collapse into just two buckets and B-dev/B-test end up
    // structurally empty). Pre-mix with the splitmix64 finalizer to avalanche
    // sequential ids before the FNV pass; the split stays a pure deterministic
    // function of the session id.
    let mut z = (session_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let z = z ^ (z >> 31);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in z.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let bucket = (h % 10) as u8;
    match bucket {
        0..=5 => SESSION_A,
        6..=7 => SESSION_BDEV,
        8 => SESSION_BTEST,
        _ => SESSION_C,
    }
}

/// Serialize rows to `.rbin` bytes.
pub fn encode_dataset(rows: &[RbinRow], n_features: usize) -> Result<Vec<u8>, RouterError> {
    if rows.len() > MAX_ROWS {
        return Err(RouterError::Dataset(format!(
            "rows {} exceeds cap {}",
            rows.len(),
            MAX_ROWS
        )));
    }
    let mut out = Vec::with_capacity(16 + rows.len() * (4 * n_features + 4));
    out.extend_from_slice(&RBIN_MAGIC.to_le_bytes());
    out.extend_from_slice(&RBIN_VERSION.to_le_bytes());
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend_from_slice(&(n_features as u32).to_le_bytes());
    for r in rows {
        if r.features.len() != n_features {
            return Err(RouterError::Dataset("row feature width mismatch".into()));
        }
        for f in &r.features {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.push(r.label);
        out.extend_from_slice(&r.session_id.to_le_bytes());
        out.push(r.family);
    }
    Ok(out)
}

/// Parse `.rbin` bytes; validates magic/version/width and rejects wrong-family rows
/// in A/B-dev splits (loader refusal = build failure, SPECS/05 §4).
pub fn decode_dataset(bytes: &[u8]) -> Result<(Vec<RbinRow>, usize), RouterError> {
    if bytes.len() < 16 {
        return Err(RouterError::Dataset("too short".into()));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().expect("4"));
    if magic != RBIN_MAGIC {
        return Err(RouterError::Dataset(format!("bad magic {magic:#x}")));
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().expect("4"));
    if version != RBIN_VERSION {
        return Err(RouterError::Dataset(format!("bad version {version}")));
    }
    let n = u32::from_le_bytes(bytes[8..12].try_into().expect("4")) as usize;
    let nf = u32::from_le_bytes(bytes[12..16].try_into().expect("4")) as usize;
    let row_sz = 4 * nf + 4;
    if bytes.len() < 16 + n * row_sz {
        return Err(RouterError::Dataset("truncated".into()));
    }
    let mut rows = Vec::with_capacity(n);
    let mut off = 16;
    for _ in 0..n {
        let mut features = Vec::with_capacity(nf);
        for _ in 0..nf {
            features.push(f32::from_le_bytes(
                bytes[off..off + 4].try_into().expect("4"),
            ));
            off += 4;
        }
        let label = bytes[off];
        let session_id = u16::from_le_bytes(bytes[off + 1..off + 3].try_into().expect("2"));
        let family = bytes[off + 3];
        off += 4;
        rows.push(RbinRow {
            features,
            label,
            session_id,
            family,
        });
    }
    // session-disjoint split enforcement + family governance: C-only sessions may
    // carry family != A; A/B-dev sessions must be family A (in-family only)
    for r in &rows {
        let split = split_of_session(r.session_id);
        if split != SESSION_C && r.family != 0 {
            return Err(RouterError::Dataset(format!(
                "family-{} row in split {} (session {}): out-of-family rows may only live in C",
                r.family, split, r.session_id
            )));
        }
    }
    Ok((rows, nf))
}

/// Write + read helpers.
pub fn write_dataset(
    path: &std::path::Path,
    rows: &[RbinRow],
    n_features: usize,
) -> Result<(), RouterError> {
    let bytes = encode_dataset(rows, n_features)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

pub fn read_dataset(path: &std::path::Path) -> Result<(Vec<RbinRow>, usize), RouterError> {
    let bytes = std::fs::read(path)?;
    decode_dataset(&bytes)
}
