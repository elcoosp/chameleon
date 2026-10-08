use cham_engine::encoder::Encoder;

#[test]
fn bucket_mix_is_injective() {
    let mut seen = std::collections::HashSet::new();
    for b in 0u16..64 {
        assert!(seen.insert(Encoder::bucket_mix(b)), "collision at {b}");
    }
}

#[test]
fn bucket_mix_zero_is_zero() {
    // mix(0) == 0 keeps the v3 zero-bucket key == the bucket-free hash.
    assert_eq!(Encoder::bucket_mix(0), 0);
}
