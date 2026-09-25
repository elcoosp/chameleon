//! Offline validator used by `chameleon verify` (SPECS/12): parse every record in an
//! `events.jsonl`, enforce the envelope shape, the kind registry, required payload
//! fields, finiteness and monotonic `seq`.

use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::schema::RecordKind;
use crate::RecError;

/// Validation summary for one events file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub records: u64,
    pub kinds: Vec<String>,
}

fn validate_line(line: &str) -> Result<(String, u64), RecError> {
    let v: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| RecError::Invalid(format!("unparseable record: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| RecError::Invalid("record must be an object".into()))?;
    for k in ["ts", "run", "kind", "seq", "data"] {
        if !obj.contains_key(k) {
            return Err(RecError::Invalid(format!("record missing envelope field `{k}`")));
        }
    }
    if !obj["ts"].is_u64() {
        return Err(RecError::Invalid("`ts` must be unix seconds (u64)".into()));
    }
    if !obj["run"].is_string() || obj["run"].as_str().is_some_and(str::is_empty) {
        return Err(RecError::Invalid("`run` must be a non-empty string".into()));
    }
    let kind_s = obj["kind"]
        .as_str()
        .ok_or_else(|| RecError::Invalid("`kind` must be a string".into()))?;
    let kind = RecordKind::from_str(kind_s)
        .ok_or_else(|| RecError::UnknownKind(kind_s.to_string()))?;
    let seq = obj["seq"]
        .as_u64()
        .ok_or_else(|| RecError::Invalid("`seq` must be u64".into()))?;
    crate::schema::check_payload(kind, &obj["data"])?;
    Ok((kind_s.to_string(), seq))
}

/// Validate a single `events.jsonl` file.
pub fn validate_file(path: &Path) -> Result<Summary, RecError> {
    let f = std::fs::File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut records = 0u64;
    let mut kinds: Vec<String> = Vec::new();
    let mut last_seq: Option<u64> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let (kind, seq) = validate_line(trimmed)?;
        if let Some(prev) = last_seq {
            if seq <= prev {
                return Err(RecError::Invalid(format!(
                    "seq not monotonic at record {records}: {seq} after {prev}"
                )));
            }
        }
        last_seq = Some(seq);
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
        records += 1;
    }
    Ok(Summary { records, kinds })
}

/// Validate every `runs/*/events.jsonl` under `runs_dir`. Returns per-file summaries.
pub fn validate_dir(runs_dir: &Path) -> Result<Vec<(std::path::PathBuf, Summary)>, RecError> {
    let mut out = Vec::new();
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(runs_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("events.jsonl").exists())
        .collect();
    dirs.sort();
    for d in dirs {
        let s = validate_file(&d.join("events.jsonl"))?;
        out.push((d, s));
    }
    Ok(out)
}
