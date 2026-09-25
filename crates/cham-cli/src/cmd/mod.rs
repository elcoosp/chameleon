//! Command implementations: thin orchestration over the workspace crates.

pub mod ab;
pub mod collect;
pub mod dashboard;
pub mod guard;
pub mod ladder;
pub mod play;
pub mod probe;
pub mod slumbot;
pub mod trace;
pub mod train_bp;
pub mod train_buckets;
pub mod train_router;
pub mod verify;

/// Shared exit-code vocabulary.
pub const EXIT_OK: i32 = 0;
pub const EXIT_FAIL: i32 = 1;
pub const EXIT_BUDGET: i32 = 2;
