#pragma once

// Minimal dense row-major matrix used at the public boundary of the adjustment
// API. It is a data carrier only: the numerical work happens behind the module
// boundary (Eigen stays private to src/katana_survey, PLAN.MD Rule 4).

#include <cstddef>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::survey {

struct Matrix {
    std::size_t rows = 0;
    std::size_t cols = 0;
    std::vector<double> values; // row-major, rows * cols entries

    Matrix() = default;
    Matrix(std::size_t rowCount, std::size_t colCount, double fill = 0.0)
        : rows(rowCount), cols(colCount), values(rowCount * colCount, fill)
    {
    }

    // InvalidArgument when the rows have different lengths.
    [[nodiscard]] static katana::core::Result<Matrix>
    fromRows(const std::vector<std::vector<double>>& rowValues)
    {
        Matrix matrix;
        matrix.rows = rowValues.size();
        matrix.cols = rowValues.empty() ? 0 : rowValues.front().size();
        matrix.values.reserve(matrix.rows * matrix.cols);
        for (std::size_t r = 0; r < rowValues.size(); ++r) {
            if (rowValues[r].size() != matrix.cols) {
                return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                               "matrix rows have different lengths",
                                               "row " + std::to_string(r));
            }
            matrix.values.insert(matrix.values.end(), rowValues[r].begin(), rowValues[r].end());
        }
        return matrix;
    }

    [[nodiscard]] static Matrix identity(std::size_t size)
    {
        Matrix matrix(size, size);
        for (std::size_t i = 0; i < size; ++i) {
            matrix(i, i) = 1.0;
        }
        return matrix;
    }

    // Precondition: row < rows and col < cols.
    [[nodiscard]] double& operator()(std::size_t row, std::size_t col)
    {
        return values[row * cols + col];
    }
    [[nodiscard]] double operator()(std::size_t row, std::size_t col) const
    {
        return values[row * cols + col];
    }

    // False when `values` was edited to a size that contradicts rows * cols.
    [[nodiscard]] bool isConsistent() const { return values.size() == rows * cols; }

    friend bool operator==(const Matrix&, const Matrix&) = default;
};

} // namespace katana::survey
