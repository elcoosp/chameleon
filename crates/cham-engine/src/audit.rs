//! EXP-017 bucket-quality audit (v2 §3.4, first implementation): from real
//! match data, within-bucket vs between-bucket variance of realized showdown
//! EV. Higher between/within ratio = buckets separate real EV differences.

use std::collections::HashMap;

pub struct BucketAuditReport {
    pub within_bucket_var: f64,
    pub between_bucket_var: f64,
    pub ratio: f64,
}

pub fn audit_bucket_quality(hands: &[(u32, f64)]) -> BucketAuditReport {
    let mut by_bucket: HashMap<u32, Vec<f64>> = HashMap::new();
    for &(b, ev) in hands {
        by_bucket.entry(b).or_default().push(ev);
    }
    if hands.is_empty() {
        return BucketAuditReport {
            within_bucket_var: 0.0,
            between_bucket_var: 0.0,
            ratio: 0.0,
        };
    }
    let grand_mean = hands.iter().map(|&(_, ev)| ev).sum::<f64>() / hands.len() as f64;
    let within: f64 = by_bucket
        .values()
        .map(|v| {
            let m = v.iter().sum::<f64>() / v.len() as f64;
            v.iter().map(|&x| (x - m).powi(2)).sum::<f64>()
        })
        .sum::<f64>()
        / hands.len() as f64;
    let between: f64 = by_bucket
        .values()
        .map(|v| {
            let m = v.iter().sum::<f64>() / v.len() as f64;
            v.len() as f64 * (m - grand_mean).powi(2)
        })
        .sum::<f64>()
        / hands.len() as f64;
    BucketAuditReport {
        within_bucket_var: within,
        between_bucket_var: between,
        ratio: between / within.max(1e-9),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separated_buckets_score_high() {
        // Two tight, well-separated buckets → high ratio.
        let mut hands = vec![];
        for _ in 0..10 {
            hands.push((0, 100.0));
            hands.push((1, -100.0));
        }
        let r = audit_bucket_quality(&hands);
        assert!(r.ratio > 10.0, "ratio={}", r.ratio);
    }

    #[test]
    fn mixed_buckets_score_low() {
        // Same EVs scrambled across buckets → ratio ≈ 0.
        let hands = vec![(0, 100.0), (1, -100.0), (0, -100.0), (1, 100.0)];
        let r = audit_bucket_quality(&hands);
        assert!(r.ratio < 1.0, "ratio={}", r.ratio);
    }
}
