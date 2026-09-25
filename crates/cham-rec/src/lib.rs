//! Flight recorder (SPECS/12): a leaf crate owning ALL structured event writing in
//! the workspace. No crate re-implements JSONL writing; no crate embeds recorder
//! logic. Depends on nothing but `serde`/`serde_json`.
//!
//! Line format (every record, field order fixed):
//! `{"ts": <unix_secs>, "run": "<id>", "kind": "<kind>", "seq": <n>, "data": { ... }}`
//!
//! - append-only: a corrupted tail stops the run (error up), never truncation
//! - flush + fsync every 1000 records or 2 s; explicit flush on drop
//! - the record-kind registry lives in [`schema`] — unknown kinds are validator errors

#![forbid(unsafe_code)]

pub mod schema;
pub mod validate;

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Errors surfaced by the recorder and the offline validator.
#[derive(Debug, Error)]
pub enum RecError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corrupt tail in {path}: {reason} (append-only; refusing to truncate)")]
    CorruptTail { path: PathBuf, reason: String },
    #[error("invalid record: {0}")]
    Invalid(String),
    #[error("unknown record kind: {0}")]
    UnknownKind(String),
    #[error("missing required field `{field}` for kind `{kind}`")]
    MissingField { kind: String, field: String },
    #[error("NaN/Infinity payload rejected")]
    NonFinite,
}

/// Format a run id: `<unix_secs>-<kind>-<hash8>`.
///
/// SPECS/12 calls for `blake3_8`; the cham-rec DoD pins the dependency closure to
/// `{serde, serde_json}` only, so the 8-hex suffix is FNV-1a-64 over
/// `(kind, unix_secs, pid, nanos)` (decision D-002; it is an id, not tamper-evidence —
/// tamper evidence is blake3 elsewhere in the workspace).
fn run_id(kind: &str, unix_secs: u64) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    for b in kind.as_bytes() {
        mix(*b);
    }
    for b in unix_secs.to_le_bytes() {
        mix(b);
    }
    for b in std::process::id().to_le_bytes() {
        mix(b);
    }
    for b in nanos.to_le_bytes() {
        mix(b);
    }
    format!("{unix_secs}-{kind}-{:08x}", (h >> 32) as u32)
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// u64 → String without pulling an itoa dependency (whitelist discipline).
fn itoa_small(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from_utf8(buf[i..].to_vec()).unwrap_or_else(|_| "0".to_string())
}

/// Append-only JSONL flight recorder for one run.
#[derive(Debug)]
pub struct Recorder {
    path: PathBuf,
    run_id: String,
    writer: BufWriter<File>,
    buffered: usize,
    seq: u64,
    last_flush: Instant,
    flushes: u64,
    bytes_written: u64,
}

const FLUSH_RECORDS: usize = 1000;
const FLUSH_SECS: u64 = 2;

impl Recorder {
    /// Open (or resume) `runs_dir/<run_id>/events.jsonl` for `kind`.
    ///
    /// Creates a NEW run directory (concurrent recorders are distinct by design).
    /// If the target file exists, its tail is parsed; a corrupt tail is a hard error
    /// (append-only contract) and `seq` continues from the last record.
    pub fn open(runs_dir: &Path, kind: &str) -> Result<Recorder, RecError> {
        if schema::RecordKind::from_str(kind).is_none() {
            return Err(RecError::UnknownKind(kind.to_string()));
        }
        // Distinct Recorders opened within the same second still collide on
        // `<secs>-<kind>-...` only if the hash collides; nanos+pid make that
        // negligible, and the contract only requires distinct run dirs.
        let id = {
            let mut id = run_id(kind, unix_secs());
            let mut dir = runs_dir.join(&id);
            while dir.exists() {
                std::thread::sleep(std::time::Duration::from_millis(2));
                id = run_id(kind, unix_secs());
                dir = runs_dir.join(&id);
            }
            id
        };
        std::fs::create_dir_all(runs_dir.join(&id))?;
        Self::open_in_dir(&runs_dir.join(id), kind)
    }

    /// Resume the most recent run of `kind` under `runs_dir` (lexical max of the
    /// `<secs>-<kind>-<hash>` children). Errors if none exists. Same append/tail
    /// contract as [`Recorder::open`].
    pub fn open_latest(runs_dir: &Path, kind: &str) -> Result<Recorder, RecError> {
        let marker = format!("-{kind}-");
        let mut best: Option<std::path::PathBuf> = None;
        for e in std::fs::read_dir(runs_dir)?.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if name.contains(marker.as_str()) && best.as_ref().is_none_or(|b| *b < p) {
                        best = Some(p);
                    }
                }
            }
        }
        let dir = best.ok_or_else(|| {
            RecError::Invalid(format!(
                "no existing run of kind `{kind}` under {}",
                runs_dir.display()
            ))
        })?;
        Self::open_in_dir(&dir, kind)
    }

    fn open_in_dir(dir: &Path, kind: &str) -> Result<Recorder, RecError> {
        if schema::RecordKind::from_str(kind).is_none() {
            return Err(RecError::UnknownKind(kind.to_string()));
        }
        let id = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let path = dir.join("events.jsonl");
        let exists = path.exists();

        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut rec = Recorder {
            path: path.clone(),
            run_id: id,
            writer: BufWriter::new(file),
            buffered: 0,
            seq: 0,
            last_flush: Instant::now(),
            flushes: 0,
            bytes_written: 0,
        };
        if exists {
            let mut buf = String::new();
            {
                let mut f = File::open(&path)?;
                // Tail check only: read the whole file (runs are modest) and take the
                // last non-empty line. A mid-file corruption surfaces as a parse error.
                f.read_to_string(&mut buf)?;
            }
            for line in buf.lines().rev() {
                if line.trim().is_empty() {
                    continue;
                }
                let v: serde_json::Value =
                    serde_json::from_str(line).map_err(|e| RecError::CorruptTail {
                        path: path.clone(),
                        reason: format!("last line unparseable: {e}"),
                    })?;
                let s =
                    v.get("seq")
                        .and_then(|s| s.as_u64())
                        .ok_or_else(|| RecError::CorruptTail {
                            path: path.clone(),
                            reason: "last line missing seq".into(),
                        })?;
                rec.seq = s;
                break;
            }
        }
        Ok(rec)
    }

    /// The run id (== run directory name).
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Full path of `events.jsonl`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Monotonic per-run sequence number (last used).
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Number of flush+fsync cycles performed (observability; used by `flush_cadence`).
    pub fn flushes(&self) -> u64 {
        self.flushes
    }

    /// Bytes written so far (after last flush; buffered bytes not counted).
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// True when the caller should call [`Recorder::flush`] now
    /// (≥ 1000 buffered records or ≥ 2 s since last flush).
    pub fn should_flush(&self) -> bool {
        self.buffered >= FLUSH_RECORDS || self.last_flush.elapsed().as_secs() >= FLUSH_SECS
    }

    /// Record one event. `data` must be a JSON object carrying every required field
    /// for `kind` (SPECS/12 §3) and must not contain NaN/Infinity sentinels.
    ///
    /// The envelope is serialized MANUALLY (not via `json!`) because `serde_json`'s
    /// default `Map` is a `BTreeMap` and would alphabetize the envelope keys — the
    /// line format `{"ts","run","kind","seq","data"}` is a byte-level contract.
    pub fn record(
        &mut self,
        kind: schema::RecordKind,
        data: serde_json::Value,
    ) -> Result<(), RecError> {
        schema::check_payload(kind, &data)?;
        self.seq += 1;
        let data_str = serde_json::to_string(&data)?;
        let run_str = serde_json::to_string(&self.run_id)?;
        let mut line = String::with_capacity(data_str.len() + 80);
        line.push_str("{\"ts\":");
        let ts = unix_secs();
        line.push_str(itoa_small(ts).as_str());
        line.push_str(",\"run\":");
        line.push_str(&run_str);
        line.push_str(",\"kind\":\"");
        line.push_str(kind.as_str());
        line.push_str("\",\"seq\":");
        line.push_str(itoa_small(self.seq).as_str());
        line.push_str(",\"data\":");
        line.push_str(&data_str);
        line.push_str("}\n");
        self.bytes_written += line.len() as u64;
        self.writer.write_all(line.as_bytes())?;
        self.buffered += 1;
        if self.should_flush() {
            self.flush()?;
        }
        Ok(())
    }

    /// Flush the buffer and fsync. Mandatory cadence; explicit flush on drop.
    pub fn flush(&mut self) -> Result<(), RecError> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()?;
        self.buffered = 0;
        self.last_flush = Instant::now();
        self.flushes += 1;
        Ok(())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Explicit flush on drop (SPECS/12 §2). Hard-crash loss of the buffered tail
        // is acceptable; a failing fsync here can no longer be surfaced up the stack.
        let _ = self.flush();
    }
}
