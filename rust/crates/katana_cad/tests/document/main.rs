//! The cad layer's Document core (wave F1), one module per C++ test file:
//!
//! ```text
//! document_index         tests/cad/test_document_index.cpp
//! indexed_queries        tests/cad/test_indexed_queries.cpp
//! dimension_draw         tests/cad/test_dimension_draw.cpp
//! section                tests/cad/test_section.cpp
//! customisation_record   tests/cad/customisation/test_customisation_record.cpp
//! surface_document       tests/cad/terrain/test_surface_document.cpp
//! surface_style          tests/cad/terrain/test_surface_style.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/surface_test_support.rs"]
pub mod surface_test_support;

mod customisation_record;
mod dimension_draw;
mod document_index;
mod indexed_queries;
mod section;
mod surface_document;
mod surface_style;

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

/// std::mt19937 with the distributions the C++ tests draw from, as GCC's
/// libstdc++ draws them (copied from katana_geometry's test support, with the
/// integer distribution added).
pub struct Mt19937 {
    state: [u32; 624],
    index: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Self {
        let mut state = [0; 624];
        state[0] = seed;
        for index in 1..624 {
            state[index] = 1812433253_u32
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 30))
                .wrapping_add(index as u32);
        }
        Self { state, index: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index == 624 {
            for index in 0..624 {
                let value =
                    (self.state[index] & 0x80000000) | (self.state[(index + 1) % 624] & 0x7fffffff);
                self.state[index] = self.state[(index + 397) % 624]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c5680;
        value ^= (value << 15) & 0xefc60000;
        value ^ (value >> 18)
    }

    /// std::uniform_real_distribution<double>(low, high): generate_canonical
    /// of two draws, redrawn (not clamped) when it rounds to one.
    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        loop {
            let bits = u64::from(self.next_u32()) | (u64::from(self.next_u32()) << 32);
            let fraction = bits as f64 / 18446744073709551616.0;
            if fraction < 1.0 {
                return fraction * (high - low) + low;
            }
        }
    }

    /// std::uniform_int_distribution over [0, range), for a range that fits
    /// the engine's 32 bits: libstdc++'s downscaling by Lemire's method
    /// (uniform_int_dist.h, _S_nd), the product's high word.
    pub fn below(&mut self, range: u32) -> u32 {
        let mut product = u64::from(self.next_u32()) * u64::from(range);
        let mut low = product as u32;
        if low < range {
            let threshold = range.wrapping_neg() % range;
            while low < threshold {
                product = u64::from(self.next_u32()) * u64::from(range);
                low = product as u32;
            }
        }
        (product >> 32) as u32
    }
}
