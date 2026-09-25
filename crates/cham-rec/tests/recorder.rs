use cham_rec::schema::RecordKind;
use cham_rec::{RecError, Recorder, validate};

fn tmpdir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(tag)
        .tempdir()
        .expect("tempdir")
}

fn sample_data() -> serde_json::Value {
    serde_json::json!({
        "hand_idx": 17, "street": 2, "slot": 3, "action": "Bet:450",
        "weights_frozen": [0.05, 0.62, 0.18, 0.15, 0.0],
        "expert_visits": [312, 1024, 61, 0],
        "fallback_used": true,
        "abstraction_hash": "1a2b3c4d5e6f7081"
    })
}

#[test]
fn line_format_stable() {
    let dir = tmpdir("rec-format");
    let mut rec = Recorder::open(dir.path(), "decision").expect("open");
    rec.record(RecordKind::Decision, sample_data())
        .expect("record");
    rec.flush().expect("flush");
    let text = std::fs::read_to_string(rec.path()).expect("read");
    let line = text.lines().next().expect("one line");
    let v: serde_json::Value = serde_json::from_str(line).expect("json");
    // Envelope shape is contractual at the BYTE level: field order is pinned.
    // (Parsed `Value` maps are BTreeMaps — they cannot express order.)
    let pos = |needle: &str| line.find(needle).expect("needle present");
    assert!(
        pos("\"ts\"") < pos("\"run\"")
            && pos("\"run\"") < pos("\"kind\"")
            && pos("\"kind\"") < pos("\"seq\"")
            && pos("\"seq\"") < pos("\"data\"")
    );
    assert!(line.starts_with("{\"ts\":"));
    assert_eq!(v["kind"], "decision");
    assert_eq!(v["seq"], 1);
    assert!(v["run"].as_str().expect("run id").contains("decision"));
    // data round-trips verbatim
    assert_eq!(v["data"], sample_data());
    // One record of each kind serializes and validates.
    let all = [
        (
            RecordKind::OppSession,
            serde_json::json!({"spec_id":"arch:tag","family":"A","arch":"tag","seed":7,"params":{}}),
        ),
        (
            RecordKind::Match,
            serde_json::json!({"label":"m","spec_ids":["a"],"seeds":[1],"deals":10,"seatings":20,"mb_per_seating":1.0,"se_mb":0.5,"vr_factor":1.0,"wall_s":0.1}),
        ),
        (
            RecordKind::BpSnapshot,
            serde_json::json!({"iters":100,"infosets":10,"bytes":1,"wall_s":1.0,"thread_mode":"deterministic","threads":1}),
        ),
        (
            RecordKind::BpProbe,
            serde_json::json!({"lbr_mb":12.0,"coverage":0.9,"iters":100}),
        ),
        (
            RecordKind::WarmstartStep,
            serde_json::json!({"src_artifact":"a.bin","depth_bb":100,"keys_transferred":5}),
        ),
        (
            RecordKind::RouterTrain,
            serde_json::json!({"rows":100,"top1_b_dev":0.8,"top1_b_test":0.8,"ece_b_test":0.1,"ece_family_c":0.1,"per_class_recall":[0.8,0.8,0.8,0.8],"gates_passed":true}),
        ),
        (
            RecordKind::SearchDecision,
            serde_json::json!({"triggered":true,"solver":"Rnr0.9","iters":400,"truncated":false,"lbr_gap":[0.03,0.05]}),
        ),
        (
            RecordKind::AgentLoad,
            serde_json::json!({"mode":"full","artifact_hashes":{},"depth_bb":100,"experts":[]}),
        ),
        (
            RecordKind::CollectRowset,
            serde_json::json!({"rows":1,"sessions":1,"family_counts":{},"abstraction_hash":"h"}),
        ),
        (
            RecordKind::LedgerEntry,
            serde_json::json!({"type":"ab","promote":false}),
        ),
        (
            RecordKind::ProbeSummary,
            serde_json::json!({"lbr_mb":1.0,"coverage":1.0,"verdict":"PASS"}),
        ),
        (
            RecordKind::FallbackUniform,
            serde_json::json!({"hand_idx":1,"street":1,"reason":"uncovered"}),
        ),
    ];
    for (k, d) in all {
        rec.record(k, d).expect("record all kinds");
    }
    rec.flush().expect("flush");
    let summary = validate::validate_file(rec.path()).expect("validate");
    assert_eq!(summary.records, 13);
    insta::assert_snapshot!(format!("{summary:?}"), @r###"Summary { records: 13, kinds: ["decision", "opp_session", "match", "bp_snapshot", "bp_probe", "warmstart_step", "router_train", "search_decision", "agent_load", "collect_rowset", "ledger_entry", "probe_summary", "fallback_uniform"] }"###);
}

#[test]
fn append_only_fsync() {
    let dir = tmpdir("rec-append");
    let path;
    {
        let mut rec = Recorder::open(dir.path(), "match").expect("open");
        for _ in 0..5 {
            rec.record(
                RecordKind::Match,
                serde_json::json!({"label":"m","spec_ids":["a"],"seeds":[1],"deals":10,"seatings":20,"mb_per_seating":1.0,"se_mb":0.5,"vr_factor":1.0,"wall_s":0.1}),
            )
            .expect("record");
        }
        rec.flush().expect("flush");
        path = rec.path().to_path_buf();
    }
    let len_before = std::fs::metadata(&path).expect("meta").len();
    // Corrupt the tail: append a garbage line, then try to reopen.
    use std::io::Write;
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        f.write_all(b"{corrupt\n").expect("append");
    }
    let err =
        Recorder::open_latest(dir.path(), "match").expect_err("corrupt tail must stop the run");
    assert!(matches!(err, RecError::CorruptTail { .. }), "got {err:?}");
    // Length unchanged (no truncation ever happens).
    let len_after = std::fs::metadata(&path).expect("meta").len();
    assert_eq!(len_before + b"{corrupt\n".len() as u64, len_after);
}

#[test]
fn flush_cadence() {
    let dir = tmpdir("rec-cadence");
    let mut rec = Recorder::open(dir.path(), "match").expect("open");
    let small = serde_json::json!({"label":"m","spec_ids":["a"],"seeds":[1],"deals":1,"seatings":2,"mb_per_seating":0.0,"se_mb":0.0,"vr_factor":1.0,"wall_s":0.0});
    for _ in 0..2500 {
        rec.record(RecordKind::Match, small.clone())
            .expect("record");
    }
    assert!(
        rec.flushes() >= 2,
        "2500 records must trigger >= 2 auto-flushes, got {}",
        rec.flushes()
    );
    drop(rec); // drop() flushes the tail
}

#[test]
fn no_nan_payloads() {
    let dir = tmpdir("rec-nan");
    let mut rec = Recorder::open(dir.path(), "bp_probe").expect("open");
    // serde_json maps non-finite floats to null, so the workspace ban is enforced on
    // the sentinel strings too (documented injection vector).
    let bad = serde_json::json!({"lbr_mb":"NaN","coverage":0.5,"iters":1});
    let err = rec
        .record(RecordKind::BpProbe, bad)
        .expect_err("NaN must be rejected");
    assert!(matches!(err, RecError::NonFinite));
    // seq must not have advanced on a rejected record
    assert_eq!(rec.seq(), 0);
}

#[test]
fn validator_rejects_unknown_kind() {
    let dir = tmpdir("rec-unknown");
    let p = dir.path().join("events.jsonl");
    std::fs::write(
        &p,
        r#"{"ts":1,"run":"x","kind":"totally_bogus","seq":1,"data":{}}"#,
    )
    .expect("write");
    let err = validate::validate_file(&p).expect_err("unknown kind must fail");
    assert!(matches!(err, RecError::UnknownKind(_)));
    // And a missing required field is caught too.
    std::fs::write(
        &p,
        r#"{"ts":1,"run":"x","kind":"match","seq":1,"data":{"label":"m"}}"#,
    )
    .expect("write");
    let err = validate::validate_file(&p).expect_err("missing fields must fail");
    assert!(matches!(err, RecError::MissingField { .. }), "got {err:?}");
}

#[test]
fn concurrent_runs_separate_files() {
    let dir = tmpdir("rec-concurrent");
    let r1 = Recorder::open(dir.path(), "match").expect("open 1");
    let r2 = Recorder::open(dir.path(), "match").expect("open 2");
    assert_ne!(
        r1.run_id(),
        r2.run_id(),
        "two Recorders -> distinct run dirs"
    );
    assert_ne!(r1.path(), r2.path());
    assert!(r1.path().is_file());
    assert!(r2.path().is_file());
}

#[test]
fn resume_continues_seq() {
    let dir = tmpdir("rec-resume");
    {
        let mut rec = Recorder::open(dir.path(), "probe_summary").expect("open");
        rec.record(
            RecordKind::ProbeSummary,
            serde_json::json!({"lbr_mb":1.0,"coverage":1.0,"verdict":"PASS"}),
        )
        .expect("record");
        drop(rec);
    }
    let mut rec2 = Recorder::open_latest(dir.path(), "probe_summary").expect("reopen");
    assert_eq!(rec2.seq(), 1, "seq resumes from the tail");
    rec2.record(
        RecordKind::ProbeSummary,
        serde_json::json!({"lbr_mb":2.0,"coverage":1.0,"verdict":"PASS"}),
    )
    .expect("record");
    drop(rec2);
    let summaries = validate::validate_dir(dir.path()).expect("validate");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].1.records, 2);
}
