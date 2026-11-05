use katana_math::Vec2;
use katana_math::numerics::{nearly_equal, tolerance};

pub const PROPERTY_ITERATIONS: usize = 500;
pub mod simd;

fn real_from_bits(mut next: impl FnMut() -> u64, low: f64, high: f64) -> f64 {
    loop {
        let fraction = next() as f64 / 18446744073709551616.0;
        if fraction < 1.0 {
            return fraction * (high - low) + low;
        }
    }
}

#[test]
fn a_fraction_rounded_to_one_redraws_instead_of_clamping() {
    let mut samples = [u64::MAX, 0].into_iter();
    assert_eq!(real_from_bits(|| samples.next().unwrap(), 2.0, 3.0), 2.0);
    assert!(samples.next().is_none());
}

pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

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

pub fn vec2_eq(actual: Vec2, expected: Vec2) -> bool {
    nearly_equal(actual.x, expected.x, tolerance::RELATIVE)
        && nearly_equal(actual.y, expected.y, tolerance::RELATIVE)
}

pub struct Random {
    state: [u64; 312],
    index: usize,
}

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
    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        real_from_bits(
            || u64::from(self.next_u32()) | (u64::from(self.next_u32()) << 32),
            low,
            high,
        )
    }
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
