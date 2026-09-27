//! `OpponentSpec` parse/build/id round-trips (SPECS/03 §5–6).
//!
//! id grammar:
//! - `arch:<nit|tag|lag|station>` and `jitter:<arch>@<seed>`
//! - `callbot`, `raisebot`, `jamfix`, `random`, `fish`
//! - `pnash:<tilt>:<delta>` (strategy source injected separately by cham-eval)
//! - `famB:<arch>`
//! - `noisy:<eps>:<inner-id>`
//! - `switch:<inner-a>-><inner-b>@<hand>`
use crate::baselines::{CallBot, FishBot, JamBot, RaiseBot, RandomBot};
use crate::drift::SwitcherBot;
use crate::family_b::FamilyBAgent;
use crate::frozen::{FrozenAgent, FrozenRows};
use crate::noisy::NoisyAgent;
use crate::params::ArchetypeId;
use crate::percentile::PercentileChart;
use crate::perturbed::{PerturbedNashAgent, StrategySource, Tilt};
use crate::{OpponentsError, archetype::ArchetypeAgent};

/// Serialized form (id strings) — OpponentSpec itself is not serde-derivable
/// because Perturbed carries a runtime strategy closure.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OpponentSpecDto(pub String);

impl OpponentSpec {
    pub fn to_dto(&self) -> OpponentSpecDto {
        OpponentSpecDto(self.id())
    }
    pub fn from_dto(dto: &OpponentSpecDto) -> Result<OpponentSpec, crate::OpponentsError> {
        OpponentSpec::parse(&dto.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum OpponentSpec {
    Arch(ArchetypeId),
    Jitter(ArchetypeId, u64),
    CallBot,
    RaiseBot,
    JamBot,
    RandomBot,
    FishBot,
    /// Tilt + δ; the strategy source is injected at build time via
    /// [`build_with_source`] (blueprint-backed) or defaults to uniform.
    Perturbed {
        tilt: Tilt,
        delta: f64,
    },
    FamilyB(ArchetypeId),
    Noisy {
        inner: Box<OpponentSpec>,
        epsilon: f64,
    },
    Switcher {
        a: Box<OpponentSpec>,
        b: Box<OpponentSpec>,
        switch_at: u64,
    },
    /// Frozen snapshot analytic opponent (v3 §6, M6): `frozen:<label>` where
    /// label identifies the snapshot run (e.g. `robust-seed7`). The strategy
    /// rows are injected at build time via [`build_frozen`] (same DAG-safe
    /// injection shape as `Perturbed`'s strategy source — the artifact never
    /// enters this crate). Family `SELF`: never a router training/tuning
    /// source, never a promotion gate — diagnostic self-measurement only.
    Frozen {
        label: String,
    },
}

impl OpponentSpec {
    /// Parse an id string into a spec.
    pub fn parse(id: &str) -> Result<OpponentSpec, OpponentsError> {
        let id = id.trim();
        if let Some(rest) = id.strip_prefix("arch:") {
            let arch = ArchetypeId::parse(rest)
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            return Ok(OpponentSpec::Arch(arch));
        }
        if let Some(rest) = id.strip_prefix("jitter:") {
            let (arch, seed) = rest
                .split_once('@')
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            let arch = ArchetypeId::parse(arch)
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            let seed: u64 = seed
                .parse()
                .map_err(|_| OpponentsError::UnknownId(id.to_string()))?;
            return Ok(OpponentSpec::Jitter(arch, seed));
        }
        match id {
            "callbot" => return Ok(OpponentSpec::CallBot),
            "raisebot" => return Ok(OpponentSpec::RaiseBot),
            "jamfix" => return Ok(OpponentSpec::JamBot),
            "random" => return Ok(OpponentSpec::RandomBot),
            "fish" => return Ok(OpponentSpec::FishBot),
            _ => {}
        }
        if let Some(rest) = id.strip_prefix("pnash:") {
            let mut parts = rest.split(':');
            let tilt = Tilt::parse(parts.next().unwrap_or(""))
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            let delta: f64 = parts
                .next()
                .unwrap_or("0.15")
                .parse()
                .map_err(|_| OpponentsError::UnknownId(id.to_string()))?;
            return Ok(OpponentSpec::Perturbed { tilt, delta });
        }
        if let Some(rest) = id.strip_prefix("famB:") {
            let arch = ArchetypeId::parse(rest)
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            return Ok(OpponentSpec::FamilyB(arch));
        }
        if let Some(rest) = id.strip_prefix("noisy:") {
            let (eps, inner_id) = rest
                .split_once(':')
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            let epsilon: f64 = eps
                .parse()
                .map_err(|_| OpponentsError::UnknownId(id.to_string()))?;
            let inner = OpponentSpec::parse(inner_id)?;
            return Ok(OpponentSpec::Noisy {
                inner: Box::new(inner),
                epsilon,
            });
        }
        if let Some(label) = id.strip_prefix("frozen:") {
            if label.is_empty() {
                return Err(OpponentsError::UnknownId(id.to_string()));
            }
            return Ok(OpponentSpec::Frozen {
                label: label.to_string(),
            });
        }
        if let Some(rest) = id.strip_prefix("switch:") {
            let (ab, hand) = rest
                .rsplit_once('@')
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            let (a, b) = ab
                .split_once("->")
                .ok_or_else(|| OpponentsError::UnknownId(id.to_string()))?;
            let switch_at: u64 = hand
                .parse()
                .map_err(|_| OpponentsError::UnknownId(id.to_string()))?;
            return Ok(OpponentSpec::Switcher {
                a: Box::new(OpponentSpec::parse(a)?),
                b: Box::new(OpponentSpec::parse(b)?),
                switch_at,
            });
        }
        Err(OpponentsError::UnknownId(id.to_string()))
    }

    /// Canonical id string (round-trips through [`OpponentSpec::parse`]).
    pub fn id(&self) -> String {
        match self {
            OpponentSpec::Arch(a) => format!("arch:{}", a.as_str()),
            OpponentSpec::Jitter(a, seed) => format!("jitter:{}@{}", a.as_str(), seed),
            OpponentSpec::CallBot => "callbot".into(),
            OpponentSpec::RaiseBot => "raisebot".into(),
            OpponentSpec::JamBot => "jamfix".into(),
            OpponentSpec::RandomBot => "random".into(),
            OpponentSpec::FishBot => "fish".into(),
            OpponentSpec::Perturbed { tilt, delta } => format!("pnash:{}:{}", tilt.as_str(), delta),
            OpponentSpec::FamilyB(a) => format!("famB:{}", a.as_str()),
            OpponentSpec::Noisy { inner, epsilon } => format!("noisy:{}:{}", epsilon, inner.id()),
            OpponentSpec::Switcher { a, b, switch_at } => {
                format!("switch:{}->{}@{}", a.id(), b.id(), switch_at)
            }
            OpponentSpec::Frozen { label } => format!("frozen:{label}"),
        }
    }

    /// The opponent FAMILY label (SPECS/03 §6): A = in-family scripts,
    /// B/PN/noise = out-of-family (never a router training or tuning source).
    pub fn family(&self) -> &'static str {
        match self {
            OpponentSpec::Arch(_) | OpponentSpec::Jitter(..) => "A",
            OpponentSpec::CallBot
            | OpponentSpec::RaiseBot
            | OpponentSpec::JamBot
            | OpponentSpec::RandomBot
            | OpponentSpec::FishBot => "A",
            OpponentSpec::Perturbed { .. } => "PN",
            OpponentSpec::FamilyB(_) => "B",
            OpponentSpec::Noisy { .. } => "noise",
            // L-17 fix (2026-09-27): delegate to the inner specs. The previous
            // `Switcher { .. } => "A"` labeled `switch:famB:...` as family A,
            // defeating the "never accidentally in-family" guarantee: a
            // switcher that spends most of its hands in family B was
            // presented to the router as a family-A opponent.
            //
            // Rule: if both inner families agree, use it. If they differ,
            // report a "mixed" family so the caller cannot treat the
            // switcher as a clean member of either group.
            OpponentSpec::Switcher { a, b, .. } => {
                let fa = a.family();
                let fb = b.family();
                if fa == fb {
                    fa
                } else {
                    "mixed"
                }
            }
            // SELF: our own frozen snapshot — excluded from router training,
            // tuning, and promotion gates (diagnostic self-measurement only).
            OpponentSpec::Frozen { .. } => "SELF",
        }
    }
}

/// Build an agent from a spec (uniform strategy source for perturbed bots).
pub fn build(spec: &OpponentSpec, chart: &'static PercentileChart) -> Box<dyn Agent> {
    build_with_source(spec, chart, None)
}

/// Build with an injected strategy source for `Perturbed` (cham-eval passes the
/// blueprint-backed closure here, keeping the crate DAG acyclic).
pub fn build_with_source(
    spec: &OpponentSpec,
    chart: &'static PercentileChart,
    source: Option<StrategySource>,
) -> Box<dyn Agent> {
    match spec {
        OpponentSpec::Arch(a) => Box::new(ArchetypeAgent::point(*a, chart)),
        OpponentSpec::Jitter(a, seed) => Box::new(ArchetypeAgent::jittered(*a, *seed, chart)),
        OpponentSpec::CallBot => Box::new(CallBot),
        OpponentSpec::RaiseBot => Box::new(RaiseBot),
        OpponentSpec::JamBot => Box::new(JamBot),
        OpponentSpec::RandomBot => Box::new(RandomBot),
        OpponentSpec::FishBot => Box::new(FishBot),
        OpponentSpec::Perturbed { tilt, delta } => {
            let mut p = PerturbedNashAgent::new(*tilt, *delta);
            if let Some(src) = source {
                p.set_source(src);
            }
            Box::new(p)
        }
        OpponentSpec::FamilyB(a) => Box::new(FamilyBAgent::new(*a, chart)),
        OpponentSpec::Noisy { inner, epsilon } => {
            let inner_agent = build_with_source(inner, chart, source);
            Box::new(NoisyAgent::new(inner_agent, *epsilon))
        }
        OpponentSpec::Switcher { a, b, switch_at } => Box::new(SwitcherBot::new(
            (**a).clone(),
            (**b).clone(),
            *switch_at,
            chart,
        )),
        // Registry hit (EXP-016 shadow gauntlet registered real snapshot
        // rows under this label) → full-fidelity frozen opponent. Miss →
        // uniform diagnostic (loudly questionable: use `build_frozen` with
        // real snapshot rows for any meaningful number).
        OpponentSpec::Frozen { label } => match crate::frozen::registered_shadow(label) {
            Some(entry) => Box::new(crate::frozen::build_registered(label.clone(), &entry)),
            None => Box::new(FrozenAgent::new(
                label.clone(),
                cham_engine::Encoder::cfg_only(cham_engine::config::AbstractionConfig::tiny())
                    .expect("tiny encoder"),
                FrozenRows::default(),
            )),
        },
    }
}

/// Build a `Frozen` opponent with real snapshot rows (v3 §6, M6). The
/// `encoder` must match the victim snapshot's abstraction EXACTLY (same
/// buckets dir + config — keys are encoder-content-addressed); `rows` come
/// from `BlueprintPolicy::export_rows`. Mismatched encoders surface as a high
/// `miss_rate()`, not silent wrongness.
pub fn build_frozen(
    spec: &OpponentSpec,
    chart: &'static PercentileChart,
    encoder: cham_engine::Encoder,
    rows: FrozenRows,
) -> Box<dyn Agent> {
    match spec {
        OpponentSpec::Frozen { label } => {
            let _ = chart;
            Box::new(FrozenAgent::new(label.clone(), encoder, rows))
        }
        other => build(other, chart),
    }
}

use cham_core::obs::Agent;
