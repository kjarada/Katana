//! What the subsurface test binaries share: gtest's floating-point
//! comparisons, as katana_geometry's test support has them.

#![allow(
    dead_code,
    reason = "Each test binary uses a part of the shared support."
)]

/// `EXPECT_NEAR(actual, expected, epsilon)`.
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// `EXPECT_DOUBLE_EQ(actual, expected)`: within 4 units in the last place.
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}
