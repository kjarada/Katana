// The clothoid (include/katana/geometry/spiral2.hpp).
//
// Every reference value here comes from OUTSIDE the code under test, by at
// least one of three routes that share nothing with it: the published Fresnel
// integrals (Abramowitz & Stegun, Table 7.7), the power series of the
// road-design texts derived by hand from the Fresnel integrand, and an
// adaptive-Simpson integration written in Python for the purpose. Where two
// of those were available they agree to every digit quoted.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/geometry/spiral2.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::geometry;
using katana::math::kPi;

namespace {

// A spiral from a straight with A * sqrt(pi) = 1, so that its coordinates at
// distance s are exactly the Fresnel integrals C(s) and S(s) in the pi/2
// convention the tables use: R * L = 1 / pi, and L = 1 is convenient.
Spiral2 fresnelSpiral()
{
    return Spiral2::fromStraightToRadius(Point2(0, 0), 0.0, 1.0 / kPi, 1.0);
}

} // namespace

// ---- against published constants -------------------------------------------------

TEST(Spiral2, ReproducesThePublishedFresnelIntegrals)
{
    // Abramowitz & Stegun Table 7.7, confirmed to 16 digits by an independent
    // adaptive-Simpson integration of the raw integrand:
    //   C(0.5) = 0.4923442258714464   S(0.5) = 0.0647324328599993
    //   C(1.0) = 0.7798934003768228   S(1.0) = 0.4382591473903548
    //   C(2.0) = 0.4882534060753407   S(2.0) = 0.3434156783636982
    // At s = 2 the spiral has turned through 2 pi radians - a full circle and
    // well beyond any real transition - so this also exercises the multi-panel
    // path of the quadrature, not only the small-angle case.
    const Spiral2 spiral = fresnelSpiral();
    const Point2 half = spiral.pointAt(0.5);
    EXPECT_NEAR(half.x, 0.4923442258714464, 1e-14);
    EXPECT_NEAR(half.y, 0.0647324328599993, 1e-14);
    const Point2 one = spiral.pointAt(1.0);
    EXPECT_NEAR(one.x, 0.7798934003768228, 1e-14);
    EXPECT_NEAR(one.y, 0.4382591473903548, 1e-14);
    const Point2 two = spiral.pointAt(2.0);
    EXPECT_NEAR(two.x, 0.4882534060753407, 1e-14);
    EXPECT_NEAR(two.y, 0.3434156783636982, 1e-14);
}

TEST(Spiral2, MatchesTheHighwayDesignSeriesOnARealTransition)
{
    // R = 300 m, L = 90 m: an ordinary rural-road transition. The end offsets
    // from the series x = L(1 - t^2/10 + t^4/216 - ...), y = L(t/3 - t^3/42 +
    // ...), with t = L / 2R = 0.15, derived by hand from the Fresnel integrand
    // and confirmed by adaptive Simpson to twelve decimals:
    //   X = 89.797710828008   Y = 4.492773032666
    const Spiral2 spiral = Spiral2::fromStraightToRadius(Point2(0, 0), 0.0, 300.0, 90.0);
    const Point2 end = spiral.endPoint();
    EXPECT_NEAR(end.x, 89.797710828008, 1e-11);
    EXPECT_NEAR(end.y, 4.492773032666, 1e-11);
    // t = 0.15 = 90 / 600 is the deflection at the end, exactly.
    EXPECT_NEAR(spiral.totalTurn(), 0.15, 1e-15);
    EXPECT_NEAR(spiral.flatness(), std::sqrt(300.0 * 90.0), 1e-12); // A = sqrt(RL)
}

TEST(Spiral2, HandlesTheCurveToCurveTransitionTheTextbookFormHasNoFormulaFor)
{
    // R 200 m into R 100 m over 60 m, entered heading 30 degrees. There is no
    // series for this; the reference is adaptive Simpson on the integrand:
    //   x = 44.587806890670   y = 39.384588253296
    Spiral2 spiral;
    spiral.start = Point2(0, 0);
    spiral.startDirection = kPi / 6.0;
    spiral.startCurvature = 1.0 / 200.0;
    spiral.endCurvature = 1.0 / 100.0;
    spiral.length = 60.0;
    const Point2 end = spiral.endPoint();
    EXPECT_NEAR(end.x, 44.587806890670, 1e-11);
    EXPECT_NEAR(end.y, 39.384588253296, 1e-11);
    // Total turn (k0 + k1) L / 2 = (1/200 + 1/100) * 30 = 0.45, exactly.
    EXPECT_NEAR(spiral.totalTurn(), 0.45, 1e-15);
}

TEST(Spiral2, AReverseSpiralThroughZeroCurvatureIsStillIntegratedFinelyEnough)
{
    // +1/100 to -1/100 over 80 m: turns left, straightens, turns right. The
    // signed total turn is exactly zero, which is the trap - a panel count
    // taken from the signed turn would be one panel over a curve that bends
    // both ways. Adaptive Simpson reference:
    //   x = 79.148831439120   y = 10.617983504393
    Spiral2 spiral;
    spiral.start = Point2(0, 0);
    spiral.startCurvature = 1.0 / 100.0;
    spiral.endCurvature = -1.0 / 100.0;
    spiral.length = 80.0;
    EXPECT_EQ(spiral.totalTurn(), 0.0);
    const Point2 end = spiral.endPoint();
    EXPECT_NEAR(end.x, 79.148831439120, 1e-11);
    EXPECT_NEAR(end.y, 10.617983504393, 1e-11);
}

// ---- against closed forms --------------------------------------------------------

TEST(Spiral2, ConstantCurvatureIsExactlyACircularArc)
{
    // k0 == k1 == 1/50 over a quarter circle from the origin heading +x: the
    // centre is (0, 50) and the end is (50, 50), in closed form.
    Spiral2 spiral;
    spiral.start = Point2(0, 0);
    spiral.startCurvature = 1.0 / 50.0;
    spiral.endCurvature = 1.0 / 50.0;
    spiral.length = 50.0 * kPi / 2.0;
    EXPECT_TRUE(spiral.isDegenerate());
    const Point2 end = spiral.endPoint();
    EXPECT_NEAR(end.x, 50.0, 1e-12);
    EXPECT_NEAR(end.y, 50.0, 1e-12);

    const auto arc = spiral.asArc();
    ASSERT_TRUE(arc.has_value());
    EXPECT_NEAR(arc->center.x, 0.0, 1e-12);
    EXPECT_NEAR(arc->center.y, 50.0, 1e-12);
    EXPECT_DOUBLE_EQ(arc->radius, 50.0);
    EXPECT_NEAR(arc->sweep, kPi / 2.0, 1e-15);
    // The arc's own arithmetic lands on the same points as the quadrature.
    for (double t : {0.0, 0.25, 0.5, 0.75, 1.0}) {
        const Point2 fromArc = arc->pointAt(t);
        const Point2 fromSpiral = spiral.pointAt(t * spiral.length);
        EXPECT_NEAR(fromArc.x, fromSpiral.x, 1e-12) << "t=" << t;
        EXPECT_NEAR(fromArc.y, fromSpiral.y, 1e-12) << "t=" << t;
    }
}

TEST(Spiral2, ARightHandArcPutsItsCentreOnTheRight)
{
    Spiral2 spiral;
    spiral.start = Point2(10, 20);
    spiral.startDirection = kPi / 2.0; // heading +y
    spiral.startCurvature = -1.0 / 8.0;
    spiral.endCurvature = -1.0 / 8.0;
    spiral.length = 8.0 * kPi; // a full half turn... and then some: pi * 8 / 8 = pi rad
    const auto arc = spiral.asArc();
    ASSERT_TRUE(arc.has_value());
    // Heading +y and turning right, the centre is at +x of the start.
    EXPECT_NEAR(arc->center.x, 18.0, 1e-12);
    EXPECT_NEAR(arc->center.y, 20.0, 1e-12);
    EXPECT_NEAR(arc->sweep, -kPi, 1e-15); // clockwise, as Arc2 signs it
}

TEST(Spiral2, ZeroCurvatureThroughoutIsAStraightLine)
{
    Spiral2 spiral;
    spiral.start = Point2(3, 4);
    spiral.startDirection = std::atan2(4.0, 3.0); // along (0.6, 0.8), exact in binary
    spiral.length = 10.0;
    const Point2 end = spiral.endPoint();
    EXPECT_NEAR(end.x, 3.0 + 6.0, 1e-13);
    EXPECT_NEAR(end.y, 4.0 + 8.0, 1e-13);
    EXPECT_FALSE(spiral.asArc().has_value()) << "a line is not an arc";
    EXPECT_EQ(spiral.chordCountFor(0.001), 2u) << "a line needs only its two ends";
    EXPECT_TRUE(std::isinf(spiral.radiusAt(5.0)));
}

// ---- internal consistency --------------------------------------------------------

TEST(Spiral2, TheTangentIsTheDerivativeOfThePosition)
{
    // Central difference of pointAt against tangentAt, at several stations of
    // a real transition. A quadrature that integrated the wrong function, or
    // a directionAt with the wrong quadratic term, would fail this whatever
    // the reference tables said.
    const Spiral2 spiral = Spiral2::fromStraightToRadius(Point2(0, 0), 0.3, 150.0, 60.0);
    const double h = 1e-4;
    for (double s : {5.0, 20.0, 45.0, 60.0}) {
        const Point2 ahead = spiral.pointAt(s + h);
        const Point2 behind = spiral.pointAt(s - h);
        const auto tangent = spiral.tangentAt(s);
        EXPECT_NEAR((ahead.x - behind.x) / (2 * h), tangent.x, 1e-7) << "s=" << s;
        EXPECT_NEAR((ahead.y - behind.y) / (2 * h), tangent.y, 1e-7) << "s=" << s;
        EXPECT_NEAR(tangent.length(), 1.0, 1e-15);
    }
}

TEST(Spiral2, AMirroredSpiralIsExactlyTheMirrorImage)
{
    // Negating the curvatures and the heading reflects the spiral in the x
    // axis. The quadrature evaluates its symmetric node pairs together, so
    // cos(-t) == cos(t) and sin(-t) == -sin(t) give an EXACT reflection, not
    // one to within rounding - which is what lets a survey compare a left-hand
    // and a right-hand design without a tolerance.
    const Spiral2 left = Spiral2::fromStraightToRadius(Point2(0, 0), 0.0, 300.0, 90.0);
    const Spiral2 right = Spiral2::fromStraightToRadius(Point2(0, 0), 0.0, -300.0, 90.0);
    for (double s : {10.0, 37.5, 90.0}) {
        const Point2 a = left.pointAt(s);
        const Point2 b = right.pointAt(s);
        EXPECT_EQ(a.x, b.x) << "s=" << s;
        EXPECT_EQ(a.y, -b.y) << "s=" << s;
    }
}

TEST(Spiral2, CurvatureAndDirectionAreExactlyLinearAndQuadratic)
{
    Spiral2 spiral;
    spiral.startCurvature = 0.0;
    spiral.endCurvature = 1.0 / 256.0; // exact in binary
    spiral.length = 128.0;
    EXPECT_EQ(spiral.curvatureAt(64.0), 1.0 / 512.0);
    // direction = k1 * s^2 / (2L) = (1/256)(128^2)/(256) = 16384/65536 = 0.25
    EXPECT_EQ(spiral.directionAt(128.0), 0.25);
    EXPECT_EQ(spiral.totalTurn(), 0.25);
    EXPECT_EQ(spiral.radiusAt(128.0), 256.0);
}

TEST(Spiral2, ChordCountTightensWithToleranceAndNeverFallsBelowTwo)
{
    const Spiral2 spiral = Spiral2::fromStraightToRadius(Point2(0, 0), 0.0, 300.0, 90.0);
    const std::size_t coarse = spiral.chordCountFor(0.1);
    const std::size_t fine = spiral.chordCountFor(0.001);
    EXPECT_GE(coarse, 2u);
    EXPECT_GT(fine, coarse);
    // Sagitta at the tightest radius: r(1 - cos(phi/2)) = tol gives, for
    // r = 300 and tol = 0.001, phi = 2 acos(1 - 1/300000) = 0.0051640 rad -
    // confirmed by the small-angle identity acos(1 - e) ~ sqrt(2e), which
    // gives 2 sqrt(2/300000) = 0.0051640 - so a chord of 300 * phi = 1.5492 m,
    // and 90 m needs ceil(58.095) = 59 chords, 60 points.
    //
    // A first draft of this comment said 38, from a slip in the arithmetic;
    // the two derivations above were done before the test was re-run, and
    // agree with each other, which is what CLAUDE.md section 3 asks for.
    EXPECT_EQ(fine, 60u);
}
