#pragma once

// Compensated (Neumaier) summation. Areas and volumes are sums over millions of
// triangles; plain accumulation loses up to log2(n) bits, the compensated sum
// stays within a few ulp of the exact sum of its terms. Relies on the project's
// -fno-fast-math / -ffp-contract=off so the compiler may not reassociate it away.

#include <cmath>

namespace katana::terrain::detail {

class CompensatedSum {
  public:
    void add(double term)
    {
        const double next = sum_ + term;
        if (std::abs(sum_) >= std::abs(term)) {
            compensation_ += (sum_ - next) + term;
        } else {
            compensation_ += (term - next) + sum_;
        }
        sum_ = next;
    }

    [[nodiscard]] double value() const { return sum_ + compensation_; }

  private:
    double sum_ = 0.0;
    double compensation_ = 0.0;
};

} // namespace katana::terrain::detail
