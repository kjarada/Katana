// The vertical alignment (include/katana/geometry/profile.hpp).
//
// Reference values are the standard textbook vertical curves, worked by hand
// from z = z_PVC + g1 x + (g2 - g1) x^2 / 2L and confirmed independently by
// evaluating that parabola directly and finding the extremum by a fine scan
// rather than by the closed form the code uses. Every number below is exact
// in decimal and the tolerances are rounding only.

#include <gtest/gtest.h>

#include <limits>
#include <string>
#include <vector>

#include "katana/geometry/profile.hpp"

using namespace katana::geometry;
using katana::core::ErrorCode;

namespace {

// A sag: -3% into +2% through a 200 m curve at PVI 1000 / 100.0.
//   PVC 900 @ 103.0, PVT 1100 @ 102.0, low point 1020 @ 101.2.
VerticalAlignment sag()
{
    VerticalAlignment profile;
    profile.pvis = {ProfilePVI{800.0, 106.0}, ProfilePVI{1000.0, 100.0, 200.0},
                    ProfilePVI{1300.0, 106.0}};
    return profile;
}

// A crest: +4% into -2% through a 300 m curve at PVI 500 / 150.0.
//   PVC 350 @ 144.0, PVT 650 @ 147.0, high point 550 @ 148.0.
VerticalAlignment crest()
{
    VerticalAlignment profile;
    profile.pvis = {ProfilePVI{200.0, 138.0}, ProfilePVI{500.0, 150.0, 300.0},
                    ProfilePVI{900.0, 142.0}};
    return profile;
}

std::vector<ProfileElementKind> kinds(const SolvedProfile& solved)
{
    std::vector<ProfileElementKind> out;
    for (const ProfileElement& element : solved.elements()) {
        out.push_back(element.kind);
    }
    return out;
}

} // namespace

TEST(Profile, ASagCurveMatchesTheTextbook)
{
    const auto solved = solveProfile(sag());
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const std::vector<ProfileElementKind> expected{
        ProfileElementKind::Tangent, ProfileElementKind::Curve, ProfileElementKind::Tangent};
    EXPECT_EQ(kinds(*solved), expected);
    EXPECT_EQ(solved->startStation(), 800.0);
    EXPECT_EQ(solved->endStation(), 1300.0);

    EXPECT_NEAR(*solved->elevationAt(900.0), 103.0, 1e-12);  // PVC
    EXPECT_NEAR(*solved->elevationAt(1100.0), 102.0, 1e-12); // PVT
    EXPECT_NEAR(*solved->elevationAt(1020.0), 101.2, 1e-12); // low point
    EXPECT_NEAR(*solved->gradeAt(850.0), -0.03, 1e-15);
    EXPECT_NEAR(*solved->gradeAt(1200.0), 0.02, 1e-15);
    EXPECT_NEAR(*solved->gradeAt(1020.0), 0.0, 1e-15);

    const auto lows = solved->highLowPoints();
    ASSERT_EQ(lows.size(), 1u);
    EXPECT_FALSE(lows[0].high);
    EXPECT_NEAR(lows[0].station, 1020.0, 1e-9);
    EXPECT_NEAR(lows[0].elevation, 101.2, 1e-12);
    EXPECT_EQ(lows[0].pvi, 1u);

    const std::vector<double> keys{800.0, 900.0, 1100.0, 1300.0};
    EXPECT_EQ(solved->keyStations(), keys);
}

TEST(Profile, ACrestCurveMatchesTheTextbook)
{
    const auto solved = solveProfile(crest());
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    EXPECT_NEAR(*solved->elevationAt(350.0), 144.0, 1e-12);
    EXPECT_NEAR(*solved->elevationAt(650.0), 147.0, 1e-12);
    EXPECT_NEAR(*solved->elevationAt(550.0), 148.0, 1e-12);
    const auto highs = solved->highLowPoints();
    ASSERT_EQ(highs.size(), 1u);
    EXPECT_TRUE(highs[0].high);
    EXPECT_NEAR(highs[0].station, 550.0, 1e-9);
    EXPECT_NEAR(highs[0].elevation, 148.0, 1e-12);
}

TEST(Profile, EveryJointIsContinuousInElevationAndGrade)
{
    // Three curves in a row, including a sag and a crest. The PVT elevation of
    // a parabola is z_PVI + g2 L / 2 by identity, which is exactly where the
    // next tangent starts; this checks that identity survived the arithmetic
    // at every joint, from both sides, in elevation AND grade. A curve started
    // with the wrong grade or the wrong start elevation shows here as a step.
    VerticalAlignment profile;
    profile.pvis = {ProfilePVI{0.0, 50.0},           ProfilePVI{300.0, 59.0, 120.0},
                    ProfilePVI{700.0, 47.0, 200.0},  ProfilePVI{1000.0, 56.0, 100.0},
                    ProfilePVI{1200.0, 52.0}};
    const auto solved = solveProfile(profile);
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const std::vector<double> joints = solved->keyStations();
    ASSERT_GE(joints.size(), 8u);
    std::size_t checked = 0;
    for (std::size_t i = 1; i + 1 < joints.size(); ++i) {
        const double s = joints[i];
        const double h = 1e-6;
        const double before = *solved->elevationAt(s - h);
        const double after = *solved->elevationAt(s + h);
        const double gradeBefore = *solved->gradeAt(s - h);
        const double gradeAfter = *solved->gradeAt(s + h);
        // 2h apart along a line of grade g rises 2hg; the joint must agree to
        // rounding beyond that.
        EXPECT_NEAR(after - before, 2.0 * h * gradeBefore, 1e-9) << "step at " << s;
        EXPECT_NEAR(gradeAfter, gradeBefore, 1e-7) << "kink at " << s;
        ++checked;
    }
    EXPECT_GE(checked, 6u);
    // The grades are +3%, -3%, +3%, -2%, so the three curves are a crest, a
    // sag and a crest: two high points and one low.
    std::size_t highs = 0;
    std::size_t lows = 0;
    for (const ProfileExtremum& point : solved->highLowPoints()) {
        (point.high ? highs : lows)++;
    }
    EXPECT_EQ(highs, 2u);
    EXPECT_EQ(lows, 1u);
}

TEST(Profile, AProfileWithNoCurvesIsTheGradeLineThroughItsPVIs)
{
    VerticalAlignment profile;
    profile.pvis = {ProfilePVI{0.0, 10.0}, ProfilePVI{100.0, 12.0}, ProfilePVI{300.0, 8.0}};
    const auto solved = solveProfile(profile);
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    EXPECT_EQ(solved->elements().size(), 2u);
    EXPECT_NEAR(*solved->elevationAt(50.0), 11.0, 1e-12);
    EXPECT_NEAR(*solved->elevationAt(200.0), 10.0, 1e-12);
    EXPECT_NEAR(*solved->gradeAt(50.0), 0.02, 1e-15);
    EXPECT_NEAR(*solved->gradeAt(200.0), -0.02, 1e-15);
    EXPECT_TRUE(solved->highLowPoints().empty()) << "a kink is not a curve, so no extremum";
    EXPECT_FALSE(solved->elevationAt(-1.0).has_value());
    EXPECT_FALSE(solved->elevationAt(301.0).has_value());
    EXPECT_TRUE(solved->elevationAt(300.0).has_value()) << "the end is inside";
}

TEST(Profile, BackToBackCurvesAreAllowedWhenTheTangentIsExactlyUsedUp)
{
    // Two 100 m curves at PVIs 100 m apart: half-lengths 50 + 50 use the
    // whole tangent. Legal, and no zero-length element between the curves.
    VerticalAlignment profile;
    profile.pvis = {ProfilePVI{0.0, 0.0}, ProfilePVI{100.0, 5.0, 100.0},
                    ProfilePVI{200.0, 0.0, 100.0}, ProfilePVI{300.0, 5.0}};
    const auto solved = solveProfile(profile);
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const std::vector<ProfileElementKind> expected{
        ProfileElementKind::Tangent, ProfileElementKind::Curve, ProfileElementKind::Curve,
        ProfileElementKind::Tangent};
    EXPECT_EQ(kinds(*solved), expected);
    for (const ProfileElement& element : solved->elements()) {
        EXPECT_GT(element.length, 1.0);
    }
    EXPECT_EQ(solved->highLowPoints().size(), 2u); // a crest then a sag
}

TEST(Profile, RefusesDefinitionsThatCannotBeBuiltAndNamesThePVI)
{
    VerticalAlignment one;
    one.pvis = {ProfilePVI{0.0, 0.0}};
    EXPECT_EQ(solveProfile(one).error().code, ErrorCode::InvalidGeometry);

    VerticalAlignment backwards;
    backwards.pvis = {ProfilePVI{100.0, 0.0}, ProfilePVI{50.0, 0.0}};
    const auto notIncreasing = solveProfile(backwards);
    EXPECT_EQ(notIncreasing.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(notIncreasing.error().describe().find("PVI 0"), std::string::npos);

    VerticalAlignment curveAtEnd;
    curveAtEnd.pvis = {ProfilePVI{0.0, 0.0}, ProfilePVI{100.0, 5.0}, ProfilePVI{200.0, 0.0, 50.0}};
    const auto atEnd = solveProfile(curveAtEnd);
    EXPECT_EQ(atEnd.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(atEnd.error().describe().find("PVI 2"), std::string::npos);

    // 150 m curves at PVIs 100 m apart: half-lengths 75 + 75 > 100.
    VerticalAlignment overlap;
    overlap.pvis = {ProfilePVI{0.0, 0.0}, ProfilePVI{100.0, 5.0, 150.0},
                    ProfilePVI{200.0, 0.0, 150.0}, ProfilePVI{300.0, 5.0}};
    const auto overlapping = solveProfile(overlap);
    EXPECT_EQ(overlapping.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(overlapping.error().describe().find("PVI 1"), std::string::npos);
    EXPECT_NE(overlapping.error().describe().find("PVI 2"), std::string::npos);
    EXPECT_NE(overlapping.error().describe().find("short by"), std::string::npos);

    VerticalAlignment negative;
    negative.pvis = {ProfilePVI{0.0, 0.0}, ProfilePVI{100.0, 5.0, -10.0}, ProfilePVI{200.0, 0.0}};
    EXPECT_EQ(solveProfile(negative).error().code, ErrorCode::InvalidArgument);

    VerticalAlignment nan;
    nan.pvis = {ProfilePVI{0.0, std::numeric_limits<double>::quiet_NaN()}, ProfilePVI{100.0, 5.0}};
    EXPECT_EQ(solveProfile(nan).error().code, ErrorCode::InvalidArgument);
}
