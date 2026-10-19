const MIX_A: u64 = 0x9e37_79b9_7f4a_7c15;
const MIX_B: u64 = 0xbf58_476d_1ce4_e5b9;
const MIX_C: u64 = 0x94d0_49bb_1331_11eb;
const UNIT_BITS: u32 = 24;

pub fn hash(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(MIX_A);
    z = (z ^ (z >> 30)).wrapping_mul(MIX_B);
    z = (z ^ (z >> 27)).wrapping_mul(MIX_C);
    z ^ (z >> 31)
}

pub fn unit(seed: u64) -> f32 {
    let scale = (1u32 << UNIT_BITS) as f32;
    (hash(seed) >> (u64::BITS - UNIT_BITS)) as f32 / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_deterministic_and_in_range() {
        assert_eq!(unit(7), unit(7));
        assert!((0..1000).map(unit).all(|v| (0.0..1.0).contains(&v)));
        assert_ne!(unit(1), unit(2));
    }
}
