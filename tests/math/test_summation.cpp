// Compensated summation (include/katana/math/summation.hpp).
//
// Every area and volume in the product goes through this. It was tested only
// through the volume tests while it was private to terrain; now that it is
// public and the corridor quantities use it too, the property it exists for
// is pinned directly.

#include <gtest/gtest.h>

#include <cmath>
#include <vector>

#include "katana/math/summation.hpp"

using katana::math::CompensatedSum;

TEST(CompensatedSum, KeepsASmallTermThatPlainAdditionWouldLose)
{
    // 1e16 + 1 - 1e16: plain double addition gives 0, because 1 is below the
    // ulp of 1e16 (which is 2). The compensated sum carries the 1 across.
    double plain = 1e16;
    plain += 1.0;
    plain -= 1e16;
    EXPECT_EQ(plain, 0.0) << "the premise: plain addition loses it";

    CompensatedSum sum;
    sum.add(1e16);
    sum.add(1.0);
    sum.add(-1e16);
    EXPECT_EQ(sum.value(), 1.0);
}

TEST(CompensatedSum, SmallTermsAddedToALargeRunningSumAreNotRoundedAway)
{
    // The case Neumaier's variant exists for. With 1e15 already in the sum,
    // the ulp is 0.125, so every 0.1 added plainly rounds up to 0.125: a
    // million of them come to 125 000 instead of 100 000, off by a quarter.
    // The compensated sum keeps each 0.1 in its compensation term and lands
    // on 100 000 once the 1e15 is taken back out.
    //
    // (A first draft added the small terms FIRST and then the large one, and
    // asserted that plain addition would visibly drift. It did not: the
    // million 0.1s accumulate only ~1e-6 of error, and the +1e15 / -1e15 round
    // trip then rounds that away to exactly 100 000 by luck. That order is
    // the case Kahan already handles; this order is the one it does not.)
    double plain = 1e15;
    for (int i = 0; i < 1000000; ++i) {
        plain += 0.1;
    }
    plain -= 1e15;
    EXPECT_NEAR(plain, 125000.0, 1.0) << "the premise: plain addition rounds each 0.1 to 0.125";

    CompensatedSum sum;
    sum.add(1e15);
    for (int i = 0; i < 1000000; ++i) {
        sum.add(0.1);
    }
    sum.add(-1e15);
    EXPECT_NEAR(sum.value(), 100000.0, 1e-6);
}

TEST(CompensatedSum, AnEmptySumIsZeroAndOrderDoesNotChangeTheAnswer)
{
    CompensatedSum empty;
    EXPECT_EQ(empty.value(), 0.0);

    // The same terms in two orders agree to the last bit or nearly so; the
    // whole point is that the order the triangles happened to come in does
    // not decide the volume (Rule 7).
    const std::vector<double> terms{1e10, 3.25, -1e10, 0.125, 7.0, -3.375, 1e-3};
    CompensatedSum forward;
    for (double term : terms) {
        forward.add(term);
    }
    CompensatedSum backward;
    for (auto it = terms.rbegin(); it != terms.rend(); ++it) {
        backward.add(*it);
    }
    EXPECT_NEAR(forward.value(), backward.value(), 1e-12);
    EXPECT_NEAR(forward.value(), 7.001, 1e-12);
}
