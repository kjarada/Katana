// Chording curves to a stated accuracy (include/katana/geometry/chording.hpp).
//
// These lived in the cad scene tests while the sagitta rule lived in
// cad::scene. The rule moved down to geometry so that the spiral and the
// alignment could share it, and the tests moved with it.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/geometry/chording.hpp"
#include "katana/math/numerics.hpp"

using katana::geometry::Arc2;
using katana::geometry::chordArc;
using katana::geometry::chordCircle;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::sagittaChordCount;
using katana::math::kTwoPi;

TEST(Chording, ChordingAnArcHonoursItsSagittaTolerance)
{
    const Arc2 arc{Point2(0.0, 0.0), 50.0, 0.0, kTwoPi * 0.25};

    for (double tolerance : {1.0, 0.1, 0.01, 0.001}) {
        const auto points = chordArc(arc, tolerance);
        ASSERT_GE(points.size(), 2u);
        EXPECT_EQ(points.front(), arc.startPoint());
        EXPECT_NEAR(points.back().distanceTo(arc.endPoint()), 0.0, 1e-9);

        // The true test: the midpoint of every chord must be within the
        // tolerance of the arc. That is the definition of the sagitta rule.
        for (std::size_t i = 1; i < points.size(); ++i) {
            const Point2 mid = (points[i - 1] + points[i]) * 0.5;
            const double deviation = std::abs(mid.distanceTo(arc.center) - arc.radius);
            EXPECT_LE(deviation, tolerance * 1.001)
                << "tolerance " << tolerance << " chord " << i;
        }
        EXPECT_LE(points.size(), 8193u);
    }

    EXPECT_GT(chordArc(arc, 0.001).size(), chordArc(arc, 0.1).size())
        << "a tighter tolerance must produce more chords";
}

TEST(Chording, ChordingACircleDoesNotRepeatTheClosingPoint)
{
    const Circle2 circle{Point2(10.0, 20.0), 5.0};
    const auto points = chordCircle(circle, 0.01);
    ASSERT_GE(points.size(), 3u);
    // The polyline is closed by the caller, so a duplicate first/last point
    // would draw a zero-length segment at the seam.
    EXPECT_GT(points.front().distanceTo(points.back()), 1e-6);
    for (const Point2& p : points) {
        EXPECT_NEAR(p.distanceTo(circle.center), circle.radius, 1e-9);
    }
}

TEST(Chording, ChordingADegenerateArcDoesNotHang)
{
    // Zero radius, zero sweep and an absurd tolerance have all produced
    // infinite loops in tessellators before.
    EXPECT_TRUE(chordArc(Arc2{Point2(0, 0), 0.0, 0.0, 1.0}, 0.01).empty());
    EXPECT_LE(chordArc(Arc2{Point2(0, 0), 10.0, 0.0, 0.0}, 0.01).size(), 2u);
    EXPECT_GE(chordArc(Arc2{Point2(0, 0), 10.0, 0.0, kTwoPi}, 1e9).size(), 2u);
    EXPECT_LE(chordArc(Arc2{Point2(0, 0), 1e9, 0.0, kTwoPi}, 1e-12).size(), 8193u)
        << "a hairline tolerance on a huge radius must still be bounded";
}

TEST(Chording, TheCountRuleIsTheOneTheArcAndTheSpiralBothUse)
{
    // chordArc produces count + 1 points for count chords; the spiral asks
    // the same rule directly. If the two ever diverged, an alignment drawn
    // through a spiral would step to a different resolution at the tangent
    // point than the arc beside it.
    const Arc2 arc{Point2(0.0, 0.0), 300.0, 0.0, 0.3};
    const std::size_t count = sagittaChordCount(300.0, 0.3, 0.001);
    EXPECT_EQ(chordArc(arc, 0.001).size(), count + 1);
    // Sagitta at r = 300, tol = 1 mm: phi = 2 acos(1 - 1/300000) = 0.0051640
    // rad, so a 0.3 rad sweep needs ceil(58.095) = 59 chords - the same
    // number the spiral test derives for its 90 m at the same radius, because
    // 90 m on r = 300 IS a 0.3 rad sweep.
    EXPECT_EQ(count, 59u);
}
