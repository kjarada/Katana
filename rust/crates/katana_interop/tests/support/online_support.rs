//! What the online test targets share (step 5, agent L2, its owner): where the
//! fixtures are, the live switch and the asserts the C++ suites make. Included
//! with `#[path = "../support/online_support.rs"] mod online_support;` by
//! tests/online_requests and tests/online_environment.

#![allow(
    dead_code,
    reason = "Each target that includes this module uses the part of it its cases need."
)]

use katana_core::{Error, Result};

/// tests/interop/data/online, where the C++ reads its fixtures
/// (KATANA_ONLINE_TEST_DATA).
pub const ONLINE_TEST_DATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/interop/data/online"
);

/// `OnlineEnvironment{}.userAgent`, online_fetch's `onlineUserAgent("dev")`:
/// what the live cases name themselves with. online_fetch is wave M's; until
/// it is ported its text stands here, and then its call should.
pub const DEFAULT_USER_AGENT: &str =
    "Katana/dev (survey and civil-engineering CAD; +https://github.com/kjarada/Katana)";

/// A fixture of tests/interop/data/online, as text.
pub fn fixture(name: &str) -> String {
    let path = format!("{ONLINE_TEST_DATA}/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// The live cases run only with KATANA_ONLINE_TESTS=1, as the C++ ones do: a
/// test run on a train must not fail because a state government's server is
/// down.
pub fn live_tests_enabled() -> bool {
    std::env::var("KATANA_ONLINE_TESTS").is_ok_and(|flag| flag == "1")
}

/// What a skipped live case says (GTEST_SKIP's reason).
pub const LIVE_SKIP: &str = "set KATANA_ONLINE_TESTS=1 to reach the network";

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place, as gtest compares.
#[track_caller]
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

/// ASSERT_TRUE(result.ok()) << result.error().describe(), and the value.
#[track_caller]
pub fn must<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// ASSERT_FALSE(result.ok()), and the error.
#[track_caller]
pub fn refused<T>(result: Result<T>, what: &str) -> Error {
    match result {
        Ok(_) => panic!("{what} should have been refused"),
        Err(error) => error,
    }
}

/// `body.find(text) != std::string::npos`, of bytes.
pub fn holds(body: &[u8], text: &str) -> bool {
    text.is_empty()
        || body
            .windows(text.len())
            .any(|window| window == text.as_bytes())
}
