//! `chameleon play` (SPECS/09 §2): interactive heads-up against the loaded agent.
//! Terminal I/O only — zero business logic: all policy lives in cham-agent, all
//! rules in cham-core. Uses the Slumbot-style short action encoding
//! (k check/call, c call, f fold, b<to>/r<to>, j jam, q quit).

use std::io::{BufRead, Write};

use cham_agent::modes::{AgentMode, SearchCfg};
use cham_agent::pipeline::ChameleonAgent;
use cham_core::card::Deck;
use cham_core::engine::config::EngineConfig;
use cham_core::engine::history::HandHistory;
use cham_core::engine::{Action, State, Street};
use cham_core::obs::{Agent as _, Observables, Player};
use cham_core::rng::{child, rng_from_seed};
use cham_router::model::SoftmaxModel;

/// Hydrate the persistent river cache at session start; save it back on
/// any clean exit (including a `return EXIT_OK`). Deliberately best-effort:
/// a missing/old/bad cache file just means this session starts cold, which
/// is what happens today. Never blocks the live path.
struct CachePersist {
    path: std::path::PathBuf,
}
impl CachePersist {
    fn hydrate(path: std::path::PathBuf) -> Self {
        match cham_search::cache_persist::hydrate_from(&path) {
            Ok(0) => {} // fresh session or empty file — nothing to log
            Ok(n) => println!("play: cache hydrated ({n} subgames)"),
            Err(e) => eprintln!("play: cache hydrate skipped ({e})"),
        }
        CachePersist { path }
    }
}
impl Drop for CachePersist {
    fn drop(&mut self) {
        match cham_search::cache_persist::save_to(&self.path) {
            Ok(n) => println!("play: cache saved ({n} subgames → {})", self.path.display()),
            Err(e) => eprintln!("play: cache save skipped ({e})"),
        }
    }
}

const PLAY_SEED: u64 = 0x0BEA;

pub fn run(agent: &str, depth: i64, search_warmstart: bool) -> i32 {
    // B6: opt-in solver warm-start (default OFF — the flag-off path is
    // bit-identical to the historical solver; see the oracle validation gate
    // `warmstart_oracle_validation` in cham-search).
    cham_search::solve::set_warm_start(search_warmstart);
    if search_warmstart {
        println!("play: solver warm-start ON (opt-in, validated by warmstart_oracle_validation)");
    }
    // B-2: hydrate persistent river-subgame cache at session start; the
    // guard saves it on any clean return. Path is stable per process;
    // concurrent sessions overwrite last-writer-wins (acceptable: the
    // cache is a speed optimization, not a correctness input).
    let _cache_guard = CachePersist::hydrate(std::path::PathBuf::from("artifacts/river-cache.bin"));
    let bundle = std::path::Path::new("artifacts/agent");
    // CLI mode names → AgentMode routing strings (SPECS/07 §3 canonical set)
    let routing = match agent {
        "full" | "no-search" | "full-no-search" => "mixture",
        "argmax" => "argmax",
        "robust-only" => "robust-only",
        "bayes" => "bayes",
        other => other,
    };
    let loaded = match cham_agent::loader::load_agent(bundle, routing, depth) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("play: artifact bundle under artifacts/agent not loadable: {e}");
            eprintln!("play: assemble it with train-buckets + train-bp (robust + 4 experts) first");
            return crate::cmd::EXIT_BUDGET;
        }
    };
    let mode = AgentMode {
        routing: routing.to_string(),
        search: SearchCfg {
            enabled: false,
            solver: "Rnr".into(),
            g4_ledger_ref: String::new(),
        },
    };
    // router: use the trained model when present; otherwise a deterministic
    // zero-initialized model (live-play convenience, recorded in the load card)
    let router = match std::fs::read(bundle.join("router.bin")) {
        Ok(bytes) => match cham_router::runtime::RouterRuntime::from_model_bytes(&bytes) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("play: router model: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        },
        Err(_) => {
            cham_router::runtime::RouterRuntime::new(SoftmaxModel::new(20, 4), 0.7, 0.3, 0.5, -1.5)
        }
    };
    let mut bot = match ChameleonAgent::new(
        mode,
        loaded.encoder,
        router,
        loaded.experts,
        loaded.robust,
        loaded.bayes,
        None,
    ) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("play: agent: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    println!(
        "play: heads-up vs '{}' at {depth} bb — you are seat 0 (button/SB first hand).",
        bot.name()
    );
    println!("play: actions: k (check/call) c (call) f (fold) b<to> r<to> j (jam) q (quit)");
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut session_rng = rng_from_seed(PLAY_SEED);
    for hand in 0u64.. {
        let deal_rng = &mut child(PLAY_SEED, &format!("hand{hand}"));
        let deck = Deck::shuffled(deal_rng);
        let mut state = match State::new(EngineConfig::depth(depth), deck) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("play: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        };
        let mut log: Vec<(Street, Player, Action)> = Vec::new();
        let mut guard = 0;
        while !state.is_terminal() {
            guard += 1;
            if guard > 400 {
                eprintln!("play: hand did not terminate — aborting session");
                return crate::cmd::EXIT_FAIL;
            }
            let seat = state.to_act();
            let obs = Observables::view(&state, Player::from_usize(seat));
            let a = if seat == 0 {
                match prompt_action(&obs, &mut lines) {
                    Prompt::Action(a) => a,
                    Prompt::Quit => return crate::cmd::EXIT_OK,
                    Prompt::Eof => return crate::cmd::EXIT_OK,
                }
            } else {
                bot.act(&obs, &mut session_rng)
            };
            log.push((state.street(), Player::from_usize(seat), a));
            if let Err(e) = state.apply(a) {
                eprintln!("play: {e}");
                return crate::cmd::EXIT_FAIL;
            }
        }
        let nets = state.payoffs();
        let board = state.board();
        let n = state.board_len() as usize;
        let board_s: Vec<String> = board[..n].iter().map(|c| c.to_str()).collect();
        let hh = HandHistory {
            seed: PLAY_SEED ^ hand,
            actions: log,
            cfg: state.cfg(),
            holes: [state.hole(0), state.hole(1)],
            board: *state.board(),
            board_len: state.board_len(),
            result_sb: nets[0],
        };
        // the bot sees ONLY the public history (I9 leak discipline, SPECS/01 §5)
        let ph = cham_core::engine::history::PublicHistory::from(&hh);
        bot.on_hand_end(&ph, nets[1]);
        println!(
            "hand {}: board [{}] nets SB {:+} / BB {:+} chips (stacks [{}, {}])",
            hand + 1,
            board_s.join(" "),
            nets[0],
            nets[1],
            state.stacks()[0],
            state.stacks()[1]
        );
    }
    crate::cmd::EXIT_OK
}

enum Prompt {
    Action(Action),
    Quit,
    Eof,
}

/// Read one legal action from the terminal; illegal input re-prompts
/// (contractual UX, SPECS/09 `play_prompt_roundtrip`).
fn prompt_action(
    obs: &Observables<'_>,
    lines: &mut std::io::Lines<std::io::StdinLock<'_>>,
) -> Prompt {
    loop {
        let legal_s: Vec<String> = obs.legal.iter().map(|la| la.action.to_str()).collect();
        print!(
            "\nstreet {} | pot {} | to_call {} | stack {} | legal [{}] > ",
            obs.street.as_u8(),
            obs.pot,
            obs.to_call,
            obs.stack,
            legal_s.join(" ")
        );
        let _ = std::io::stdout().flush();
        let Some(line) = lines.next() else {
            println!();
            return Prompt::Eof;
        };
        let input = line.unwrap_or_default().trim().to_lowercase();
        if input == "q" || input == "quit" {
            return Prompt::Quit;
        }
        // Slumbot convention: "k" means check when nothing to call, call otherwise
        let action = match input.as_str() {
            "k" if obs.to_call > 0 => Some(Action::Call),
            "j" => obs.legal.iter().find(|la| la.is_all_in).map(|la| la.action),
            other => Action::parse(other),
        };
        let Some(a) = action else {
            println!("  ? unparsable '{input}' — use k/c/f/b<to>/r<to>/j/q");
            continue;
        };
        if obs.legal.iter().any(|la| la.action == a) {
            return Prompt::Action(a);
        }
        println!(
            "  ? '{input}' not legal here — legal: [{}]",
            legal_s.join(" ")
        );
    }
}
