// Support shared by the surveyio layer's test binaries.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::path::{Path, PathBuf};

/// A scratch folder unique to one test of one run (the test's name and the
/// process id), removed on drop: the tests of a binary run on parallel threads
/// of one process, where gtest ran each case in its own.
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// Writes `bytes` to `relative` under the folder, making its folders.
    pub fn write(&self, relative: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// haystack.find(needle) != npos.
pub fn contains(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}

/// EXPECT_NEAR(actual, expected, epsilon).
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
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

/// std::mt19937, whose sequence the standard fixes (katana_geometry's test
/// support ports it as Random32), so a test that damages input at random
/// damages it as the C++ test does.
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

    /// `random()`: the engine's next output.
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

    /// `std::uniform_int_distribution(low, high)(random)` as libstdc++ draws
    /// it from this 32-bit engine: for a range of at most 2^32 values, Lemire's
    /// multiply-and-shift with its rejection of the biased low words
    /// (`_S_nd`), and the engine's whole output for exactly 2^32.
    pub fn uniform(&mut self, low: u64, high: u64) -> u64 {
        let range = high - low + 1;
        if range > u64::from(u32::MAX) {
            return low + u64::from(self.next_u32());
        }
        let range = range as u32;
        let mut product = u64::from(self.next_u32()) * u64::from(range);
        let mut low_word = product as u32;
        if low_word < range {
            let threshold = range.wrapping_neg() % range;
            while low_word < threshold {
                product = u64::from(self.next_u32()) * u64::from(range);
                low_word = product as u32;
            }
        }
        low + (product >> 32)
    }
}
