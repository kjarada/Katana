// The portable SIMD facade, as ordinary (baseline) code sees it. Built as C++26
// it is std::simd; as C++23 (KATANA_CXX_STANDARD=23) std::experimental::simd -
// the same tests hold for both.

#include <gtest/gtest.h>

#include <algorithm>
#include <bit>
#include <cmath>
#include <cstdint>
#include <limits>
#include <string>
#include <vector>

#include "katana/core/simd.hpp"

namespace simd = katana::core::simd;

namespace {

std::uint64_t bits(double value) { return std::bit_cast<std::uint64_t>(value); }

} // namespace

TEST(SimdFacade, TheBackendIsStdSimdExactlyWhenTheLibraryProvidesIt)
{
#if defined(__cpp_lib_simd) || defined(__glibcxx_simd)
    EXPECT_STREQ(simd::kBackend, "std::simd");
#else
    EXPECT_STREQ(simd::kBackend, "std::experimental::simd");
#endif
}

TEST(SimdFacade, BaselineCodeGetsWholeSse2RegistersOfEveryType)
{
    // x86-64 guarantees SSE2: 16 bytes a register, so 2 doubles, 4 floats.
    // An ordinary file is compiled for exactly that, never wider.
    EXPECT_EQ(simd::lanes<double> * sizeof(double), 16u);
    EXPECT_EQ(simd::lanes<float> * sizeof(float), 16u);
}

TEST(SimdFacade, LoadAndStoreCopyExactlyOneVectorAndNeedNoAlignment)
{
    const std::size_t n = simd::lanes<double>;
    // One element in, so the vector's address is not a multiple of its size.
    std::vector<double> in(n + 2);
    for (std::size_t i = 0; i < in.size(); ++i) {
        in[i] = 1.5 * static_cast<double>(i);
    }
    std::vector<double> out(n + 2, -7.0);
    simd::store(simd::load(in.data() + 1), out.data() + 1);
    EXPECT_EQ(out.front(), -7.0);
    EXPECT_EQ(out.back(), -7.0);
    for (std::size_t i = 1; i <= n; ++i) {
        EXPECT_EQ(out[i], in[i]);
    }
}

TEST(SimdFacade, MinOfAndMaxOfAreStdMinAndStdMaxLaneByLaneEvenForNaNAndZeros)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    // Worked by hand from std::min(a, b) = (b < a) ? b : a and
    // std::max(a, b) = (a < b) ? b : a, where every comparison with NaN is
    // false and -0 < +0 is false:
    //   a      b      min     max
    //   +0     -0     +0      +0     (neither is less: a both times)
    //   -0     +0     -0      -0
    //   1      NaN    1       1      (b is NaN: a)
    //   NaN    3      NaN     NaN    (a is NaN: a)
    //   2      1      1       2
    //   -4     5      -4      5
    const std::vector<double> a = {0.0, -0.0, 1.0, nan, 2.0, -4.0};
    const std::vector<double> b = {-0.0, 0.0, nan, 3.0, 1.0, 5.0};
    const std::vector<double> expectedMin = {0.0, -0.0, 1.0, nan, 1.0, -4.0};
    const std::vector<double> expectedMax = {0.0, -0.0, 1.0, nan, 2.0, 5.0};

    const std::size_t n = simd::lanes<double>;
    std::vector<double> padA = a;
    std::vector<double> padB = b;
    while (padA.size() % n != 0) {
        padA.push_back(0.0);
        padB.push_back(0.0);
    }
    std::vector<double> gotMin(padA.size());
    std::vector<double> gotMax(padA.size());
    for (std::size_t i = 0; i < padA.size(); i += n) {
        const auto va = simd::load(padA.data() + i);
        const auto vb = simd::load(padB.data() + i);
        simd::store(simd::minOf<double>(va, vb), gotMin.data() + i);
        simd::store(simd::maxOf<double>(va, vb), gotMax.data() + i);
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        EXPECT_EQ(bits(gotMin[i]), bits(expectedMin[i])) << "lane " << i;
        EXPECT_EQ(bits(gotMax[i]), bits(expectedMax[i])) << "lane " << i;
        EXPECT_EQ(bits(gotMin[i]), bits(std::min(a[i], b[i]))) << "lane " << i;
        EXPECT_EQ(bits(gotMax[i]), bits(std::max(a[i], b[i]))) << "lane " << i;
    }
}

TEST(SimdFacade, SelectTakesTheFirstWhereTheMaskIsSetAndTheSecondElsewhere)
{
    const std::size_t n = simd::lanes<float>;
    std::vector<float> x(n);
    std::vector<float> y(n);
    for (std::size_t i = 0; i < n; ++i) {
        x[i] = static_cast<float>(i);         // 0 1 2 3
        y[i] = static_cast<float>(n - 1 - i); // 3 2 1 0
    }
    const auto vx = simd::load(x.data());
    const auto vy = simd::load(y.data());
    std::vector<float> out(n);
    simd::store(simd::select<float>(vx < vy, vx, vy), out.data());
    // Lane i takes x where i < n-1-i, that is the first half; y elsewhere.
    // With 4 lanes: 0 1 1 0.
    for (std::size_t i = 0; i < n; ++i) {
        EXPECT_EQ(out[i], std::min(x[i], y[i])) << "lane " << i;
    }
}
