// The column-pivoted Householder QR in this file is a translation into Rust of Eigen 3.4's
// Eigen/src/QR/ColPivHouseholderQR.h (Copyright (C) 2008-2009 Gael Guennebaud, (C) 2009
// Benoit Jacob) and Eigen/src/Householder/Householder.h and HouseholderSequence.h
// (Copyright (C) 2009 Gael Guennebaud, (C) 2010 Benoit Jacob), used under the Mozilla Public
// License 2.0 <https://mozilla.org/MPL/2.0/>; THIRD_PARTY_NOTICES.md ("Code translated
// into the Rust port") says what that licence asks.
//! Private numerical core shared by the linear solver and the network
//! adjustments: the twin of the C++ private header lsq_core.hpp and of the
//! Eigen code behind it in least_squares.cpp.
//!
//! The C++ factorises with Eigen's ColPivHouseholderQR. nalgebra's ColPivQR is
//! not the same algorithm - it pivots on the column holding the largest single
//! entry, not on the largest column norm, and has neither rank() nor a
//! threshold - so a degenerate network would be found rank deficient in other
//! unknowns, or not at all. The decomposition here is Eigen 3.4's
//! ColPivHouseholderQR::computeInPlace, step for step: the column norms, the
//! first-largest pivot, the nonzero-pivot count, the Householder reflector and
//! its application, the LAPACK norm downdate (LAWN 176), the permutation, and
//! rank() against the prescribed threshold. The solve, the triangular solves
//! and Q are applied as Eigen applies them, panel by panel where it has panels.
//!
//! What differs from the C++, by design of the port: every sum here is
//! sequential, where Eigen's SSE2 kernels (the C++ build's baseline) add a
//! vector of three or more terms in two interleaved lanes, so results agree
//! with the C++ to the last bit or two, not bit for bit. Every sum of one or two
//! terms, and so every result of a problem with at most three equations, is the
//! same. The rank decision compares pivots against kRankTolerance = 1e-9 of the
//! largest, so a deficiency the tests construct (a pivot of ~1e-16) is found in
//! the same unknowns. Eigen applies Q in blocks once it has 48 reflectors and
//! solves triangular systems against a matrix in register panels; both are
//! applied one reflector and one column at a time here.

use nalgebra::DMatrix;

use crate::least_squares::RANK_TOLERANCE;
use crate::matrix::Matrix;

pub(crate) struct WhitenedSolution {
    /// x
    pub(crate) parameters: Vec<f64>,
    /// Qxx = (A^T A)^-1 of the whitened system, exactly symmetric.
    pub(crate) cofactor: DMatrix<f64>,
    /// Diagonal of the hat matrix A Qxx A^T (squared row norms of the thin Q),
    /// each in [0, 1]. The redundancy number of whitened observation i is
    /// 1 - leverage_i.
    pub(crate) leverage: Vec<f64>,
}

pub(crate) struct RankDeficiency {
    pub(crate) rank: usize,
    /// Column indices without a usable pivot, ascending.
    pub(crate) unresolved: Vec<usize>,
}

pub(crate) enum Whitened {
    Solved(WhitenedSolution),
    Deficient(RankDeficiency),
}

// Eigen's EIGEN_TUNE_TRIANGULAR_PANEL_WIDTH: a triangular solve against a vector
// substitutes within panels of this many columns and updates the rest by a
// matrix-vector product per panel.
const TRIANGULAR_PANEL_WIDTH: usize = 16;

fn squared_norm(matrix: &DMatrix<f64>, column: usize, first_row: usize) -> f64 {
    let mut sum = 0.0;
    for row in first_row..matrix.nrows() {
        let value = matrix[(row, column)];
        sum += value * value;
    }
    sum
}

// The essential part of reflector k: column k below the diagonal.
fn essential_of(qr: &DMatrix<f64>, k: usize) -> Vec<f64> {
    (k + 1..qr.nrows()).map(|row| qr[(row, k)]).collect()
}

// MatrixBase::applyHouseholderOnTheLeft: H = I - tau v v^T with v = [1, essential]
// applied to rows first_row.. of the columns `columns` of `target`.
fn apply_householder_on_the_left(
    target: &mut DMatrix<f64>,
    first_row: usize,
    columns: std::ops::Range<usize>,
    essential: &[f64],
    tau: f64,
) {
    if target.nrows() - first_row == 1 {
        let factor = 1.0 - tau;
        for column in columns {
            target[(first_row, column)] *= factor;
        }
    } else if tau != 0.0 {
        for column in columns {
            // tmp = essential^T bottom + row0
            let mut tmp = 0.0;
            for (offset, e) in essential.iter().enumerate() {
                tmp += target[(first_row + 1 + offset, column)] * e;
            }
            tmp += target[(first_row, column)];
            target[(first_row, column)] -= tau * tmp;
            // bottom -= (tau essential) tmp, the scaled vector formed first as
            // Eigen's outer product evaluates it.
            for (offset, e) in essential.iter().enumerate() {
                target[(first_row + 1 + offset, column)] -= tmp * (tau * e);
            }
        }
    }
}

// MatrixBase::makeHouseholderInPlace on column `column` from row `first_row`:
// stores the essential part below the diagonal and returns (tau, beta).
fn make_householder_in_place(qr: &mut DMatrix<f64>, column: usize, first_row: usize) -> (f64, f64) {
    let rows = qr.nrows();
    let tail_sq_norm = if rows - first_row == 1 {
        0.0
    } else {
        squared_norm(qr, column, first_row + 1)
    };
    let c0 = qr[(first_row, column)];
    let tol = f64::MIN_POSITIVE;
    if tail_sq_norm <= tol {
        for row in first_row + 1..rows {
            qr[(row, column)] = 0.0;
        }
        (0.0, c0)
    } else {
        let mut beta = (c0 * c0 + tail_sq_norm).sqrt();
        if c0 >= 0.0 {
            beta = -beta;
        }
        let divisor = c0 - beta;
        for row in first_row + 1..rows {
            qr[(row, column)] /= divisor;
        }
        ((beta - c0) / beta, beta)
    }
}

struct ColPivHouseholderQr {
    qr: DMatrix<f64>,
    h_coeffs: Vec<f64>,
    // indices(): column i of A P is column permutation[i] of A.
    permutation: Vec<usize>,
    nonzero_pivots: usize,
    max_pivot: f64,
}

impl ColPivHouseholderQr {
    fn new(matrix: DMatrix<f64>) -> Self {
        let mut qr = matrix;
        let rows = qr.nrows();
        let cols = qr.ncols();
        let size = rows.min(cols);

        let mut h_coeffs = vec![0.0; size];
        let mut transpositions = vec![0; size];
        let mut col_norms_direct: Vec<f64> =
            (0..cols).map(|k| squared_norm(&qr, k, 0).sqrt()).collect();
        let mut col_norms_updated = col_norms_direct.clone();

        let biggest_norm = first_largest(&col_norms_updated).1;
        let threshold_helper =
            (biggest_norm * f64::EPSILON) * (biggest_norm * f64::EPSILON) / rows as f64;
        let norm_downdate_threshold = f64::EPSILON.sqrt();

        // the generic case is that in which all pivots are nonzero (invertible case)
        let mut nonzero_pivots = size;
        let mut max_pivot = 0.0;

        for k in 0..size {
            // first, we look up in our table col_norms_updated which column has the biggest norm
            let (offset, biggest_col_norm) = first_largest(&col_norms_updated[k..]);
            let biggest_col_index = offset + k;
            let biggest_col_sq_norm = biggest_col_norm * biggest_col_norm;

            // Track the number of meaningful pivots but do not stop the
            // decomposition to make sure that the initial matrix is properly
            // reproduced (Eigen's bug 941).
            if nonzero_pivots == size && biggest_col_sq_norm < threshold_helper * (rows - k) as f64
            {
                nonzero_pivots = k;
            }

            transpositions[k] = biggest_col_index;
            if k != biggest_col_index {
                qr.swap_columns(k, biggest_col_index);
                col_norms_updated.swap(k, biggest_col_index);
                col_norms_direct.swap(k, biggest_col_index);
            }

            // generate the householder vector, store it below the diagonal
            let (tau, beta) = make_householder_in_place(&mut qr, k, k);
            h_coeffs[k] = tau;
            qr[(k, k)] = beta;
            if beta.abs() > max_pivot {
                max_pivot = beta.abs();
            }

            let essential = essential_of(&qr, k);
            apply_householder_on_the_left(&mut qr, k, k + 1..cols, &essential, tau);

            // The stable norm downdate of LAPACK's xGEQPF and xGEQP3, from
            // http://www.netlib.org/lapack/lawnspdf/lawn176.pdf
            for j in k + 1..cols {
                if col_norms_updated[j] != 0.0 {
                    let mut temp = qr[(k, j)].abs() / col_norms_updated[j];
                    temp = (1.0 + temp) * (1.0 - temp);
                    if temp < 0.0 {
                        temp = 0.0;
                    }
                    let ratio = col_norms_updated[j] / col_norms_direct[j];
                    let temp2 = temp * (ratio * ratio);
                    if temp2 <= norm_downdate_threshold {
                        // The updated norm has become too inaccurate so
                        // re-compute the column norm directly.
                        col_norms_direct[j] = squared_norm(&qr, j, k + 1).sqrt();
                        col_norms_updated[j] = col_norms_direct[j];
                    } else {
                        col_norms_updated[j] *= temp.sqrt();
                    }
                }
            }
        }

        let mut permutation: Vec<usize> = (0..cols).collect();
        for (k, &transposition) in transpositions.iter().enumerate() {
            permutation.swap(k, transposition);
        }

        Self {
            qr,
            h_coeffs,
            permutation,
            nonzero_pivots,
            max_pivot,
        }
    }

    // ColPivHouseholderQR::rank() with the threshold set by setThreshold().
    fn rank(&self, threshold: f64) -> usize {
        let premultiplied_threshold = self.max_pivot.abs() * threshold;
        (0..self.nonzero_pivots)
            .filter(|&i| self.qr[(i, i)].abs() > premultiplied_threshold)
            .count()
    }

    // ColPivHouseholderQR::_solve_impl for one right-hand side.
    fn solve(&self, rhs: &[f64]) -> Vec<f64> {
        let cols = self.qr.ncols();
        let nonzero_pivots = self.nonzero_pivots;
        let mut dst = vec![0.0; cols];
        if nonzero_pivots == 0 {
            return dst;
        }
        let mut c = DMatrix::from_fn(rhs.len(), 1, |row, _| rhs[row]);
        // Q^T c: the reflectors in order, H_0 first.
        for (k, &tau) in self.h_coeffs.iter().enumerate().take(nonzero_pivots) {
            let essential = essential_of(&self.qr, k);
            apply_householder_on_the_left(&mut c, k, 0..1, &essential, tau);
        }
        let mut top: Vec<f64> = (0..nonzero_pivots).map(|row| c[(row, 0)]).collect();
        solve_upper_vector_in_place(&self.qr, &mut top);
        // dst.row(indices(i)) = c.row(i); the rows past the nonzero pivots stay zero
        for (i, value) in top.into_iter().enumerate() {
            dst[self.permutation[i]] = value;
        }
        dst
    }

    // R^-1 of the leading size x size block: TriangularView<Upper>::solve against
    // the identity, which Eigen does by its matrix kernel (a multiplication by
    // the reciprocal of each diagonal entry).
    fn inverse_r(&self, size: usize) -> DMatrix<f64> {
        let mut inverse = DMatrix::<f64>::identity(size, size);
        for column in 0..size {
            for i in (0..size).rev() {
                let reciprocal = 1.0 / self.qr[(i, i)];
                let b = inverse[(i, column)] * reciprocal;
                inverse[(i, column)] = b;
                for row in 0..i {
                    inverse[(row, column)] -= b * self.qr[(row, i)];
                }
            }
        }
        inverse
    }

    // householderQ() * Identity(rows, cols): the reflectors applied last to first.
    fn thin_q(&self, cols: usize) -> DMatrix<f64> {
        let rows = self.qr.nrows();
        let mut q = DMatrix::<f64>::identity(rows, cols);
        for (k, &tau) in self.h_coeffs.iter().enumerate().rev() {
            let essential = essential_of(&self.qr, k);
            apply_householder_on_the_left(&mut q, k, 0..cols, &essential, tau);
        }
        q
    }
}

// maxCoeff(&index): the first of equal largest values, as Eigen's visitor
// replaces its candidate only on a strictly larger value. Precondition: not
// empty.
fn first_largest(values: &[f64]) -> (usize, f64) {
    let mut best = (0, values[0]);
    for (index, &value) in values.iter().enumerate().skip(1) {
        if value > best.1 {
            best = (index, value);
        }
    }
    best
}

// Eigen's triangular_solve_vector for an upper-triangular, column-major R:
// substitution within panels from the bottom, each subtracting a solved
// unknown from the rows above it, and the rows above a panel updated by one
// matrix-vector product. An exactly zero right-hand side is skipped, as there.
pub(crate) fn solve_upper_vector_in_place(r: &DMatrix<f64>, rhs: &mut [f64]) {
    let size = rhs.len();
    let mut pi = size;
    while pi > 0 {
        let actual_panel_width = pi.min(TRIANGULAR_PANEL_WIDTH);
        let start_block = pi - actual_panel_width;
        for k in 0..actual_panel_width {
            let i = pi - k - 1;
            if rhs[i] != 0.0 {
                rhs[i] /= r[(i, i)];
                let x = rhs[i];
                let remaining = actual_panel_width - k - 1;
                for row in i - remaining..i {
                    rhs[row] -= x * r[(row, i)];
                }
            }
        }
        for row in 0..start_block {
            let mut sum = 0.0;
            for column in start_block..pi {
                sum += r[(row, column)] * rhs[column];
            }
            rhs[row] -= sum;
        }
        pi = start_block;
    }
}

// The same for a lower-triangular, column-major L, panels from the top.
pub(crate) fn solve_lower_vector_in_place(l: &DMatrix<f64>, rhs: &mut [f64]) {
    let size = rhs.len();
    let mut pi = 0;
    while pi < size {
        let actual_panel_width = (size - pi).min(TRIANGULAR_PANEL_WIDTH);
        let end_block = pi + actual_panel_width;
        for k in 0..actual_panel_width {
            let i = pi + k;
            if rhs[i] != 0.0 {
                rhs[i] /= l[(i, i)];
                let x = rhs[i];
                let remaining = actual_panel_width - k - 1;
                for row in i + 1..i + 1 + remaining {
                    rhs[row] -= x * l[(row, i)];
                }
            }
        }
        for row in end_block..size {
            let mut sum = 0.0;
            for column in pi..end_block {
                sum += l[(row, column)] * rhs[column];
            }
            rhs[row] -= sum;
        }
        pi = end_block;
    }
}

// TriangularView<Lower>::solve against a matrix, column by column with the
// reciprocal of each diagonal entry, as Eigen's matrix kernel multiplies.
pub(crate) fn solve_lower_matrix_in_place(l: &DMatrix<f64>, rhs: &mut DMatrix<f64>) {
    let size = l.nrows();
    for column in 0..rhs.ncols() {
        for i in 0..size {
            let reciprocal = 1.0 / l[(i, i)];
            let b = rhs[(i, column)] * reciprocal;
            rhs[(i, column)] = b;
            for row in i + 1..size {
                rhs[(row, column)] -= b * l[(row, i)];
            }
        }
    }
}

/// Solves min |A x - l|^2 for an already whitened system (unit weights) by
/// column-equilibrated, column-pivoted Householder QR. Precondition: all
/// entries finite, at least one row and one column. Any shape is accepted;
/// n < u is reported as a rank deficiency.
pub(crate) fn solve_whitened(design: &DMatrix<f64>, observations: &[f64]) -> Whitened {
    let rows = design.nrows();
    let parameter_count = design.ncols();

    // Column equilibration. Survey design matrices mix units (metres per metre,
    // radians per metre, pure ones for orientation-like parameters); scaling
    // every column to unit length makes the rank decision independent of those
    // units and minimises the condition number over all column scalings to
    // within sqrt(u).
    let scale: Vec<f64> = (0..parameter_count)
        .map(|j| {
            let norm = squared_norm(design, j, 0).sqrt();
            // a zero column stays zero and is rejected below
            if norm > 0.0 { 1.0 / norm } else { 1.0 }
        })
        .collect();
    let scaled = DMatrix::from_fn(rows, parameter_count, |i, j| design[(i, j)] * scale[j]);

    let qr = ColPivHouseholderQr::new(scaled);
    let rank = qr.rank(RANK_TOLERANCE);
    if rank < parameter_count {
        let mut unresolved = qr.permutation[rank..parameter_count].to_vec();
        unresolved.sort_unstable();
        return Whitened::Deficient(RankDeficiency { rank, unresolved });
    }

    let scaled_parameters = qr.solve(observations);
    let parameters = scale
        .iter()
        .zip(&scaled_parameters)
        .map(|(s, x)| s * x)
        .collect();

    // (As^T As)^-1 = Pi R^-1 R^-T Pi^T. R^-1 comes from a triangular solve
    // against the identity; nothing is inverted through the normal equations.
    let inverse_r = qr.inverse_r(parameter_count);
    let mut unpermuted = DMatrix::<f64>::zeros(parameter_count, parameter_count);
    for a in 0..parameter_count {
        for b in 0..parameter_count {
            let mut sum = 0.0;
            for k in 0..parameter_count {
                sum += inverse_r[(a, k)] * inverse_r[(b, k)];
            }
            unpermuted[(qr.permutation[a], qr.permutation[b])] = sum;
        }
    }

    let mut cofactor = DMatrix::<f64>::zeros(parameter_count, parameter_count);
    for i in 0..parameter_count {
        for j in i..parameter_count {
            // Upper triangle mirrored: exactly symmetric whatever the rounding.
            let value = scale[i] * scale[j] * unpermuted[(i, j)];
            cofactor[(i, j)] = value;
            cofactor[(j, i)] = value;
        }
    }

    let thin_q = qr.thin_q(parameter_count);
    let leverage = (0..rows)
        .map(|row| {
            let mut sum = 0.0;
            for column in 0..parameter_count {
                let value = thin_q[(row, column)];
                sum += value * value;
            }
            sum
        })
        .collect();

    Whitened::Solved(WhitenedSolution {
        parameters,
        cofactor,
        leverage,
    })
}

pub(crate) fn to_matrix(matrix: &DMatrix<f64>) -> Matrix {
    let mut result = Matrix::new(matrix.nrows(), matrix.ncols());
    for r in 0..matrix.nrows() {
        for c in 0..matrix.ncols() {
            result[(r, c)] = matrix[(r, c)];
        }
    }
    result
}

// Every entry of `matrix` times `factor`, as Eigen's `matrix * scalar`.
pub(crate) fn scaled_matrix(matrix: &DMatrix<f64>, factor: f64) -> Matrix {
    let mut result = to_matrix(matrix);
    for value in &mut result.values {
        *value *= factor;
    }
    result
}
