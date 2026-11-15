// Support shared by the survey layer's test binaries: the gtest comparisons and
// the C++ tests' property generator (tests/support/property.hpp), whose
// std::mt19937_64 and distributions are ported from katana_geometry's test
// support so that every property test draws the C++ sequence.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use katana_core::{ErrorCode, Result};

pub const PROPERTY_ITERATIONS: usize = 500;

/// EXPECT_NEAR(actual, expected, epsilon).
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within 4 units in the last place.
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

/// The error code of a result that must have failed.
pub fn error_code<T: std::fmt::Debug>(result: Result<T>) -> ErrorCode {
    match result {
        Ok(value) => panic!("expected a failure, got {value:?}"),
        Err(error) => error.code,
    }
}

fn real_from_bits(mut next: impl FnMut() -> u64, low: f64, high: f64) -> f64 {
    loop {
        let fraction = next() as f64 / 18446744073709551616.0;
        if fraction < 1.0 {
            return fraction * (high - low) + low;
        }
    }
}

/// katana::test::Random: std::mt19937_64 with the C++ default seed, and GCC's
/// uniform_real_distribution and uniform_int_distribution over it.
pub struct Random {
    state: [u64; 312],
    index: usize,
}

impl Default for Random {
    fn default() -> Self {
        Self::new(0x4B4154414E41)
    }
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let mut state = [0; 312];
        state[0] = seed;
        for index in 1..312 {
            state[index] = 6364136223846793005_u64
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 62))
                .wrapping_add(index as u64);
        }
        Self { state, index: 312 }
    }

    pub fn next_u64(&mut self) -> u64 {
        if self.index == 312 {
            for index in 0..312 {
                let value = (self.state[index] & 0xffffffff80000000)
                    | (self.state[(index + 1) % 312] & 0x7fffffff);
                self.state[index] = self.state[(index + 156) % 312]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 {
                        0xb5026f5aa96619e9
                    } else {
                        0
                    };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= (value >> 29) & 0x5555555555555555;
        value ^= (value << 17) & 0x71d67fffeda60000;
        value ^= (value << 37) & 0xfff7eee000000000;
        value ^ (value >> 43)
    }

    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        real_from_bits(|| self.next_u64(), low, high)
    }

    pub fn integer(&mut self, low: i32, high: i32) -> i32 {
        let range = (high as i64 - low as i64 + 1) as u64;
        let threshold = range.wrapping_neg() % range;
        loop {
            let product = self.next_u64() as u128 * range as u128;
            if product as u64 >= threshold {
                return low + (product >> 64) as i32;
            }
        }
    }
}
