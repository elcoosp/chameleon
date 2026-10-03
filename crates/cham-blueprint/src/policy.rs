//! Quantized, strategy-only inference artifacts (SPECS/04 §6).
//!
//! - u8-quantized per-action probabilities (2 decimals), visits u32→u16 saturating
//! - owned-bytes load. **Unsafe-scope note (SPECS/00 §3.5):** the workspace's
//!   `#![forbid(unsafe_code)]` covers *this crate's own code*. `memmap2` (a
//!   whitelisted dependency) does contain unsafe at the mmap syscall boundary —
//!   that's the crate's business, not ours. Decision D-008 chose *not* to mmap
//!   these artifacts anyway: `BlueprintPolicy::load` reads the whole file into an
//!   owned `Vec<u8>` because inference bundles are ≤ ~100 MB for five experts
//!   (five orders of magnitude inside the 1.5 GB budget), and the simplicity of
//!   "the artifact is a byte slice we own" is worth more than the
//!   marginal memory savings.
//! - NO regrets ship in inference artifacts
//! - confidence = visits / (visits + 64) — visit-based (review B5; the v1
//!   regret-ratio formula saturated exactly when least converged and is DELETED)
//! - LAZY ROW DECODE (BROAD-PERF-PLAN B7): `load` reads the header + provenance
//!   eagerly and keeps the row payloads as opaque owned bytes — no per-row
//!   parsing, no quantization decode, no allocation beyond the file bytes
//!   (see `resident_bytes`). `strategy`/`confidence` binary-search the key
//!   index and decode exactly one row on first use. Same bytes → same hashes:
//!   `loader_hash_guards` and `artifact_hash_printed` pass unchanged, and a
//!   golden decision replay is bit-identical across independent loads
//!   (`artifact_lazy_replay_bit_identical`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use cham_engine::encoder::ActionSeq;

use crate::BlueprintError;
use crate::table::RegretTable;

pub const ARTIFACT_MAGIC: u32 = 0x5042_4843; // "CHBP"
/// §3.6: v2 = u16 probability quantization (was u8). `load` accepts v1
/// (legacy decode) and v2; `build_artifact` always writes v2.
pub const ARTIFACT_VERSION: u32 = 2;
/// Last readable legacy version (u8 probs, 1e-2 units).
const ARTIFACT_VERSION_V1: u32 = 1;

/// Provenance record (SPECS/04 §6).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub abstraction_hash: u64,
    pub artifact_hash: u64,
    pub mode: String,
    pub opponent_id: Option<String>,
    pub depth_bb: i64,
    pub iters: u64,
    pub train_seed: u64,
    pub thread_mode: String,
    pub threads: u32,
    pub parent: Option<String>,
    pub wall_s: f64,
    pub infosets: usize,
    pub created_unix: i64,
}

/// File layout (all LE):
/// `magic u32 | version u32 | abstraction_hash u64 | n u32 | pad u32 |
///  provenance_len u32 | provenance json bytes |
///  keys: n × u64 (ascending) | offsets: (n+1) × u32 |
///  rows: [w u8 | visits u16 | probs u16 × w] × n`
///
/// §3.6 (artifact v2): probabilities are u16 in units of 1/10000 (was u8
/// in units of 1/100 — actions below 0.5% vanished and mixed strategies
/// were rounded, harmful once policies serve as re-solve ranges). New
/// artifacts write v2; `load` still READS v1 (shipped bundles keep
/// working) and decodes both widths transparently.
#[derive(Clone, Debug)]
pub struct BlueprintPolicy {
    bytes: Vec<u8>,
    #[allow(dead_code)]
    prov_offset: usize,
    keys_off: usize,
    offsets_off: usize,
    rows_off: usize,
    n: usize,
    provenance: ProvenanceRecord,
    artifact_hash: u64,
    /// Probability quantum width in bytes: 1 (v1 legacy) or 2 (v2).
    prob_width: u8,
}

const HEADER_LEN: usize = 24;

impl BlueprintPolicy {
    /// Build an artifact from a trained table + provenance.
    ///
    /// H-6 fix (2026-09-27): the artifact is now **self-verifying**.
    /// `provenance.artifact_hash` embedded in `policy.bin` is
    /// `blake3(payload)[..8]` where `payload = keys || offsets || rows` —
    /// the semantically meaningful region. `load` recomputes this hash and
    /// refuses a mismatch. The previous version hashed the entire
    /// pre-provenance byte sequence (circular w.r.t. the hash field) and
    /// only wrote the correct hash to the *sibling* `provenance.json`, so a
    /// hand-edited `policy.bin` loaded silently.
    pub fn build_artifact(
        table: &RegretTable,
        prov: &ProvenanceRecord,
        out: &Path,
    ) -> Result<(), BlueprintError> {
        // collect rows: strategy-only, quantized (§3.6: u16, 1e-4 units)
        let mut rows: Vec<(u64, u8, u16, Vec<u16>)> = Vec::with_capacity(table.len());
        for (key, off, _w) in table.iter() {
            let w = table.row_width(off);
            let sigma = table.avg_strategy(off, w);
            let visits = table.visits(off, w).min(u16::MAX as u32) as u16;
            let probs: Vec<u16> = sigma
                .iter()
                .map(|p| ((*p * 10000.0).round() as i64).clamp(0, 10000) as u16)
                .collect();
            rows.push((key, w as u8, visits, probs));
        }
        rows.sort_by_key(|r| r.0);
        let n = rows.len();

        // Build the payload ONCE: keys, then offsets, then rows. The
        // artifact hash will be computed over exactly these bytes.
        let mut payload: Vec<u8> = Vec::with_capacity(n * 16);
        for (k, ..) in &rows {
            payload.extend_from_slice(&k.to_le_bytes());
        }
        let mut acc: u32 = 0;
        payload.extend_from_slice(&acc.to_le_bytes());
        for (_, _w, _visits, probs) in &rows {
            acc += (1 + 2 + 2 * probs.len()) as u32;
            payload.extend_from_slice(&acc.to_le_bytes());
        }
        for (_, w, visits, probs) in &rows {
            payload.push(*w);
            payload.extend_from_slice(&visits.to_le_bytes());
            for p in probs {
                payload.extend_from_slice(&p.to_le_bytes());
            }
        }

        let payload_hash = blake3::hash(&payload);
        let artifact_hash = u64::from_le_bytes(payload_hash.as_bytes()[..8].try_into().expect("8"));

        // Stamp the hash into the provenance that ships INSIDE policy.bin.
        let mut prov_embedded = prov.clone();
        prov_embedded.artifact_hash = artifact_hash;
        let prov_json = serde_json::to_vec(&prov_embedded)?;

        // Assemble header + provenance + padding + payload.
        let mut bytes: Vec<u8> =
            Vec::with_capacity(HEADER_LEN + prov_json.len() + 8 + payload.len());
        bytes.extend_from_slice(&ARTIFACT_MAGIC.to_le_bytes());
        bytes.extend_from_slice(&ARTIFACT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&prov.abstraction_hash.to_le_bytes());
        bytes.extend_from_slice(&(n as u32).to_le_bytes());
        bytes.extend_from_slice(&(prov_json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&prov_json);
        while bytes.len() % 8 != 0 {
            bytes.push(0u8);
        }
        bytes.extend_from_slice(&payload);

        std::fs::create_dir_all(out).map_err(|e| BlueprintError::Artifact {
            path: out.to_path_buf(),
            reason: format!("mkdir: {e}"),
        })?;
        // Sibling provenance.json is human-readable (pretty); the embedded
        // one is compact. Same content, both carry the artifact_hash.
        let prov_pretty = serde_json::to_vec_pretty(&prov_embedded)?;
        std::fs::write(out.join("provenance.json"), &prov_pretty)?;
        std::fs::write(out.join("policy.bin"), &bytes)?;
        Ok(())
    }

    /// Load an artifact from a directory containing `policy.bin`.
    pub fn load(
        dir: &Path,
        expected_abstraction_hash: u64,
    ) -> Result<BlueprintPolicy, BlueprintError> {
        let path = dir.join("policy.bin");
        let bytes = std::fs::read(&path).map_err(|e| BlueprintError::Artifact {
            path: path.clone(),
            reason: format!("read: {e}"),
        })?;
        let artifact_hash =
            u64::from_le_bytes(blake3::hash(&bytes).as_bytes()[..8].try_into().expect("8"));
        if bytes.len() < HEADER_LEN {
            return Err(BlueprintError::Artifact {
                path,
                reason: "too short".into(),
            });
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().expect("4"));
        if magic != ARTIFACT_MAGIC {
            return Err(BlueprintError::Artifact {
                path,
                reason: format!("bad magic {magic:#x}"),
            });
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().expect("4"));
        // §3.6: write v2, read v1+v2. Anything else is rejected loudly.
        let prob_width = match version {
            ARTIFACT_VERSION => 2u8,
            ARTIFACT_VERSION_V1 => 1u8,
            v => {
                return Err(BlueprintError::Artifact {
                    path,
                    reason: format!("bad version {v}"),
                });
            }
        };
        let abstraction_hash = u64::from_le_bytes(bytes[8..16].try_into().expect("8"));
        if expected_abstraction_hash != 0 && abstraction_hash != expected_abstraction_hash {
            return Err(BlueprintError::HashMismatch {
                expected: expected_abstraction_hash,
                found: abstraction_hash,
            });
        }
        let n = u32::from_le_bytes(bytes[16..20].try_into().expect("4")) as usize;
        let prov_len = u32::from_le_bytes(bytes[20..24].try_into().expect("4")) as usize;
        // prov_len is the last header field; the JSON begins at HEADER_LEN
        let prov_offset = HEADER_LEN;
        let prov_json = &bytes[prov_offset..prov_offset + prov_len];
        let provenance: ProvenanceRecord = serde_json::from_slice(prov_json)?;
        let mut o = prov_offset + prov_len;
        if o % 8 != 0 {
            o += 8 - o % 8;
        }
        let keys_off = o;
        // M-8 fix (2026-09-27): the previous check was
        //   let need = rows_off + (bytes.len() - rows_off);
        //   if bytes.len() < need { ... }
        // which is algebraically `bytes.len() < bytes.len()` — vacuously
        // false, and when `rows_off > bytes.len()` the subtraction underflows
        // (wraps) making `need` equal to `bytes.len()` again. Either way the
        // "truncated" branch is dead. A truncated `policy.bin` then panics on
        // a later slice instead of returning a clean error. Check `rows_off`
        // directly, and use `checked_*` to guard the u64 arithmetic.
        let offsets_off = match keys_off.checked_add(n.saturating_mul(8)) {
            Some(v) => v,
            None => {
                return Err(BlueprintError::Artifact {
                    path,
                    reason: "row count overflow".into(),
                });
            }
        };
        let rows_off = match offsets_off.checked_add((n + 1).saturating_mul(4)) {
            Some(v) => v,
            None => {
                return Err(BlueprintError::Artifact {
                    path,
                    reason: "row count overflow".into(),
                });
            }
        };
        if bytes.len() < rows_off {
            return Err(BlueprintError::Artifact {
                path,
                reason: format!(
                    "truncated: need at least {rows_off} bytes for {n} rows, file has {}",
                    bytes.len()
                ),
            });
        }
        // Row payloads must also fit: the last row's `offset_at(n)` gives the
        // final payload end within the rows region.
        let last_payload_end = {
            let o = offsets_off + n * 4;
            u32::from_le_bytes(bytes[o..o + 4].try_into().expect("4")) as usize
        };
        if rows_off + last_payload_end > bytes.len() {
            return Err(BlueprintError::Artifact {
                path,
                reason: format!(
                    "truncated rows: offset table reaches {}, file has {}",
                    rows_off + last_payload_end,
                    bytes.len()
                ),
            });
        }

        // H-6 fix v2 (2026-09-27): verify the artifact against its embedded
        // payload hash. The check is gated on `provenance.artifact_hash != 0`
        // because every artifact written by the PRE-H-6 build path carries
        // `artifact_hash: 0` in its embedded provenance: the old
        // `build_artifact` wrote the true hash only to the SIBLING
        // `provenance.json`, never into `policy.bin` itself. Gating on zero
        // preserves loadability of those bundles while still enforcing the
        // contract on every artifact this build produces (which stamps the
        // real payload hash, per the H-6 write-side fix).
        //
        // The abstraction-hash check above (skippable via
        // CHAM_IGNORE_ABSTRACTION_HASH in the loader) is a separate, older
        // guard; the payload check here is the one that catches a
        // hand-edited action probability.
        if provenance.artifact_hash != 0 {
            let payload_end = rows_off + last_payload_end;
            let payload_hash = blake3::hash(&bytes[keys_off..payload_end]);
            let payload_hash_u64 =
                u64::from_le_bytes(payload_hash.as_bytes()[..8].try_into().expect("8"));
            if payload_hash_u64 != provenance.artifact_hash {
                return Err(BlueprintError::HashMismatch {
                    expected: provenance.artifact_hash,
                    found: payload_hash_u64,
                });
            }
        }
        let _ = artifact_hash; // whole-file hash retained for API compat

        Ok(BlueprintPolicy {
            bytes,
            prov_offset,
            keys_off,
            offsets_off,
            rows_off,
            n,
            provenance,
            artifact_hash,
            prob_width,
        })
    }

    #[inline]
    fn key_at(&self, i: usize) -> u64 {
        let o = self.keys_off + i * 8;
        u64::from_le_bytes(self.bytes[o..o + 8].try_into().expect("8"))
    }

    #[inline]
    fn offset_at(&self, i: usize) -> usize {
        let o = self.offsets_off + i * 4;
        u32::from_le_bytes(self.bytes[o..o + 4].try_into().expect("4")) as usize
    }

    /// Binary search for the row of `key`.
    fn find(&self, key: u64) -> Option<usize> {
        let mut lo = 0usize;
        let mut hi = self.n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.key_at(mid) < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < self.n && self.key_at(lo) == key {
            Some(lo)
        } else {
            None
        }
    }

    /// Strategy at an infoset (normalized, dequantized); None if uncovered.
    pub fn strategy(
        &self,
        obs: &cham_core::obs::Observables<'_>,
        enc: &mut cham_engine::Encoder,
        seq: &ActionSeq,
    ) -> Option<Vec<f64>> {
        let key = enc.key(obs, seq);
        let idx = self.find(key.0)?;
        let start = self.rows_off + self.offset_at(idx);
        let end = self.rows_off + self.offset_at(idx + 1);
        if end <= start {
            return None;
        }
        let w = self.bytes[start] as usize;
        let probs = self.decode_row(start + 3, w)?;
        let total: u64 = probs.iter().sum();
        if total == 0 {
            return Some(vec![1.0 / w as f64; w]);
        }
        Some(probs.iter().map(|&b| b as f64 / total as f64).collect())
    }

    /// Export all rows as (key, normalized distribution) pairs (v3 §6, M6):
    /// materializes the frozen snapshot for `FrozenAgent` without exposing
    /// the quantized byte layout outside this module. Distributions are
    /// dequantized exactly as [`Self::strategy`] decodes them, so the export
    /// is bit-identical to live queries.
    pub fn export_rows(&self) -> Vec<(u64, Vec<f64>)> {
        let mut out = Vec::with_capacity(self.n);
        for i in 0..self.n {
            let key = self.key_at(i);
            let start = self.rows_off + self.offset_at(i);
            let end = self.rows_off + self.offset_at(i + 1);
            if end <= start {
                continue;
            }
            let w = self.bytes[start] as usize;
            let Some(probs) = self.decode_row(start + 3, w) else {
                continue;
            };
            let total: u64 = probs.iter().sum();
            let dist = if total == 0 {
                vec![1.0 / w as f64; w]
            } else {
                probs.iter().map(|&b| b as f64 / total as f64).collect()
            };
            out.push((key, dist));
        }
        out
    }

    /// Visit-based confidence: c = visits / (visits + 64); None if uncovered.
    pub fn confidence(
        &self,
        obs: &cham_core::obs::Observables<'_>,
        enc: &mut cham_engine::Encoder,
        seq: &ActionSeq,
    ) -> Option<f64> {
        let key = enc.key(obs, seq);
        let idx = self.find(key.0)?;
        let start = self.rows_off + self.offset_at(idx);
        let visits =
            u16::from_le_bytes(self.bytes[start + 1..start + 3].try_into().expect("2")) as f64;
        Some(visits / (visits + 64.0))
    }

    pub fn provenance(&self) -> &ProvenanceRecord {
        &self.provenance
    }

    pub fn abstraction_hash(&self) -> u64 {
        self.provenance.abstraction_hash
    }

    pub fn artifact_hash(&self) -> u64 {
        self.artifact_hash
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Owned resident bytes (B7): exactly the file bytes — load performs no
    /// per-row parse or decode, so this equals the `policy.bin` file size.
    pub fn resident_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Decode one row's probability quanta (width-aware: v1 u8 / v2 u16).
    fn decode_row(&self, at: usize, w: usize) -> Option<Vec<u64>> {
        decode_row(&self.bytes, self.prob_width, at, w).map(|(v, _)| v)
    }

    /// Decode one row's quanta plus their scale (v1: 100.0, v2: 10000.0).
    fn decode_row_scaled(&self, at: usize, w: usize) -> Option<(Vec<u64>, f64)> {
        decode_row(&self.bytes, self.prob_width, at, w)
    }

    /// Touch every row once (sums dequantized mass): warmup helper for
    /// river-only processes that want decode faults paid up front. Pure read —
    /// never mutates the artifact.
    pub fn prefetch_all(&self) -> f64 {
        let mut acc = 0.0;
        for i in 0..self.n {
            let start = self.rows_off + self.offset_at(i);
            let end = self.rows_off + self.offset_at(i + 1);
            if end > start {
                let w = self.bytes[start] as usize;
                if let Some((probs, scale)) = self.decode_row_scaled(start + 3, w) {
                    for b in probs {
                        acc += b as f64 / scale;
                    }
                }
            }
        }
        acc
    }
}

/// Decode one row's probability payload at either quantum width (§3.6
/// artifact v2; v1 legacy supported on load). Returns `None` when the
/// byte range is out of bounds (truncated file that passed the coarse
/// length check).
fn decode_row(bytes: &[u8], prob_width: u8, at: usize, w: usize) -> Option<(Vec<u64>, f64)> {
    match prob_width {
        1 => {
            let end = at.checked_add(w)?;
            let slice = bytes.get(at..end)?;
            Some((slice.iter().map(|&b| b as u64).collect(), 100.0))
        }
        _ => {
            let end = at.checked_add(w.checked_mul(2)?)?;
            let slice = bytes.get(at..end)?;
            let mut out = Vec::with_capacity(w);
            for c in slice.chunks_exact(2) {
                out.push(u16::from_le_bytes([c[0], c[1]]) as u64);
            }
            Some((out, 10000.0))
        }
    }
}
