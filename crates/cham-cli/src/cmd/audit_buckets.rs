//! EXP-017 bucket-quality audit CLI: reads (bucket_id, realized EV) pairs.

pub fn run(input: &str) -> i32 {
    let txt = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("audit-buckets: cannot read '{input}': {e}");
            return crate::cmd::EXIT_BUDGET;
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&txt) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("audit-buckets: bad JSON: {e}");
            return crate::cmd::EXIT_FAIL;
        }
    };
    let hands: Vec<(u32, f64)> = v
        .get("hands")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    Some((
                        h.get("bucket")?.as_u64()? as u32,
                        h.get("ev")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if hands.len() < 2 {
        eprintln!("audit-buckets: need ≥2 hands in {{\"hands\":[{{\"bucket\":..,\"ev\":..}}]}}");
        return crate::cmd::EXIT_FAIL;
    }
    let r = cham_engine::audit::audit_bucket_quality(&hands);
    println!(
        "audit-buckets: within={:.4} between={:.4} ratio={:.4} (n={})",
        r.within_bucket_var,
        r.between_bucket_var,
        r.ratio,
        hands.len()
    );
    crate::cmd::EXIT_OK
}
