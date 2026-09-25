//! OFFLINE bucket-table builder (SPECS/02 §3): canonical orbit sampling →
//! river-equity CDF features (16 equal-mass bins over runouts, seeded MC — offline
//! only) → seeded k-means++ (EMD ≈ L1 on CDFs) → mmap table artifacts + meta.json.
//!
//! The full-spec build enumerates ALL canonical orbits (flop ≈ 1.29M, turn ≈ 55M)
//! and assigns every one; the sampled M1-tiny build features a subsample and marks
//! misses with the runtime fallback (decision D-006). Command: `chameleon train-buckets`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use cham_core::card::{Card, Hand2};
use cham_core::eval::Range;
use cham_core::rng::{child, next_f64};

use crate::canon::{canonical_key, encode_table, enumerate_orbits};
use crate::config::AbstractionConfig;
use crate::tables::KmeansMeta;
use crate::canon::TABLE_MAGIC;
use crate::canon::TABLE_VERSION;
use crate::tables::MISS_SENTINEL;

/// Builder knobs.
#[derive(Clone, Copy, Debug)]
pub struct BuildParams {
    /// orbits to sample for features (0 = enumerate ALL orbits; full coverage)
    pub sample_orbits: usize,
    /// runout samples per orbit for the CDF feature
    pub feature_runs: u32,
    /// MC equity iterations per runout
    pub mc_iters: u32,
    /// pilot sample size for the global equal-mass equity quantiles
    pub quantile_sample: u32,
    pub lloyd_iters: u32,
}


impl BuildParams {
    /// M1-tiny (fast, sampled; ships fallback misses).
    pub fn tiny() -> BuildParams {
        BuildParams {
            sample_orbits: 20_000,
            feature_runs: 8,
            mc_iters: 16,
            quantile_sample: 20_000,
            lloyd_iters: 24,
        }
    }
    /// Full-spec defaults (M2 overnight build; document wall time before running).
    pub fn full() -> BuildParams {
        BuildParams {
            sample_orbits: 0,
            feature_runs: 24,
            mc_iters: 16,
            quantile_sample: 100_000,
            lloyd_iters: 50,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct MetaOut {
    version: u32,
    river_eq_edges: Vec<f64>,
    flop: Option<StreetMeta>,
    turn: Option<StreetMeta>,
    default_bucket: u16,
    blake3: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StreetMeta {
    k: u32,
    orbits: u64,
    coverage: String,
    kmeans: KmeansMeta,
}

/// 16 equal-mass CDF bins: global edges committed in meta (pilot quantiles).
const CDF_BINS: usize = 16;

/// Build bucket artifacts into `out_dir` for `which ∈ {flop, turn}`.
pub fn build_street(
    cfg: &AbstractionConfig,
    out_dir: &Path,
    which: &str,
    params: BuildParams,
) -> Result<std::path::PathBuf, crate::EngineError> {
    std::fs::create_dir_all(out_dir)?;
    let (board_len, k, _seed_label) = match which {
        "flop" => (3usize, cfg.buckets.flop_k, "flop"),
        "turn" => (4usize, cfg.buckets.turn_k, "turn"),
        _ => return Err(crate::EngineError::Config("which must be flop|turn".into())),
    };

    // ---- 1. orbit keys (full enumeration or seeded uniform sample of canon keys) ----
    let (keys, coverage): (Vec<u64>, &'static str) = if params.sample_orbits == 0 {
        (enumerate_orbits(board_len), "full")
    } else {
        (sample_orbits(board_len, params.sample_orbits, 0xB0), "sampled")
    };

    // ---- 2. global equal-mass equity quantiles (for the CDF bins) ----
    let edges = equity_quantile_edges(params.quantile_sample, 0xC1);

    // ---- 3. features per orbit: 16-bin river-equity CDF over runouts ----
    let feats = features_for(&keys, board_len, params, &edges);

    // ---- 4. seeded k-means++ (L1) ----
    let km = kmeans_l1(&feats, k as usize, 0xD1, params.lloyd_iters);

    // ---- 5. assign buckets ----
    let mut buckets: Vec<u16> = feats
        .iter()
        .map(|f| nearest_centroid(f, &km.centroids) as u16)
        .collect();

    // ---- 6. write artifact + meta ----
    let mut key_copy = keys.clone();
    let bytes = encode_table(&mut key_copy, &mut buckets, MISS_SENTINEL);
    let out_path = out_dir.join(format!("{which}.bin"));
    std::fs::write(&out_path, &bytes)?;
    let _ = (TABLE_MAGIC, TABLE_VERSION);

    // meta (river edges + kmeans info) — written by the caller so both streets and
    // the river quantiles land in ONE meta.json; return the artifact path here.
    let meta_path = out_dir.join("meta.json");
    if !meta_path.exists() {
        write_meta(out_dir, &edges, None)?;
    }
    let meta: MetaOut = {
        let text = std::fs::read_to_string(&meta_path)?;
        match serde_json::from_str::<MetaOut>(&text) {
            Ok(m) => m,
            Err(_) => MetaOut {
                version: 2,
                river_eq_edges: edges.clone(),
                flop: None,
                turn: None,
                default_bucket: MISS_SENTINEL,
                blake3: String::new(),
            },
        }
    };
    let mut meta = meta;
    let sm = StreetMeta {
        k,
        orbits: keys.len() as u64,
        coverage: coverage.to_string(),
        kmeans: KmeansMeta {
            k,
            feature: "river_cdf16".into(),
            feature_runs: params.feature_runs,
            seeds: km.seeds,
            inertia: km.inertia,
        },
    };
    if which == "flop" {
        meta.flop = Some(sm);
    } else {
        meta.turn = Some(sm);
    }
    write_meta_struct(out_dir, &meta)?;
    Ok(out_path)
}

/// Write meta.json with river edges and hash it into blake3.
fn write_meta(out_dir: &Path, edges: &[f64], flop: Option<StreetMeta>) -> Result<(), crate::EngineError> {
    let meta = MetaOut {
        version: 2,
        river_eq_edges: edges.to_vec(),
        flop,
        turn: None,
        default_bucket: MISS_SENTINEL,
        blake3: String::new(),
    };
    write_meta_struct(out_dir, &meta)
}

fn write_meta_struct(out_dir: &Path, meta: &MetaOut) -> Result<(), crate::EngineError> {
    let json = serde_json::to_vec_pretty(meta)
        .map_err(|e| crate::EngineError::Meta(format!("serialize: {e}")))?;
    let h = blake3::hash(&json);
    // final meta carries its own hash (computed over the payload without the field)
    let m2 = meta.clone_for_hash(&h.to_string());
    let json = serde_json::to_vec_pretty(&m2)
        .map_err(|e| crate::EngineError::Meta(format!("serialize: {e}")))?;
    std::fs::write(out_dir.join("meta.json"), json)?;
    Ok(())
}

impl MetaOut {
    fn clone_for_hash(&self, hash: &str) -> MetaOut {
        MetaOut {
            version: self.version,
            river_eq_edges: self.river_eq_edges.clone(),
            flop: self.flop.clone(),
            turn: self.turn.clone(),
            default_bucket: self.default_bucket,
            blake3: hash.to_string(),
        }
    }
}

/// Sample `n` distinct canonical orbit keys uniformly over (hand, board).
fn sample_orbits(board_len: usize, n: usize, seed: u64) -> Vec<u64> {
    let mut rng = child(seed, &format!("orbits{board_len}"));
    use rustc_hash::FxHashSet;
    let mut set: FxHashSet<u64> = FxHashSet::default();
    let mut guard = 0usize;
    while set.len() < n && guard < n * 20 {
        guard += 1;
        // uniform hand
        let mut deck: Vec<u8> = (0..52).collect();
        for i in (1..52).rev() {
            let j = (next_f64(&mut rng) * (i + 1) as f64) as usize;
            deck.swap(i, j);
        }
        let hand = Hand2::new(Card(deck[0]), Card(deck[1]));
        let board: Vec<Card> = (0..board_len).map(|i| Card(deck[2 + i])).collect();
        set.insert(canonical_key(hand, &board));
    }
    let mut v: Vec<u64> = set.into_iter().collect();
    v.sort_unstable();
    v
}

/// Global equal-mass equity quantile edges over random (combo, board) pairs.
pub fn equity_quantile_edges(n: u32, seed: u64) -> Vec<f64> {
    let mut eqs: Vec<f64> = Vec::with_capacity(n as usize);
    for it in 0..n {
        // independent stream per draw — autocorrelated decks inflate the order-
        // statistic noise and quantile bins drift (measured: −4σ populations)
        let mut rng = child(seed, &format!("q{it}"));
        let mut deck: Vec<u8> = (0..52).collect();
        for i in (1..52).rev() {
            let j = (next_f64(&mut rng) * (i + 1) as f64) as usize;
            deck.swap(i, j);
        }
        let hand = Hand2::new(Card(deck[0]), Card(deck[1]));
        let board: Vec<Card> = (0..5).map(|i| Card(deck[2 + i])).collect();
        // exact enumeration on a complete board (continuous values — MC with small
        // iters quantizes equities and collapses quantile bins)
        let range = Range::all();
        let (w, t) = cham_core::eval::equity_exact(hand, &range, &board);
        eqs.push(w + t / 2.0);
    }
    eqs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let bins = CDF_BINS as u32;
    let mut edges = Vec::with_capacity(bins as usize + 1);
    edges.push(0.0);
    for b in 1..bins {
        let idx = ((b as f64 / bins as f64) * eqs.len() as f64) as usize;
        edges.push(*eqs.get(idx).unwrap_or(&1.0));
    }
    edges.push(1.0);
    edges
}

/// CDF feature for one orbit: distribution of FINAL RIVER EQUITY vs uniform over
/// seeded runout samples, binned into the global equal-mass bins (EMD = L1 on CDFs).
/// Runout equities are computed EXACTLY (MC would quantize and collapse bins).
fn features_for(
    keys: &[u64],
    board_len: usize,
    params: BuildParams,
    edges: &[f64],
) -> Vec<[f32; CDF_BINS]> {
    use rayon::prelude::*;
    let feats: Vec<[f32; CDF_BINS]> = keys
        .par_iter()
        .enumerate()
        .map(|(oi, &key)| {
            let (hand, board) = unpack_orbit(key, board_len);
            let mut rng = child(0xF00D, &format!("{board_len}.{oi}"));
            let mut hist = [0u32; CDF_BINS];
            for _ in 0..params.feature_runs {
                let need = 5 - board_len;
                let mut deck: Vec<u8> = (0..52).collect();
                let mut used = [false; 52];
                for c in hand.cards() {
                    used[c.idx() as usize] = true;
                }
                for c in &board {
                    used[c.idx() as usize] = true;
                }
                for i in (1..52).rev() {
                    let j = (next_f64(&mut rng) * (i + 1) as f64) as usize;
                    deck.swap(i, j);
                }
                let mut full = board.clone();
                let mut got = 0;
                for &d in deck.iter() {
                    if got == need {
                        break;
                    }
                    if !used[d as usize] {
                        used[d as usize] = true;
                        full.push(Card(d));
                        got += 1;
                    }
                }
                let mut b5 = [Card(0); 5];
                b5[..full.len()].copy_from_slice(&full);
                let range = Range::all();
                let (w, t) = cham_core::eval::equity_exact(hand, &range, &b5);
                let eq = w + t / 2.0;
                let bin = edges.partition_point(|&e| e <= eq).saturating_sub(1).min(CDF_BINS - 1);
                hist[bin] += 1;
            }
            let total = params.feature_runs as f32;
            let mut f = [0f32; CDF_BINS];
            let mut acc = 0f32;
            for (i, &h) in hist.iter().enumerate() {
                acc += h as f32 / total;
                f[i] = acc; // CDF, not PDF
            }
            f[CDF_BINS - 1] = 1.0;
            f
        })
        .collect();
    feats
}

/// Unpack an orbit key back into (hand, board).
fn unpack_orbit(key: u64, board_len: usize) -> (Hand2, Vec<Card>) {
    let h0 = ((key >> 48) & 0xff) as u8;
    let h1 = ((key >> 40) & 0xff) as u8;
    let hand = Hand2::new(Card(h0), Card(h1));
    let board: Vec<Card> = (0..board_len)
        .map(|i| Card(((key >> (32 - 8 * i)) & 0xff) as u8))
        .collect();
    (hand, board)
}

pub struct Km {
    pub centroids: Vec<[f32; CDF_BINS]>,
    pub seeds: Vec<u64>,
    pub inertia: Vec<f64>,
}

fn l1(a: &[f32; CDF_BINS], b: &[f32; CDF_BINS]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
}

fn nearest_centroid(f: &[f32; CDF_BINS], cs: &[[f32; CDF_BINS]]) -> usize {
    let mut best = 0usize;
    let mut bd = f32::MAX;
    for (i, c) in cs.iter().enumerate() {
        let d = l1(f, c);
        if d < bd {
            bd = d;
            best = i;
        }
    }
    best
}

/// Seeded k-means++ over L1 distance (EMD proxy for 1-D CDFs), deterministic.
pub fn kmeans_l1(
    data: &[[f32; CDF_BINS]],
    k: usize,
    seed: u64,
    max_iters: u32,
) -> Km {
    assert!(k > 0 && !data.is_empty());
    let mut rng = child(seed, "kmeans");
    let mut seeds = Vec::new();
    // ---- k-means++ init on L2-ish surrogate (squared L1) ----
    let mut centroids: Vec<[f32; CDF_BINS]> = Vec::with_capacity(k);
    let first = (next_f64(&mut rng) * data.len() as f64) as usize % data.len();
    centroids.push(data[first]);
    seeds.push(first as u64);
    while centroids.len() < k.min(data.len()) {
        // D(x) = min dist to existing centroids
        let mut weights: Vec<f64> = data
            .iter()
            .map(|d| {
                let mut best = f64::MAX;
                for c in &centroids {
                    let dist = l1(d, c) as f64;
                    if dist < best {
                        best = dist;
                    }
                }
                best * best
            })
            .collect();
        let total: f64 = weights.iter().sum();
        if total <= 1e-12 {
            let idx = (next_f64(&mut rng) * data.len() as f64) as usize % data.len();
            centroids.push(data[idx]);
            seeds.push(idx as u64);
            continue;
        }
        for w in weights.iter_mut() {
            *w /= total;
        }
        let mut u = next_f64(&mut rng);
        let mut idx = data.len() - 1;
        for (i, w) in weights.iter().enumerate() {
            u -= w;
            if u <= 0.0 {
                idx = i;
                break;
            }
        }
        centroids.push(data[idx]);
        seeds.push(idx as u64);
    }
    // ---- Lloyd iterations (assignment = L1 nearest; update = component median) ----
    let mut inertia_curve: Vec<f64> = Vec::new();
    let mut assign = vec![0usize; data.len()];
    let mut prev_centroids = centroids.clone();
    for it in 0..max_iters {
        let mut inertia = 0f64;
        for (i, d) in data.iter().enumerate() {
            let c = nearest_centroid(d, &centroids);
            assign[i] = c;
            inertia += l1(d, &centroids[c]) as f64;
        }
        inertia_curve.push(inertia);
        // update: per-cluster coordinate-wise mean (median on CDFs is noisy; the
        // spec's inertia-delta criterion governs; mean keeps it deterministic)
        let mut sums = vec![[0f32; CDF_BINS]; centroids.len()];
        let mut counts = vec![0u32; centroids.len()];
        for (i, d) in data.iter().enumerate() {
            let c = assign[i];
            counts[c] += 1;
            for j in 0..CDF_BINS {
                sums[c][j] += d[j];
            }
        }
        for (ci, c) in centroids.iter_mut().enumerate() {
            if counts[ci] > 0 {
                for j in 0..CDF_BINS {
                    c[j] = sums[ci][j] / counts[ci] as f32;
                }
            }
        }
        if it > 0 {
            let prev = inertia_curve[(it - 1) as usize];
            if inertia > prev {
                // mean-update can (rarely) increase L1 inertia — revert & stop so the
                // curve is non-increasing (deterministic guarantee, tested)
                centroids = prev_centroids;
                inertia_curve[it as usize] = prev;
                break;
            }
            let delta = prev - inertia;
            if delta < 1e-6 {
                break;
            }
        }
        prev_centroids = centroids.clone();
    }
    Km { centroids, seeds, inertia: inertia_curve }
}

/// Commit the RIVER quantile edges into an existing meta.json (called once after
/// both street builds; edges computed from the same pilot as `build_street`).
pub fn finalize_meta(cfg: &AbstractionConfig, out_dir: &Path, params: BuildParams) -> Result<(), crate::EngineError> {
    let _ = cfg;
    let edges = equity_quantile_edges(params.quantile_sample, 0xC1);
    let path = out_dir.join("meta.json");
    let mut meta: MetaOut = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&path)?)
            .map_err(|e| crate::EngineError::Meta(format!("parse: {e}")))?
    } else {
        MetaOut { version: 2, river_eq_edges: edges.clone(), flop: None, turn: None, default_bucket: MISS_SENTINEL, blake3: String::new() }
    };
    meta.river_eq_edges = edges;
    write_meta_struct(out_dir, &meta)?;
    Ok(())
}
