//! Append-only ledger (SPECS/08 §7): ledger.jsonl with a baseline pointer.
//! Corruption = stop (never truncate).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::EvalError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub ts: u64,
    pub run: String,
    #[serde(rename = "type")]
    pub kind: String, // ab | ladder | slumbot | probe
    pub a: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_mb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci: Option<(f64, f64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprt: Option<String>,
    pub promote: bool,
    pub seatings: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn open(dir: &Path) -> Result<Ledger, EvalError> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("ledger.jsonl");
        // corruption check: every existing line must parse
        if path.exists() {
            let f = std::fs::File::open(&path)?;
            for (i, line) in BufReader::new(f).lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                serde_json::from_str::<LedgerEntry>(&line).map_err(|e| {
                    EvalError::Ledger(format!("corrupt ledger line {i}: {e} (stopping; never truncates)"))
                })?;
            }
        }
        Ok(Ledger { path })
    }

    /// Append one entry (never rewrite; corruption stops the run upstream).
    pub fn append(&mut self, entry: &LedgerEntry) -> Result<(), EvalError> {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        f.write_all(line.as_bytes())?;
        f.sync_data()?;
        Ok(())
    }

    /// All entries (ordered).
    pub fn entries(&self) -> Result<Vec<LedgerEntry>, EvalError> {
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let f = std::fs::File::open(&self.path)?;
        let mut out = vec![];
        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            out.push(serde_json::from_str(&line)?);
        }
        Ok(out)
    }
}
