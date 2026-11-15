#[derive(Clone, Copy, Debug, Default)]
pub struct CompensatedSum {
    sum: f64,
    compensation: f64,
}

impl CompensatedSum {
    pub const fn new() -> Self {
        Self {
            sum: 0.0,
            compensation: 0.0,
        }
    }

    pub fn add(&mut self, term: f64) {
        let next = self.sum + term;
        if self.sum.abs() >= term.abs() {
            self.compensation += (self.sum - next) + term;
        } else {
            self.compensation += (term - next) + self.sum;
        }
        self.sum = next;
    }

    pub fn value(&self) -> f64 {
        self.sum + self.compensation
    }
}
