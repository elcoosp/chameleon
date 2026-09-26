//! Shared river-subgame cache guard (B-2 / V2 A/B speedup).
//!
//! `CachePersist` hydrates `cham-search`'s process-global subgame cache at
//! the start of a command and saves it on clean exit via `Drop`. Used by
//! `play` (live decisions), `ladder` (batch eval; a no-op until search is
//! wired there), and `ab` (paired A/B; same).
//!
//! Best-effort: a missing or corrupt file is logged and the session starts
//! cold. Never blocks a decision path.

/// RAII guard: hydrate on construct, save on drop.
pub struct CachePersist {
    path: std::path::PathBuf,
    tag: &'static str,
}

impl CachePersist {
    pub fn hydrate(tag: &'static str, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        match cham_search::cache_persist::hydrate_from(&path) {
            Ok(0) => {} // fresh session or empty file — nothing to log
            Ok(n) => println!("{tag}: cache hydrated ({n} subgames)"),
            Err(e) => eprintln!("{tag}: cache hydrate skipped ({e})"),
        }
        CachePersist { path, tag }
    }
}

impl Drop for CachePersist {
    fn drop(&mut self) {
        match cham_search::cache_persist::save_to(&self.path) {
            Ok(n) => println!(
                "{}: cache saved ({n} subgames → {})",
                self.tag,
                self.path.display()
            ),
            Err(e) => eprintln!("{}: cache save skipped ({e})", self.tag),
        }
    }
}
