pub const PROPERTY_ITERATIONS: usize = 500;

pub struct Random {
    state: [u64; 312],
    index: usize,
}

impl Default for Random {
    fn default() -> Self {
        let mut state = [0_u64; 312];
        state[0] = 0x4B4154414E41;
        for index in 1..state.len() {
            state[index] = 6364136223846793005_u64
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 62))
                .wrapping_add(index as u64);
        }
        Self { state, index: 312 }
    }
}

impl Random {
    fn next_u64(&mut self) -> u64 {
        if self.index == self.state.len() {
            for index in 0..self.state.len() {
                let combined = (self.state[index] & 0xFFFFFFFF80000000)
                    | (self.state[(index + 1) % 312] & 0x7FFFFFFF);
                self.state[index] = self.state[(index + 156) % 312]
                    ^ (combined >> 1)
                    ^ if combined & 1 != 0 {
                        0xB5026F5AA96619E9
                    } else {
                        0
                    };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= (value >> 29) & 0x5555555555555555;
        value ^= (value << 17) & 0x71D67FFFEDA60000;
        value ^= (value << 37) & 0xFFF7EEE000000000;
        value ^= value >> 43;
        value
    }

    pub fn real(&mut self, lower: f64, upper: f64) -> f64 {
        loop {
            let canonical = self.next_u64() as f64 / 18446744073709551616.0;
            if canonical < 1.0 {
                return canonical * (upper - lower) + lower;
            }
        }
    }
}
