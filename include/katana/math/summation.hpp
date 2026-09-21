#pragma once

// Compensated (Neumaier) summation.
//
// Areas, volumes and earthwork quantities are sums over thousands to millions
// of terms. Plain accumulation loses up to log2(n) bits to rounding; the
// compensated sum stays within a few ulp of the exact sum of its terms
// whatever the order and magnitude of them (PLAN.MD section 35). Relies on
// the project's -fno-fast-math / -ffp-contract=off so the compiler may not
// reassociate the compensation away - which is one of the reasons those
// flags are not negotiable.
//
// This lived in katana::terrain::detail until the corridor quantities in cad
// needed the same thing, at which point a second copy would have been the
// defect CLAUDE.md section 1 names. The terrain header now aliases this.
//
// Neumaier's variant rather than Kahan's: Kahan loses the compensation when a
// term is larger than the running sum, which happens on the first term of
// every sum and whenever a large triangle follows small ones. Neumaier's
// branch handles both orders.

#include <cmath>

namespace katana::math {

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

} // namespace katana::math
