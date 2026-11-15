#[macro_export]
macro_rules! assert_double_eq {
    ($actual:expr, $expected:expr $(,)?) => {{
        let actual: f64 = $actual;
        let expected: f64 = $expected;
        let biased = |value: f64| {
            let bits = value.to_bits();
            let sign = 1_u64 << 63;
            if bits & sign != 0 {
                (!bits).wrapping_add(1)
            } else {
                bits | sign
            }
        };
        assert!(
            !actual.is_nan()
                && !expected.is_nan()
                && biased(actual).abs_diff(biased(expected)) <= 4,
            "{actual:?} is not within four ULPs of {expected:?}"
        );
    }};
}

#[macro_export]
macro_rules! assert_near {
    ($actual:expr, $expected:expr, $tolerance:expr $(,)?) => {{
        let actual: f64 = $actual;
        let expected: f64 = $expected;
        let tolerance: f64 = $tolerance;
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual:?} is not within {tolerance:?} of {expected:?}"
        );
    }};
}
