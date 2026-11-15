//! What the render tests share: gtest's comparisons, GCC's random engine and
//! distribution, and the SIMD level switch (the step-1 crates' test support).

use katana_core::Result;
use katana_core::cpu_features::{SimdLevel, detected_simd_level, set_simd_level};
use katana_render::{Framebuffer, RenderStats, Rgba};
use std::sync::Mutex;

/// EXPECT_NEAR.
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_NEAR with what the C++'s SCOPED_TRACE or streamed message said.
pub fn near_because(actual: f64, expected: f64, epsilon: f64, because: &str) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "{because}: actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
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

/// EXPECT_FLOAT_EQ: within 4 units in the last place of a float.
pub fn float_eq(actual: f32, expected: f32) {
    fn biased(value: f32) -> u32 {
        let bits = value.to_bits();
        if bits & (1 << 31) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 31)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// How many pixels of the frame are `color`.
pub fn count_pixels(framebuffer: &Framebuffer, color: Rgba) -> usize {
    count_colour(framebuffer.color(), color)
}

pub fn count_colour(pixels: &[Rgba], color: Rgba) -> usize {
    pixels.iter().filter(|&&pixel| pixel == color).count()
}

/// A render the test expects to succeed, with the error's description when it
/// does not (the C++'s `<< stats.error().describe()`).
pub fn rendered(result: Result<RenderStats>) -> RenderStats {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn real_from_bits(mut next: impl FnMut() -> u64, low: f64, high: f64) -> f64 {
    loop {
        // GCC 16's generate_canonical: one 64-bit draw over 2^64, and a draw
        // that rounds to 1 is drawn again rather than clamped.
        let fraction = next() as f64 / 18446744073709551616.0;
        if fraction < 1.0 {
            return fraction * (high - low) + low;
        }
    }
}

/// std::mt19937_64, as GCC's libstdc++ implements it.
pub struct Random {
    state: [u64; 312],
    index: usize,
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

    /// One draw of the engine: the C++'s `random()`.
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

    /// std::uniform_real_distribution<double>(low, high) on this engine.
    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        real_from_bits(|| self.next_u64(), low, high)
    }
}

// The SIMD level is the process's, and Rust runs a binary's tests on threads
// of one process: a test that sets it holds this lock while it runs at it.
static LEVEL_LOCK: Mutex<()> = Mutex::new(());

struct RestoreLevel(SimdLevel);

impl Drop for RestoreLevel {
    fn drop(&mut self) {
        set_simd_level(self.0).expect("restore SIMD level");
    }
}

/// `body()` at `level`, with the level restored afterwards. Fails the test if
/// the processor cannot run the level.
pub fn at_simd_level<Value>(level: SimdLevel, body: impl FnOnce() -> Value) -> Value {
    let _lock = LEVEL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _restore = RestoreLevel(set_simd_level(level).expect("requested SIMD level must run"));
    body()
}

/// The level whose kernels this machine runs - Avx2 on an x86-64 processor
/// with AVX2, Neon on 64-bit ARM - and a failure where there are none: a
/// kernel comparison run on the scalar path alone would compare it with
/// itself, and is not counted as having compared the kernels (docs/rust.md,
/// "Step 1").
pub fn require_kernel() -> SimdLevel {
    let level = detected_simd_level();
    assert_ne!(
        level,
        SimdLevel::Scalar,
        "a real SIMD backend is required for this parity test"
    );
    level
}
