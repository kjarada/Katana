#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/survey/angles.hpp"
#include "katana/survey/traverse.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kDegToRad;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();
constexpr double kArcSecond = kDegToRad / 3600.0;

// Square loop A -> B -> C -> D -> A, run counter-clockwise on the map so that
// the clockwise "angles right" are the interior angles (90° each):
//   A (N1000, E1000) -> B due east -> C due north -> D due west -> A due south.
// The angles are exact; the distances carry the injected misclosure
//   sum of departures = AB - CD = 99.97 - 100.03 = -0.06
//   sum of latitudes  = BC - DA = 100.04 - 99.96 = +0.08
// so the linear misclosure is the 3-4-5 triple 0.10 m over exactly 400.00 m.
Traverse squareLoop()
{
    Traverse traverse;
    traverse.name = "square";
    traverse.kind = TraverseKind::ClosedLoop;
    traverse.start = {1000.0, 1000.0};
    traverse.startAzimuth = kHalfPi; // A -> B is due east
    traverse.setups = {
        {"A", kHalfPi, 99.97}, {"B", kHalfPi, 100.04}, {"C", kHalfPi, 100.03}, {"D", kHalfPi, 99.96}};
    return traverse;
}

// Link traverse with exact geometry, A -> P1 -> P2 -> Z:
//   leg      azimuth  distance        dN        dE
//   A  - P1     60°   200            100       100*sqrt(3)
//   P1 - P2    120°   150            -75        75*sqrt(3)
//   P2 - Z      45°   100*sqrt(2)    100       100
// Reference azimuths: A -> R1 = 210°, Z -> R2 = 100°. Clockwise angles from the
// back direction to the forward direction (forward - back, mod 360°):
//   at A : 60  - 210       = 210°
//   at P1: 120 - (60+180)  = 240°
//   at P2: 45  - (120+180) = 105°
//   at Z : 100 - (45+180)  = 235°   (closing angle)
const Coordinate2 kLinkStart{5000.0, 2000.0};
const Coordinate2 kLinkP1{5100.0, 2000.0 + 100.0 * std::sqrt(3.0)};
const Coordinate2 kLinkP2{5025.0, 2000.0 + 175.0 * std::sqrt(3.0)};
const Coordinate2 kLinkEnd{5125.0, 2100.0 + 175.0 * std::sqrt(3.0)};

Traverse linkTraverse()
{
    Traverse traverse;
    traverse.name = "link";
    traverse.kind = TraverseKind::Link;
    traverse.start = kLinkStart;
    traverse.startAzimuth = 210.0 * kDegToRad;
    traverse.setups = {{"A", 210.0 * kDegToRad, 200.0},
                       {"P1", 240.0 * kDegToRad, 150.0},
                       {"P2", 105.0 * kDegToRad, 100.0 * std::sqrt(2.0)}};
    traverse.endStationId = "Z";
    traverse.end = kLinkEnd;
    traverse.closingAngle = TraverseClosingAngle{235.0 * kDegToRad, 100.0 * kDegToRad};
    return traverse;
}

} // namespace

// ---- closed loop -----------------------------------------------------------

TEST(SurveyTraverse, ClosedLoopMisclosureAndPrecision)
{
    const auto result = computeTraverse(squareLoop());
    ASSERT_TRUE(result.ok()) << result.error().describe();

    EXPECT_EQ(result->kind, TraverseKind::ClosedLoop);
    EXPECT_EQ(result->angleCount, 4u);
    ASSERT_TRUE(result->angularMisclosure.has_value());
    EXPECT_NEAR(*result->angularMisclosure, 0.0, 1e-15); // 4 x 90° = (n-2) x 180°
    EXPECT_NEAR(result->angleCorrection, 0.0, 1e-15);
    EXPECT_NEAR(result->totalLength, 400.0, 1e-12);

    ASSERT_TRUE(result->linearMisclosure.has_value());
    const LinearMisclosure& misclosure = *result->linearMisclosure;
    EXPECT_NEAR(misclosure.latitude, 0.08, 1e-12);
    EXPECT_NEAR(misclosure.departure, -0.06, 1e-12);
    EXPECT_NEAR(misclosure.length, 0.10, 1e-12);
    EXPECT_NEAR(misclosure.relativePrecision, 0.10 / 400.0, 1e-15);
    ASSERT_TRUE(misclosure.precisionDenominator.has_value());
    EXPECT_NEAR(*misclosure.precisionDenominator, 4000.0, 1e-6); // 1 : 4000

    ASSERT_EQ(result->legs.size(), 4u);
    const double azimuths[] = {90.0, 0.0, 270.0, 180.0};
    const double latitudes[] = {0.0, 100.04, 0.0, -99.96};
    const double departures[] = {99.97, 0.0, -100.03, 0.0};
    const char* const to[] = {"B", "C", "D", "A"};
    for (std::size_t k = 0; k < 4; ++k) {
        EXPECT_NEAR(result->legs[k].azimuth, azimuths[k] * kDegToRad, 1e-14) << k;
        EXPECT_NEAR(result->legs[k].latitude, latitudes[k], 1e-12) << k;
        EXPECT_NEAR(result->legs[k].departure, departures[k], 1e-12) << k;
        EXPECT_EQ(result->legs[k].toId, to[k]);
    }
}

TEST(SurveyTraverse, CompassRuleHandVerified)
{
    const auto result = computeTraverse(squareLoop());
    ASSERT_TRUE(result.ok());

    // Compass (Bowditch) rule: correction = -misclosure * leg length / total length,
    // with misclosure (+0.08, -0.06) and total 400:
    //   leg   length   lat corr = -0.08 L/400   dep corr = +0.06 L/400
    //   AB     99.97        -0.019994               +0.0149955
    //   BC    100.04        -0.020008               +0.0150060
    //   CD    100.03        -0.020006               +0.0150045
    //   DA     99.96        -0.019992               +0.0149940
    //   sum                 -0.080000               +0.0600000   (closes exactly)
    const double latitudeCorrections[] = {-0.019994, -0.020008, -0.020006, -0.019992};
    const double departureCorrections[] = {0.0149955, 0.0150060, 0.0150045, 0.0149940};
    double sumLatitude = 0.0;
    double sumDeparture = 0.0;
    for (std::size_t k = 0; k < 4; ++k) {
        EXPECT_NEAR(result->legs[k].latitudeCorrection, latitudeCorrections[k], 1e-12) << k;
        EXPECT_NEAR(result->legs[k].departureCorrection, departureCorrections[k], 1e-12) << k;
        sumLatitude += result->legs[k].latitudeCorrection;
        sumDeparture += result->legs[k].departureCorrection;
    }
    EXPECT_NEAR(sumLatitude, -0.08, 1e-12);
    EXPECT_NEAR(sumDeparture, 0.06, 1e-12);

    // Adjusted coordinates, accumulated by hand from A (1000, 1000):
    //   B: N = 1000 + 0      - 0.019994 =  999.980006   E = 1000 + 99.97 + 0.0149955 = 1099.9849955
    //   C: N = B  + 100.04   - 0.020008 = 1099.999998   E = B + 0       + 0.0150060 = 1100.0000015
    //   D: N = C  + 0        - 0.020006 = 1099.979992   E = C - 100.03  + 0.0150045 =  999.9850060
    //   A: N = D  - 99.96    - 0.019992 = 1000.000000   E = D + 0       + 0.0149940 = 1000.0000000
    ASSERT_EQ(result->stations.size(), 4u); // the start is not repeated
    const Coordinate2 adjusted[] = {{1000.0, 1000.0},
                                    {999.980006, 1099.9849955},
                                    {1099.999998, 1100.0000015},
                                    {1099.979992, 999.985006}};
    const Coordinate2 unadjusted[] = {
        {1000.0, 1000.0}, {1000.0, 1099.97}, {1100.04, 1099.97}, {1100.04, 999.94}};
    const char* const ids[] = {"A", "B", "C", "D"};
    for (std::size_t s = 0; s < 4; ++s) {
        EXPECT_EQ(result->stations[s].id, ids[s]);
        EXPECT_NEAR(result->stations[s].adjusted.northing, adjusted[s].northing, 1e-9) << s;
        EXPECT_NEAR(result->stations[s].adjusted.easting, adjusted[s].easting, 1e-9) << s;
        EXPECT_NEAR(result->stations[s].unadjusted.northing, unadjusted[s].northing, 1e-9) << s;
        EXPECT_NEAR(result->stations[s].unadjusted.easting, unadjusted[s].easting, 1e-9) << s;
    }
    // The last adjusted leg returns to A.
    const auto& last = result->legs[3];
    EXPECT_NEAR(result->stations[3].adjusted.northing + last.latitude + last.latitudeCorrection,
                1000.0, 1e-9);
    EXPECT_NEAR(result->stations[3].adjusted.easting + last.departure + last.departureCorrection,
                1000.0, 1e-9);
}

TEST(SurveyTraverse, TransitRuleHandVerified)
{
    TraverseOptions options;
    options.adjustment = TraverseAdjustment::Transit;
    const auto result = computeTraverse(squareLoop(), options);
    ASSERT_TRUE(result.ok());

    // Transit rule: latitude corrections in proportion to |latitude|, departure
    // corrections in proportion to |departure|.
    //   sum |lat| = 100.04 + 99.96 = 200   sum |dep| = 99.97 + 100.03 = 200
    //   BC lat corr = -0.08 * 100.04/200 = -0.040016   DA: -0.08 * 99.96/200 = -0.039984
    //   AB dep corr = +0.06 * 99.97/200  = +0.029991   CD: +0.06 * 100.03/200 = +0.030009
    // East-west legs get no latitude correction and vice versa.
    const double latitudeCorrections[] = {0.0, -0.040016, 0.0, -0.039984};
    const double departureCorrections[] = {0.029991, 0.0, 0.030009, 0.0};
    for (std::size_t k = 0; k < 4; ++k) {
        EXPECT_NEAR(result->legs[k].latitudeCorrection, latitudeCorrections[k], 1e-12) << k;
        EXPECT_NEAR(result->legs[k].departureCorrection, departureCorrections[k], 1e-12) << k;
    }
}

TEST(SurveyTraverse, NoAdjustmentReportsMisclosureOnly)
{
    TraverseOptions options;
    options.adjustment = TraverseAdjustment::None;
    const auto result = computeTraverse(squareLoop(), options);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->linearMisclosure->length, 0.10, 1e-12);
    for (const auto& leg : result->legs) {
        EXPECT_DOUBLE_EQ(leg.latitudeCorrection, 0.0);
        EXPECT_DOUBLE_EQ(leg.departureCorrection, 0.0);
    }
    for (const auto& station : result->stations) {
        EXPECT_EQ(station.adjusted, station.unadjusted);
    }
}

TEST(SurveyTraverse, AngularMisclosureIsBalancedEquallyAndAzimuthsWrap)
{
    // Observed interior angles sum to 360°00'20": misclosure +20", -5" per angle.
    //   observed  A 90°00'20"   B 89°59'50"   C 90°00'05"   D 90°00'05"
    //   balanced  A 90°00'15"   B 89°59'45"   C 90°00'00"   D 90°00'00"
    // Azimuths from the balanced angles (previous + 180° + angle):
    //   AB = 90° (given)
    //   BC = 90° + 180° + 89°59'45"      = 359°59'45"   (just below the wrap)
    //   CD = 359°59'45" + 180° + 90°     = 269°59'45"
    //   DA = 269°59'45" + 180° + 90°     = 179°59'45"
    //   check: 179°59'45" + 180° + 90°00'15" = 450° = 90°.
    Traverse traverse = squareLoop();
    traverse.setups[0].angle = kHalfPi + 20.0 * kArcSecond;
    traverse.setups[1].angle = kHalfPi - 10.0 * kArcSecond;
    traverse.setups[2].angle = kHalfPi + 5.0 * kArcSecond;
    traverse.setups[3].angle = kHalfPi + 5.0 * kArcSecond;

    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    ASSERT_TRUE(result->angularMisclosure.has_value());
    EXPECT_NEAR(*result->angularMisclosure, 20.0 * kArcSecond, 1e-14);
    EXPECT_NEAR(result->angleCorrection, -5.0 * kArcSecond, 1e-14);

    EXPECT_NEAR(result->legs[0].azimuth, kHalfPi, 1e-14);
    EXPECT_NEAR(result->legs[1].azimuth, kTwoPi - 15.0 * kArcSecond, 1e-13);
    EXPECT_NEAR(result->legs[2].azimuth, 1.5 * kPi - 15.0 * kArcSecond, 1e-13);
    EXPECT_NEAR(result->legs[3].azimuth, kPi - 15.0 * kArcSecond, 1e-13);
    for (const auto& leg : result->legs) {
        EXPECT_GE(leg.azimuth, 0.0);
        EXPECT_LT(leg.azimuth, kTwoPi);
    }

    // Without balancing the raw angles are carried: BC = 90° + 180° + 89°59'50".
    TraverseOptions raw;
    raw.balanceAngles = false;
    const auto unbalanced = computeTraverse(traverse, raw);
    ASSERT_TRUE(unbalanced.ok());
    EXPECT_NEAR(*unbalanced->angularMisclosure, 20.0 * kArcSecond, 1e-14);
    EXPECT_DOUBLE_EQ(unbalanced->angleCorrection, 0.0);
    EXPECT_NEAR(unbalanced->legs[1].azimuth, kTwoPi - 10.0 * kArcSecond, 1e-13);
}

TEST(SurveyTraverse, ClockwiseLoopUsesExteriorAngles)
{
    // The same square run the other way (A -> D -> C -> B): the angles right are
    // now the exterior angles, 270° each, summing to (n + 2) x 180° = 1080°.
    Traverse traverse;
    traverse.kind = TraverseKind::ClosedLoop;
    traverse.start = {1000.0, 1000.0};
    traverse.startAzimuth = 0.0; // A -> D is due north
    traverse.setups = {{"A", 1.5 * kPi, 100.0},
                       {"D", 1.5 * kPi, 100.0},
                       {"C", 1.5 * kPi, 100.0},
                       {"B", 1.5 * kPi, 100.0}};
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(*result->angularMisclosure, 0.0, 1e-14);
    EXPECT_NEAR(result->linearMisclosure->length, 0.0, 1e-12);
    // D north of A, C north-east, B east.
    EXPECT_NEAR(result->stations[1].adjusted.northing, 1100.0, 1e-10);
    EXPECT_NEAR(result->stations[1].adjusted.easting, 1000.0, 1e-10);
    EXPECT_NEAR(result->stations[2].adjusted.easting, 1100.0, 1e-10);
    EXPECT_NEAR(result->stations[3].adjusted.northing, 1000.0, 1e-10);
    EXPECT_NEAR(result->stations[3].adjusted.easting, 1100.0, 1e-10);
}

TEST(SurveyTraverse, ConsistentLoopClosesToRoundingNoise)
{
    // A -> B north, then west, south, east: a consistent 100 m square. It closes
    // exactly on paper; in doubles cos(pi/2) is 6e-17 rather than 0, so the
    // misclosure is rounding noise (~1e-14 m), far below any survey meaning.
    Traverse traverse;
    traverse.kind = TraverseKind::ClosedLoop;
    traverse.start = {0.0, 0.0};
    traverse.startAzimuth = 0.0;
    traverse.setups = {{"A", kHalfPi, 100.0}, {"B", kHalfPi, 100.0}, {"C", kHalfPi, 100.0},
                       {"D", kHalfPi, 100.0}};
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    EXPECT_LT(result->linearMisclosure->length, 1e-12);
    EXPECT_LT(result->linearMisclosure->relativePrecision, 1e-14);
}

TEST(SurveyTraverse, ExactClosureHasNoPrecisionDenominator)
{
    // Due north is the one direction whose sine and cosine are exact (0 and 1), so
    // this single leg closes with a misclosure of exactly zero: "1 : infinity" is
    // reported as an absent denominator, not as a division by zero.
    Traverse traverse;
    traverse.kind = TraverseKind::Link;
    traverse.start = {1000.0, 2000.0};
    traverse.startAzimuth = 0.0;
    traverse.setups = {{"A", 0.0, 100.0}};
    traverse.endStationId = "Z";
    traverse.end = {1100.0, 2000.0};
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    ASSERT_TRUE(result->linearMisclosure.has_value());
    EXPECT_DOUBLE_EQ(result->linearMisclosure->length, 0.0);
    EXPECT_DOUBLE_EQ(result->linearMisclosure->relativePrecision, 0.0);
    EXPECT_FALSE(result->linearMisclosure->precisionDenominator.has_value());
    EXPECT_DOUBLE_EQ(result->legs[0].latitudeCorrection, 0.0);
}

// ---- link traverse ---------------------------------------------------------

TEST(SurveyTraverse, LinkTraverseWithExactObservationsCloses)
{
    const auto result = computeTraverse(linkTraverse());
    ASSERT_TRUE(result.ok()) << result.error().describe();

    EXPECT_EQ(result->angleCount, 4u); // three setups plus the closing angle
    ASSERT_TRUE(result->angularMisclosure.has_value());
    EXPECT_NEAR(*result->angularMisclosure, 0.0, 1e-14);
    ASSERT_TRUE(result->linearMisclosure.has_value());
    EXPECT_NEAR(result->linearMisclosure->length, 0.0, 1e-11);

    EXPECT_NEAR(result->legs[0].azimuth, 60.0 * kDegToRad, 1e-14);
    EXPECT_NEAR(result->legs[1].azimuth, 120.0 * kDegToRad, 1e-14);
    EXPECT_NEAR(result->legs[2].azimuth, 45.0 * kDegToRad, 1e-14);

    ASSERT_EQ(result->stations.size(), 4u);
    const Coordinate2 expected[] = {kLinkStart, kLinkP1, kLinkP2, kLinkEnd};
    const char* const ids[] = {"A", "P1", "P2", "Z"};
    for (std::size_t s = 0; s < 4; ++s) {
        EXPECT_EQ(result->stations[s].id, ids[s]);
        EXPECT_NEAR(result->stations[s].adjusted.northing, expected[s].northing, 1e-10) << s;
        EXPECT_NEAR(result->stations[s].adjusted.easting, expected[s].easting, 1e-10) << s;
    }
    EXPECT_EQ(result->stations.back().adjusted, kLinkEnd); // control is stated exactly
}

TEST(SurveyTraverse, LinkTraverseBalancesInjectedAngularMisclosure)
{
    // +6" on each of the four angles: misclosure +24", correction -6" each, which
    // restores the exact angles and therefore the exact coordinates.
    Traverse traverse = linkTraverse();
    for (TraverseSetup& setup : traverse.setups) {
        setup.angle += 6.0 * kArcSecond;
    }
    traverse.closingAngle->angle += 6.0 * kArcSecond;

    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(*result->angularMisclosure, 24.0 * kArcSecond, 1e-13);
    EXPECT_NEAR(result->angleCorrection, -6.0 * kArcSecond, 1e-13);
    EXPECT_NEAR(result->linearMisclosure->length, 0.0, 1e-9);
    EXPECT_NEAR(result->stations[1].unadjusted.northing, kLinkP1.northing, 1e-9);
    EXPECT_NEAR(result->stations[2].unadjusted.easting, kLinkP2.easting, 1e-9);

    // Unbalanced, the 6", 12", 18" azimuth errors swing the far end sideways by
    // roughly d * error: visible at the millimetre level over these legs.
    TraverseOptions raw;
    raw.balanceAngles = false;
    raw.adjustment = TraverseAdjustment::None;
    const auto unbalanced = computeTraverse(traverse, raw);
    ASSERT_TRUE(unbalanced.ok());
    EXPECT_GT(unbalanced->linearMisclosure->length, 0.005);
    EXPECT_LT(unbalanced->linearMisclosure->length, 0.05);
}

TEST(SurveyTraverse, LinkTraverseWithoutClosingAngleHasOnlyALinearCheck)
{
    Traverse traverse = linkTraverse();
    traverse.closingAngle.reset();
    traverse.end.easting += 0.03; // known end 3 cm further east than the traverse arrives
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    EXPECT_FALSE(result->angularMisclosure.has_value());
    EXPECT_EQ(result->angleCount, 0u);
    EXPECT_DOUBLE_EQ(result->angleCorrection, 0.0);
    // misclosure = computed - known
    EXPECT_NEAR(result->linearMisclosure->departure, -0.03, 1e-10);
    EXPECT_NEAR(result->linearMisclosure->latitude, 0.0, 1e-10);
    // Compass rule spreads +0.03 of easting by length: 200 : 150 : 141.42.
    const double total = 350.0 + 100.0 * std::sqrt(2.0);
    EXPECT_NEAR(result->legs[0].departureCorrection, 0.03 * 200.0 / total, 1e-12);
    EXPECT_NEAR(result->stations[1].adjusted.easting, kLinkP1.easting + 0.03 * 200.0 / total,
                1e-10);
}

// ---- open traverse ---------------------------------------------------------

TEST(SurveyTraverse, OpenTraverseHasNoClosure)
{
    Traverse traverse = linkTraverse();
    traverse.kind = TraverseKind::Open;
    traverse.closingAngle.reset();
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    EXPECT_FALSE(result->angularMisclosure.has_value());
    EXPECT_FALSE(result->linearMisclosure.has_value());
    ASSERT_EQ(result->stations.size(), 4u);
    EXPECT_NEAR(result->stations[3].adjusted.northing, kLinkEnd.northing, 1e-10);
    EXPECT_NEAR(result->stations[3].adjusted.easting, kLinkEnd.easting, 1e-10);
    EXPECT_EQ(result->stations[3].adjusted, result->stations[3].unadjusted);
}

TEST(SurveyTraverse, SingleLegOpenTraverse)
{
    // Backsight due north (azimuth 0), 90° to the right, 50 m: due east.
    Traverse traverse;
    traverse.kind = TraverseKind::Open;
    traverse.start = {100.0, 200.0};
    traverse.startAzimuth = 0.0;
    traverse.setups = {{"A", kHalfPi, 50.0}};
    traverse.endStationId = "P";
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    ASSERT_EQ(result->legs.size(), 1u);
    ASSERT_EQ(result->stations.size(), 2u);
    EXPECT_EQ(result->stations[1].id, "P");
    EXPECT_NEAR(result->stations[1].adjusted.northing, 100.0, 1e-13);
    EXPECT_NEAR(result->stations[1].adjusted.easting, 250.0, 1e-13);
    EXPECT_DOUBLE_EQ(result->totalLength, 50.0);
}

TEST(SurveyTraverse, UtmSizedStartKeepsMillimetres)
{
    Traverse traverse = linkTraverse();
    const double dN = 5000000.0;
    const double dE = 500000.0;
    traverse.start = {kLinkStart.northing + dN, kLinkStart.easting + dE};
    traverse.end = {kLinkEnd.northing + dN, kLinkEnd.easting + dE};
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok());
    // Latitudes and departures are summed before they meet the large start
    // coordinates, so the closure is not polluted by 1e-9 m rounding per leg.
    EXPECT_LT(result->linearMisclosure->length, 5e-9);
    EXPECT_NEAR(result->stations[2].adjusted.northing, kLinkP2.northing + dN, 5e-9);
    EXPECT_NEAR(result->stations[2].adjusted.easting, kLinkP2.easting + dE, 5e-9);
}

// ---- validation ------------------------------------------------------------

TEST(SurveyTraverse, RejectsMalformedTraverses)
{
    const auto expectInvalid = [](const Traverse& traverse, const char* why) {
        const auto result = computeTraverse(traverse);
        ASSERT_FALSE(result.ok()) << why;
        EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument) << why;
        EXPECT_FALSE(validateTraverse(traverse).ok()) << why;
    };

    Traverse empty;
    expectInvalid(empty, "no setups");

    Traverse shortLoop = squareLoop();
    shortLoop.setups.resize(2);
    expectInvalid(shortLoop, "loop with two setups");

    Traverse repeated = squareLoop();
    repeated.setups[2].stationId = "A";
    expectInvalid(repeated, "repeated station id");

    Traverse unnamed = squareLoop();
    unnamed.setups[1].stationId.clear();
    expectInvalid(unnamed, "empty station id");

    for (const double distance : {0.0, -10.0, kNaN}) {
        Traverse bad = squareLoop();
        bad.setups[1].distance = distance;
        expectInvalid(bad, "bad distance");
    }

    Traverse badAngle = squareLoop();
    badAngle.setups[0].angle = kNaN;
    expectInvalid(badAngle, "non-finite angle");

    Traverse badStart = squareLoop();
    badStart.start.northing = kNaN;
    expectInvalid(badStart, "non-finite start");

    Traverse noEnd = linkTraverse();
    noEnd.endStationId.clear();
    expectInvalid(noEnd, "missing end station id");

    Traverse endRepeats = linkTraverse();
    endRepeats.endStationId = "P1";
    expectInvalid(endRepeats, "end id repeats an interior station");

    Traverse badEnd = linkTraverse();
    badEnd.end.easting = kNaN;
    expectInvalid(badEnd, "non-finite end");

    Traverse badClosing = linkTraverse();
    badClosing.closingAngle->referenceAzimuth = kNaN;
    expectInvalid(badClosing, "non-finite closing azimuth");

    EXPECT_TRUE(validateTraverse(squareLoop()).ok());
    EXPECT_TRUE(validateTraverse(linkTraverse()).ok());
}

TEST(SurveyTraverse, LinkMayCloseOnItsOwnStartButOnlyConsistently)
{
    // A loop oriented on an external reference mark: a link whose end is its start.
    Traverse traverse;
    traverse.kind = TraverseKind::Link;
    traverse.start = {1000.0, 1000.0};
    traverse.end = {1000.0, 1000.0};
    traverse.endStationId = "A";
    traverse.startAzimuth = kPi; // reference mark due south of A
    // A -> B east (from south, 270° clockwise reaches east), then the square.
    traverse.setups = {{"A", 1.5 * kPi, 100.0},
                       {"B", kHalfPi, 100.0},
                       {"C", kHalfPi, 100.0},
                       {"D", kHalfPi, 100.0}};
    // At A again: back direction D (due north, azimuth 0) to the mark (180°).
    traverse.closingAngle = TraverseClosingAngle{kPi, kPi};
    const auto result = computeTraverse(traverse);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(*result->angularMisclosure, 0.0, 1e-14);
    EXPECT_NEAR(result->linearMisclosure->length, 0.0, 1e-11);
    EXPECT_EQ(result->stations.back().id, "A");

    traverse.end.easting += 1.0; // same id, different coordinates: contradictory
    EXPECT_FALSE(computeTraverse(traverse).ok());
}
