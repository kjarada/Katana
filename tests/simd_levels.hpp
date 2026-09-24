#pragma once

// For tests that hold a SIMD kernel to its scalar reference: run the same call
// at each level, through the same public function, and compare.
//
// The level is switched with core::setSimdLevel, the in-process form of the
// KATANA_SIMD override. That is legitimate only because every level is
// bit-identical - the property these tests exist to prove.

#include <gtest/gtest.h>

#include "katana/core/cpu_features.hpp"

namespace katana::test {

class ScopedSimdLevel {
  public:
    explicit ScopedSimdLevel(katana::core::SimdLevel level)
    {
        const auto previous = katana::core::setSimdLevel(level);
        ok_ = static_cast<bool>(previous);
        if (ok_) {
            previous_ = *previous;
        }
    }
    ~ScopedSimdLevel()
    {
        if (ok_) {
            (void)katana::core::setSimdLevel(previous_);
        }
    }
    ScopedSimdLevel(const ScopedSimdLevel&) = delete;
    ScopedSimdLevel& operator=(const ScopedSimdLevel&) = delete;

    [[nodiscard]] bool ok() const { return ok_; }

  private:
    bool ok_ = false;
    katana::core::SimdLevel previous_ = katana::core::SimdLevel::Scalar;
};

// f() at `level`, with the level restored afterwards. Fails the test if the
// processor cannot run the level; callers check avx2Available() first.
template <typename F> auto atSimdLevel(katana::core::SimdLevel level, F&& f)
{
    ScopedSimdLevel scope(level);
    EXPECT_TRUE(scope.ok()) << "cannot run at " << katana::core::toString(level);
    return f();
}

[[nodiscard]] inline bool avx2Available()
{
    return katana::core::detectedSimdLevel() >= katana::core::SimdLevel::Avx2;
}

} // namespace katana::test

// Skips (visibly, with the reason) a comparison that would compare the scalar
// path with itself on a processor that cannot run the AVX2 one.
#define KATANA_REQUIRE_AVX2()                                                                      \
    do {                                                                                           \
        if (!katana::test::avx2Available()) {                                                      \
            GTEST_SKIP() << "this processor has no AVX2: only the scalar path can run here";       \
        }                                                                                          \
    } while (false)
