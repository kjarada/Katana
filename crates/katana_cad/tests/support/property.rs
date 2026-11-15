//! Minimal property-testing support (tests/support/property.hpp): a
//! fixed-seed generator so that every run explores the same inputs
//! (deterministic, reproducible failures).
//!
//! `Random` is std::mt19937_64 with GCC's libstdc++ distributions
//! (uniform_real_distribution through generate_canonical, and
//! uniform_int_distribution by Lemire's multiply-and-reject), as katana_geometry's
//! test support ports them, so a case draws the very numbers the C++ drew.
//! The cad layer's test targets include it with
//! `#[path = "../support/property.rs"] mod property;`.

#![allow(
    dead_code,
    reason = "A target that includes this file uses only what its cases draw."
)]

/// katana::test::kPropertyIterations.
pub const PROPERTY_ITERATIONS: usize = 500;

// generate_canonical<double, 53> over a 64-bit engine: one draw divided by
// 2^64, drawn again when rounding makes it 1.
fn real_from_bits(mut next: impl FnMut() -> u64, low: f64, high: f64) -> f64 {
    loop {
        let fraction = next() as f64 / 18_446_744_073_709_551_616.0;
        if fraction < 1.0 {
            return fraction * (high - low) + low;
        }
    }
}

/// katana::test::Random: std::mt19937_64 seeded 0x4B4154414E41 by default.
pub struct Random {
    state: [u64; 312],
    index: usize,
}

impl Default for Random {
    fn default() -> Self {
        Self::new(0x4B41_5441_4E41)
    }
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let mut state = [0; 312];
        state[0] = seed;
        for index in 1..312 {
            state[index] = 6_364_136_223_846_793_005_u64
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 62))
                .wrapping_add(index as u64);
        }
        Self { state, index: 312 }
    }

    pub fn next_u64(&mut self) -> u64 {
        if self.index == 312 {
            for index in 0..312 {
                let value = (self.state[index] & 0xffff_ffff_8000_0000)
                    | (self.state[(index + 1) % 312] & 0x7fff_ffff);
                self.state[index] = self.state[(index + 156) % 312]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 {
                        0xb502_6f5a_a966_19e9
                    } else {
                        0
                    };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= (value >> 29) & 0x5555_5555_5555_5555;
        value ^= (value << 17) & 0x71d6_7fff_eda6_0000;
        value ^= (value << 37) & 0xfff7_eee0_0000_0000;
        value ^ (value >> 43)
    }

    /// std::uniform_real_distribution<double>(lo, hi).
    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        real_from_bits(|| self.next_u64(), low, high)
    }

    /// std::uniform_int_distribution<int>(lo, hi), both ends included.
    pub fn integer(&mut self, low: i32, high: i32) -> i32 {
        let range = (i64::from(high) - i64::from(low) + 1) as u64;
        let threshold = range.wrapping_neg() % range;
        loop {
            let product = u128::from(self.next_u64()) * u128::from(range);
            if product as u64 >= threshold {
                return low + (product >> 64) as i32;
            }
        }
    }
}
