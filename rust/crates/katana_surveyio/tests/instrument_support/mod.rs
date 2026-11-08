// Support shared by the test binaries of the instrument readers (Leica GSI and
// DBX, RINEX): the gtest comparisons, the C++ tests' random engine, and the
// fixtures under tests/surveyio/data.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::path::{Path, PathBuf};

/// tests/surveyio/data, where the C++ tests read their fixtures.
pub fn data_folder() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/surveyio/data"
    ))
}

/// The bytes of a fixture; empty when it is missing, as the C++'s slurp gives.
pub fn slurp(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// Every regular file under `folder`, at any depth, as
/// std::filesystem::recursive_directory_iterator walks it (in no promised
/// order; the tests that walk it do not depend on one).
pub fn files_under(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(next) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = next.join(entry.file_name());
            if path.is_dir() {
                folders.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
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

/// std::atoi: leading blanks, a sign, digits; 0 when there are none.
pub fn atoi(text: &str) -> i32 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for c in digits.bytes() {
        if !c.is_ascii_digit() {
            break;
        }
        value = (value * 10 + i64::from(c - b'0')).min(i64::from(i32::MAX) + 1);
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// std::mt19937, the 32-bit Mersenne Twister, as GCC's libstdc++ has it, with
/// the one distribution the C++ tests draw from it: std::uniform_int_distribution
/// over a range narrower than the engine's.
pub struct Random32 {
    state: [u32; 624],
    index: usize,
}

impl Random32 {
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

    /// The engine's next output: random() in the C++ tests.
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

    /// std::uniform_int_distribution(low, high) as libstdc++ draws it from an
    /// engine of exactly 32 bits: Lemire's nearly divisionless reduction in 64
    /// bits (_S_nd), which takes the high word of the product and redraws only
    /// below the threshold.
    pub fn uniform(&mut self, low: u64, high: u64) -> u64 {
        let range = (high - low + 1) as u32;
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
