//! cham-router (SPECS/05): the classifier + switching policy.
//!
//! v2 math fixes (review A6/A7):
//! - 20 opponent-type-informative features (tracker-only, hand-frozen; the
//!   circular self-confidence dims are gone)
//! - mixture weights are PER-HAND, sharpened by `w ∝ p^(1/T)` (softmax over
//!   probabilities flattens; `p^(1/0.7)` gives a certain posterior weight 1.0)
//! - the behavioral mixture is REACH-WEIGHTED: `σ_mix(a|i) ∝ Σ_k w_k·π_k(i)·σ_k(a|i)`
//!   (applied by cham-agent; the Kuhn-correct form)

#![forbid(unsafe_code)]

pub mod dataset;
pub mod features;
pub mod metrics;
pub mod model;
pub mod runtime;
pub mod train;

pub use dataset::{
    DatasetMeta, RbinRow, SESSION_A, SESSION_BDEV, SESSION_BTEST, SESSION_C, read_dataset,
    write_dataset,
};
pub use features::{FeatureContract, FeatureInputs};
pub use model::SoftmaxModel;
pub use runtime::{CHANGEPOINT_FORCE, ChangepointShield, N_EXPERTS, RouterRuntime};
pub use runtime::enable_changepoint_global;
pub use train::train_model;

use thiserror::Error;

/// Errors surfaced by the router.
#[derive(Debug, Error)]
pub enum RouterError {
    #[error("dataset: {0}")]
    Dataset(String),
    #[error("model: {0}")]
    Model(String),
    #[error("features: {0}")]
    Features(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
