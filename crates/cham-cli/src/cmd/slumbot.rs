//! `chameleon slumbot` (SPECS/09 §2): the 200bb anchor — **diagnostic only** (G8);
//! it can never gate a promotion (SPECS/08 §5 honesty rules: σ ≥ 10 bb/seating at
//! 200bb ⇒ ±280 mb at 5k seatings). Default runs the offline mock (tests never
//! touch the network); `--real` requires `--yes-i-am-live` — the verify-first gate.

pub fn run(seatings: u64, real: bool, yes_i_am_live: bool, resume: Option<&str>) -> i32 {
    if real && !yes_i_am_live {
        eprintln!(
            "slumbot: refusing a live run without --yes-i-am-live \
             (verify-first gate, SPECS/08 §5 — diff the dialect fixture first)"
        );
        return crate::cmd::EXIT_BUDGET;
    }
    if let Some(path) = resume {
        println!("slumbot: resuming session log from {path}");
    }
    // Serial, rate-limited actions in the published dialect: k/c/f/b<amount>.
    let actions = ["k", "c", "b100", "f"];
    let outcome = if real {
        let mut client = cham_eval::slumbot::SlumbotClient::new("https://slumbot.com".to_string());
        cham_eval::slumbot::run_session(&mut client, seatings, &actions)
    } else {
        // Mock mode caps at 50 hands: the mock exists so tests/CI never hit the network.
        let mut mock = cham_eval::slumbot::MockSlumbot {
            hands: 0,
            logged_in: false,
            requests: Vec::new(),
        };
        cham_eval::slumbot::run_session(&mut mock, seatings.min(50), &actions)
    };
    match outcome {
        Ok(s) => {
            println!(
                "slumbot: hands {} errored {} winnings {:+.2} bb — diagnostic only",
                s.hands_played, s.errored_hands, s.winnings_bb
            );
            crate::cmd::EXIT_OK
        }
        Err(e) => {
            eprintln!("slumbot: {e}");
            crate::cmd::EXIT_FAIL
        }
    }
}
