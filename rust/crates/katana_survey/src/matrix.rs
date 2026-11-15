//! Minimal dense row-major matrix used at the public boundary of the adjustment
//! API. It is a data carrier only: the numerical work happens behind the module
//! boundary, and nalgebra stays private to the crate (docs/rust.md).

use std::ops::{Index, IndexMut};

use katana_core::{Error, ErrorCode, Result};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    /// Row-major, rows * cols entries.
    pub values: Vec<f64>,
}

impl Matrix {
    /// A rows x cols matrix of zeros.
    pub fn new(row_count: usize, col_count: usize) -> Self {
        Self::filled(row_count, col_count, 0.0)
    }

    /// A rows x cols matrix with every entry `fill`.
    pub fn filled(row_count: usize, col_count: usize, fill: f64) -> Self {
        Self {
            rows: row_count,
            cols: col_count,
            values: vec![fill; row_count * col_count],
        }
    }

    /// InvalidArgument when the rows have different lengths.
    pub fn from_rows<R: AsRef<[f64]>>(row_values: &[R]) -> Result<Matrix> {
        let rows = row_values.len();
        let cols = row_values.first().map_or(0, |row| row.as_ref().len());
        let mut values = Vec::with_capacity(rows * cols);
        for (r, row) in row_values.iter().enumerate() {
            let row = row.as_ref();
            if row.len() != cols {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "matrix rows have different lengths",
                )
                .with_context(format!("row {r}")));
            }
            values.extend_from_slice(row);
        }
        Ok(Matrix { rows, cols, values })
    }

    pub fn identity(size: usize) -> Self {
        let mut matrix = Self::new(size, size);
        for i in 0..size {
            matrix[(i, i)] = 1.0;
        }
        matrix
    }

    /// False when `values` was edited to a size that contradicts rows * cols.
    pub fn is_consistent(&self) -> bool {
        self.values.len() == self.rows * self.cols
    }
}

/// Precondition: row < rows and col < cols.
impl Index<(usize, usize)> for Matrix {
    type Output = f64;

    fn index(&self, (row, col): (usize, usize)) -> &f64 {
        &self.values[row * self.cols + col]
    }
}

impl IndexMut<(usize, usize)> for Matrix {
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut f64 {
        &mut self.values[row * self.cols + col]
    }
}
