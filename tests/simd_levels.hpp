#pragma once

// For tests that hold a SIMD kernel to its scalar reference: run the same call
// at each level, through the same public function, and compare.
//
// The level is switched with core::setSimdLevel, the in-process form of the
// KATANA_SIMD override. That is legitimate only because every level is
// bit-identical - the property these tests exist to prove.

#include <gtest/gtest.h>

#include <vector>

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
// processor cannot run the level; callers ask kernelsAvailable() first.
template <typename F> auto atSimdLevel(katana::core::SimdLevel level, F&& f)
{
    ScopedSimdLevel scope(level);
    EXPECT_TRUE(scope.ok()) << "cannot run at " << katana::core::toString(level);
    return f();
}

// The level whose kernels this machine runs - Avx2 on an x86-64 processor
// with AVX2, Neon on 64-bit ARM - or Scalar where there are none. It is what
// every comparison holds against Scalar, so one test covers whichever kernel
// set the build has.
[[nodiscard]] inline katana::core::SimdLevel kernelLevel()
{
    return katana::core::detectedSimdLevel();
}

[[nodiscard]] inline bool kernelsAvailable()
{
    return kernelLevel() != katana::core::SimdLevel::Scalar;
}

// Scalar, then the kernel level where there is one: the levels a test runs
// the same call at.
[[nodiscard]] inline std::vector<katana::core::SimdLevel> simdLevels()
{
    std::vector<katana::core::SimdLevel> levels{katana::core::SimdLevel::Scalar};
    if (kernelsAvailable()) {
        levels.push_back(kernelLevel());
    }
    return levels;
}

} // namespace katana::test

// Skips (visibly, with the reason) a comparison that would compare the scalar
// path with itself on a processor that has no kernels to run.
#define KATANA_REQUIRE_SIMD_KERNELS()                                                              \
    do {                                                                                           \
        if (!katana::test::kernelsAvailable()) {                                                   \
            GTEST_SKIP() << "no SIMD kernels run on this processor: only the scalar path can "     \
                            "run here";                                                            \
        }                                                                                          \
    } while (false)
