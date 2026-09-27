//! Slumbot client (SPECS/08 §5): the PUBLISHED dialect, verify-first.
//!
//! - Endpoints: POST /api/login, POST /api/new_hand, POST /api/act
//! - actions: `k` (check/call per their convention), `c`, `f`, `b<amount>`
//! - responses carry the game state and `winnings` on completed hands
//! - verify-first gate: a recorded 50-hand real session shape must match the mock
//! - client rules: serial requests, ≥ 1 s spacing, ×3 exponential backoff on 5xx,
//!   session persisted after EVERY action, errored hands counted not dropped

use serde::{Deserialize, Serialize};

use crate::EvalError;

/// The transport abstraction (mock for tests, ureq for live).
pub trait SlumbotApi: Send {
    fn login(&mut self) -> Result<String, EvalError>;
    fn new_hand(&mut self, token: &str) -> Result<String, EvalError>;
    fn act(&mut self, token: &str, action: &str) -> Result<String, EvalError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlumbotSession {
    pub token: String,
    pub hands_played: u64,
    pub errored_hands: u64,
    pub winnings_bb: f64,
    #[serde(default)]
    pub last_response: String,
}

/// Live client (rate-limited, backoff, serial).
pub struct SlumbotClient {
    pub base_url: String,
    pub min_spacing_ms: u64,
    last_request: Option<std::time::Instant>,
    /// B11: one keep-alive agent reused across ALL requests (was: a fresh
    /// agent per call — a new TCP+TLS handshake on every action).
    agent: ureq::Agent,
}

impl SlumbotClient {
    pub fn new(base_url: String) -> SlumbotClient {
        SlumbotClient {
            base_url,
            min_spacing_ms: 1000,
            last_request: None,
            agent: ureq::Agent::new(),
        }
    }

    fn throttle(&mut self) {
        if let Some(t) = self.last_request {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < self.min_spacing_ms {
                std::thread::sleep(std::time::Duration::from_millis(
                    self.min_spacing_ms - elapsed,
                ));
            }
        }
        self.last_request = Some(std::time::Instant::now());
    }

    fn post(&mut self, path: &str, body: &str) -> Result<String, EvalError> {
        self.throttle();
        // ×3 exponential backoff on 5xx / network errors
        let mut delay = 500u64;
        for _ in 0..3 {
            let url = format!("{}{}", self.base_url, path);
            match self.agent.post(&url).send_string(body) {
                Ok(resp) => {
                    let mut text = String::new();
                    use std::io::Read;
                    resp.into_reader().read_to_string(&mut text)?;
                    return Ok(text);
                }
                Err(ureq::Error::Status(code, _)) if code >= 500 => {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    delay *= 3;
                }
                Err(e) => {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    delay *= 3;
                    let _ = e;
                }
            }
        }
        Err(EvalError::Slumbot(format!(
            "POST {path} failed after backoff"
        )))
    }
}

impl SlumbotApi for SlumbotClient {
    fn login(&mut self) -> Result<String, EvalError> {
        // M-13 fix (2026-09-27): the previous body hardcoded empty
        // credentials, so `--real` could only fail. Now they come from env
        // (`SLUMBOT_USER`, `SLUMBOT_PASS`) with JSON-escaped values; the
        // error path is a clean refusal, not a silently-swallowed 4xx.
        let user = std::env::var("SLUMBOT_USER").unwrap_or_default();
        let pass = std::env::var("SLUMBOT_PASS").unwrap_or_default();
        if user.is_empty() || pass.is_empty() {
            return Err(EvalError::Slumbot(
                "SLUMBOT_USER / SLUMBOT_PASS not set — refusing a live login \
                 with empty credentials"
                    .into(),
            ));
        }
        let esc = |s: &str| -> String {
            let mut o = String::with_capacity(s.len() + 2);
            for ch in s.chars() {
                match ch {
                    '"' => o.push_str("\\\""),
                    '\\' => o.push_str("\\\\"),
                    '\n' => o.push_str("\\n"),
                    '\r' => o.push_str("\\r"),
                    '\t' => o.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        o.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => o.push(c),
                }
            }
            o
        };
        let body = format!(
            "{{\"username\": \"{}\", \"password\": \"{}\"}}",
            esc(&user),
            esc(&pass)
        );
        self.post("/api/login", &body)
    }
    fn new_hand(&mut self, token: &str) -> Result<String, EvalError> {
        self.post("/api/new_hand", &format!("{{\"token\": \"{token}\"}}"))
    }
    fn act(&mut self, token: &str, action: &str) -> Result<String, EvalError> {
        self.post(
            "/api/act",
            &format!("{{\"token\": \"{token}\", \"action\": \"{action}\"}}"),
        )
    }
}

/// Mock server implementing the same dialect (tests never touch the network).
#[derive(Default)]
pub struct MockSlumbot {
    pub hands: u64,
    pub logged_in: bool,
    pub requests: Vec<(String, String)>,
}

impl SlumbotApi for MockSlumbot {
    fn login(&mut self) -> Result<String, EvalError> {
        self.requests.push(("POST /api/login".into(), "{}".into()));
        self.logged_in = true;
        Ok(r#"{"token": "mock-token-1"}"#.into())
    }
    fn new_hand(&mut self, token: &str) -> Result<String, EvalError> {
        self.requests
            .push(("POST /api/new_hand".into(), token.into()));
        self.hands += 1;
        Ok(r#"{"actions": [], "hole_cards": "AsKd", "winnings": 0, "debug_hash": 0, "in_progress": true}"#.into())
    }
    fn act(&mut self, token: &str, action: &str) -> Result<String, EvalError> {
        self.requests
            .push(("POST /api/act".into(), format!("{token} {action}")));
        if action == "f" && self.hands % 3 == 0 {
            return Ok(r#"{"actions": ["f"], "winnings": -50, "in_progress": false}"#.into());
        }
        Ok(r#"{"actions": ["k"], "winnings": 0, "in_progress": true}"#.into())
    }
}

/// The documented dialect sequence (committed fixture): the verify-first gate
/// diffs observed request shapes against this in order.
pub fn expected_dialect_fixture() -> Vec<String> {
    vec![
        "POST /api/login".into(),
        "POST /api/new_hand".into(),
        "POST /api/act".into(),
    ]
}

/// Bookkeeping event fired immediately after OUR request is sent (B11).
/// While awaiting the opponent's response no computation is possible (the
/// protocol is strict request/response) — so recorder flushes and tracker
/// bookkeeping happen HERE, keeping the response-handler path minimal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendEvent {
    NewHand,
    Act,
}

/// Run a session against any API: verify-first helper (50-hand real gate).
pub fn run_session(
    api: &mut dyn SlumbotApi,
    seatings: u64,
    actions: &[&str],
) -> Result<SlumbotSession, EvalError> {
    run_session_with_hook(api, seatings, actions, &mut |_| {})
}

/// Session runner with a post-send bookkeeping hook (see [`SendEvent`]).
pub fn run_session_with_hook(
    api: &mut dyn SlumbotApi,
    seatings: u64,
    actions: &[&str],
    on_after_send: &mut dyn FnMut(SendEvent),
) -> Result<SlumbotSession, EvalError> {
    let token_raw = api.login()?;
    let token = token_raw
        .split('"')
        .nth(3)
        .unwrap_or("mock-token-1")
        .to_string();
    let mut session = SlumbotSession {
        token,
        hands_played: 0,
        errored_hands: 0,
        winnings_bb: 0.0,
        last_response: String::new(),
    };
    for i in 0..seatings {
        let resp = api.new_hand(&session.token)?;
        on_after_send(SendEvent::NewHand);
        session.last_response = resp.clone();
        // act through the hand until it reports in_progress = false (capped)
        let mut steps = 0;
        let mut action_idx = i as usize % actions.len();
        loop {
            steps += 1;
            if steps > 12 {
                break;
            }
            let r = api.act(&session.token, actions[action_idx % actions.len()]);
            on_after_send(SendEvent::Act);
            action_idx += 1;
            match r {
                Ok(text) => {
                    session.last_response = text.clone();
                    if text.contains("\"in_progress\": false") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(w) = v.get("winnings").and_then(|x| x.as_f64()) {
                                session.winnings_bb += w / 100.0;
                            }
                        }
                        break;
                    }
                }
                Err(e) => {
                    session.errored_hands += 1;
                    let _ = e;
                    break;
                }
            }
        }
        session.hands_played += 1;
    }
    Ok(session)
}
