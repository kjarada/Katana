// One section of a road template (include/katana/cad/road/road_section.hpp),
// against offsets, levels and areas worked by hand from the template's
// widths, crossfalls and depths. The ground is a plane, which the TIN
// reproduces exactly, so the only tolerance is the daylight's bisection.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <functional>
#include <limits>
#include <string>
#include <vector>

#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"
#include "road_fixtures.hpp"

using namespace katana::cad::road;
using namespace katana::cad::road::testing;
using katana::core::ErrorCode;

namespace {

// Lane 3.5 m at -3 % (AC14 50 mm over a 200 mm base), shoulder 1.5 m at -4 %
// (a 50 mm seal), a barrier kerb as it comes, a 1.5 m footpath rising 2.5 %;
// both sides alike, no batter.
RoadTemplate street()
{
    RoadTemplate made;
    made.name = "street";
    for (const Side side : {Side::Left, Side::Right}) {
        Component lane = defaultComponent(ComponentKind::Lane, side);
        lane.width = 3.5;
        lane.crossfall = -3.0;
        lane.courses = {makeCourse("AC14", 0.05, "asphalt"), makeCourse("base", 0.2, "rock")};
        Component shoulder = defaultComponent(ComponentKind::Shoulder, side);
        shoulder.width = 1.5;
        shoulder.crossfall = -4.0;
        shoulder.courses = {makeCourse("seal", 0.05, "asphalt")};
        Component kerb = defaultComponent(ComponentKind::Kerb, side);
        Component path = defaultComponent(ComponentKind::Footpath, side);
        path.width = 1.5;
        path.crossfall = 2.5;
        path.courses.clear();
        made.components.insert(made.components.end(), {lane, shoulder, kerb, path});
    }
    return made;
}

// (0,0) to (100,0) to (100,100), the corner rounded at 50 m - with a spiral of
// `spiral` metres into and out of the arc when it is given: a quarter turn to
// the left, tangents of 100 m less the curve's own.
katana::geometry::SolvedAlignment quarterTurn(double spiral = 0.0)
{
    katana::geometry::HorizontalAlignment definition;
    definition.pis = {katana::geometry::AlignmentPI{katana::geometry::Point2(0, 0)},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(100, 0), 50.0, spiral,
                                                    spiral},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(100, 100)}};
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok()) << solved.error().describe();
    return *solved;
}

// How far the alignment turns between two stations, radians: the difference of
// its directions there, taken the short way round. Worked from the directions,
// not from the curvature sectionStations reads, so it is an independent measure
// of what a step does.
double turnBetween(const katana::geometry::SolvedAlignment& alignment, double from, double to)
{
    const double difference =
        *alignment.directionAtStation(to) - *alignment.directionAtStation(from);
    return std::abs(std::atan2(std::sin(difference), std::cos(difference)));
}

const SectionPoint* pointCoded(const std::vector<SectionPoint>& line, const std::string& code)
{
    const auto found = std::ranges::find(line, code, &SectionPoint::code);
    return found == line.end() ? nullptr : &*found;
}

void expectPoint(const std::vector<SectionPoint>& line, const std::string& code, double offset,
                 double level)
{
    const SectionPoint* point = pointCoded(line, code);
    ASSERT_NE(point, nullptr) << "no point " << code;
    EXPECT_NEAR(point->offset, offset, 1e-12) << code;
    EXPECT_NEAR(point->level, level, 1e-12) << code;
}

} // namespace

TEST(RoadSection, AStreetIsSetOutAtItsHandWorkedOffsetsAndLevels)
{
    // At design level 10: the lane falls 3.5 x 3 % = 0.105 to 9.895; the
    // shoulder 1.5 x 4 % = 0.06 more to 9.835 at 5.0; the kerb's face rises
    // 0.15 to 9.985 over 0.025 (battered 1 in 6), its top runs 0.15 to
    // 5.175; the footpath rises 1.5 x 2.5 % = 0.0375 to 10.0225 at 6.675.
    const auto section = roadSection(street(), straight(), level(10.0), nullptr, 100.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    const auto& top = section->top;
    ASSERT_EQ(top.size(), 11u); // the centreline and five points a side
    expectPoint(top, "CL", 0.0, 10.0);
    expectPoint(top, "left/lane/outer", 3.5, 9.895);
    expectPoint(top, "left/shoulder/outer", 5.0, 9.835);
    expectPoint(top, "left/kerb/face", 5.025, 9.985);
    expectPoint(top, "left/kerb/back", 5.175, 9.985);
    expectPoint(top, "left/footpath/outer", 6.675, 10.0225);
    expectPoint(top, "right/lane/outer", -3.5, 9.895);
    expectPoint(top, "right/footpath/outer", -6.675, 10.0225);
    // Left to right.
    EXPECT_TRUE(std::ranges::is_sorted(top, std::greater<>(), &SectionPoint::offset));
    EXPECT_EQ(top.front().code, "left/footpath/outer");
    EXPECT_EQ(top.back().code, "right/footpath/outer");
    EXPECT_NEAR(section->designLevel, 10.0, 1e-12);
    EXPECT_NEAR(section->centre.x, 100.0, 1e-12);
    // No batter: there is nothing to reach, and no ground to measure against.
    EXPECT_TRUE(section->leftReached);
    EXPECT_TRUE(section->rightReached);
    EXPECT_FALSE(section->complete);
    EXPECT_TRUE(section->ground.empty());
    EXPECT_EQ(section->cutArea, 0.0);
}

TEST(RoadSection, TheDividedRoadsLanesSitUnderTheMedianAndItsKerbFacesTheTraffic)
{
    // The divided road on a level road at 0: the median 2 m wide at 0 % (its
    // top is the profile), then the mountable kerb facing outward: 0.15 m of
    // top at level 0, the face falling 0.1 m over 1.5 x 0.1 = 0.15 m to the lip
    // at 2.3, -0.1. The carriageway starts at the lip: lane 1 3.5 m at -3 %
    // falls 0.105 to 5.8 at -0.205, lane 2 to 9.3 at -0.31, the 2.5 m shoulder
    // 0.075 more to 11.8 at -0.385. (Facing the other way the kerb rose to 0.1
    // and the lanes started 100 mm ABOVE the profile, with a drop to the median.)
    RoadTemplate divided;
    for (const RoadTemplate& each : standardTemplates()) {
        if (each.name == "divided road") {
            divided = each;
        }
    }
    ASSERT_EQ(divided.name, "divided road");
    const auto section = templateSection(divided);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    ASSERT_EQ(section->top.size(), 13u); // the centreline and six points a side
    for (const double sign : {1.0, -1.0}) {
        const std::string side = sign > 0.0 ? "left" : "right";
        expectPoint(section->top, side + "/median/outer", sign * 2.0, 0.0);
        expectPoint(section->top, side + "/kerb/face", sign * 2.15, 0.0);
        expectPoint(section->top, side + "/kerb/lip", sign * 2.3, -0.1);
        expectPoint(section->top, side + "/lane1/outer", sign * 5.8, -0.205);
        expectPoint(section->top, side + "/lane2/outer", sign * 9.3, -0.31);
        expectPoint(section->top, side + "/shoulder/outer", sign * 11.8, -0.385);
    }
    EXPECT_EQ(pointCoded(section->top, "left/kerb/back"), nullptr) << "the back is the median's edge";
    // The kerb's base is 0.15 under its lip at -0.1: -0.25 throughout, and it
    // is one solid of three columns that ends where the lane starts.
    const auto kerb = std::ranges::find(section->solids, std::string("left/kerb"),
                                        &SectionSolid::key);
    ASSERT_NE(kerb, section->solids.end());
    ASSERT_EQ(kerb->columns.size(), 3u);
    EXPECT_NEAR(kerb->columns.front().offset, 2.0, 1e-12);
    EXPECT_NEAR(kerb->columns.back().offset, 2.3, 1e-12);
    for (const SolidColumn& column : kerb->columns) {
        EXPECT_NEAR(column.bottom, -0.25, 1e-12);
    }
}

TEST(RoadSection, EachCourseAndKerbIsASolidUnderItsComponent)
{
    const auto section = roadSection(street(), straight(), level(10.0), nullptr, 100.0);
    ASSERT_TRUE(section.ok());
    // Lane AC14 and base, shoulder seal, the kerb: four a side.
    ASSERT_EQ(section->solids.size(), 8u);
    const auto solid = [&](const std::string& key) -> const SectionSolid* {
        const auto found = std::ranges::find(section->solids, key, &SectionSolid::key);
        return found == section->solids.end() ? nullptr : &*found;
    };
    const SectionSolid* wearing = solid("left/lane/AC14");
    ASSERT_NE(wearing, nullptr);
    ASSERT_EQ(wearing->columns.size(), 2u);
    EXPECT_NEAR(wearing->columns[0].top, 10.0, 1e-12);
    EXPECT_NEAR(wearing->columns[0].bottom, 9.95, 1e-12);
    EXPECT_NEAR(wearing->columns[1].offset, 3.5, 1e-12);
    EXPECT_NEAR(wearing->columns[1].top, 9.895, 1e-12);
    EXPECT_NEAR(wearing->columns[1].bottom, 9.845, 1e-12);
    const SectionSolid* base = solid("right/lane/base");
    ASSERT_NE(base, nullptr);
    EXPECT_NEAR(base->columns[1].offset, -3.5, 1e-12);
    EXPECT_NEAR(base->columns[1].top, 9.845, 1e-12);
    EXPECT_NEAR(base->columns[1].bottom, 9.645, 1e-12);
    EXPECT_FALSE(base->course == std::nullopt);
    const SectionSolid* kerb = solid("left/kerb");
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->course, std::nullopt);
    ASSERT_EQ(kerb->columns.size(), 3u);
    for (const SolidColumn& column : kerb->columns) {
        EXPECT_NEAR(column.bottom, 9.835 - 0.15, 1e-12); // 150 mm under the lip
    }
}

TEST(RoadSection, TheFormationStepsAMillimetreWhereTheDepthChangesAndStaysAFunctionOfOffset)
{
    // Under the lane the courses are 250 mm deep (9.75 at the centreline,
    // 9.645 at its edge), under the shoulder 50 mm (9.845 at its inner edge,
    // 9.785 at its outer), under the kerb its base 9.685, under the footpath
    // nothing (its top). At each change the shallower corner moves kFaceRun
    // (1 mm) into its own component.
    const auto section = roadSection(street(), straight(), level(10.0), nullptr, 100.0);
    ASSERT_TRUE(section.ok());
    std::vector<std::pair<double, double>> left;
    for (const SectionPoint& point : section->formation) {
        if (point.offset >= 0.0) {
            left.emplace_back(point.offset, point.level);
        }
    }
    std::ranges::sort(left);
    const std::vector<std::pair<double, double>> expected{
        {0.0, 9.75},    {3.5, 9.645},   {3.501, 9.845}, {4.999, 9.785},  {5.0, 9.685},
        {5.025, 9.685}, {5.175, 9.685}, {5.176, 9.985}, {6.675, 10.0225}};
    ASSERT_EQ(left.size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_NEAR(left[i].first, expected[i].first, 1e-12) << i;
        EXPECT_NEAR(left[i].second, expected[i].second, 1e-12) << i;
    }
    // A function of offset all the way across: strictly falling left to right.
    for (std::size_t i = 0; i + 1 < section->formation.size(); ++i) {
        EXPECT_GT(section->formation[i].offset, section->formation[i + 1].offset) << i;
    }
}

TEST(RoadSection, SuperelevationAtTheMidpointOfItsTransitionIsHalfwayBetweenItsEnds)
{
    // From normal crown at 50 (-3, -3) to full superelevation at 100 (+3,
    // -3): at 75 the left lane is level, the right still falls 3 %.
    Corridor corridor;
    corridor.superelevation = {SuperelevationPoint{50.0, -3.0, -3.0},
                               SuperelevationPoint{100.0, 3.0, -3.0}};
    const auto halfway = superelevationAt(corridor, 75.0);
    ASSERT_TRUE(halfway.has_value());
    EXPECT_NEAR(halfway->left, 0.0, 1e-15);
    EXPECT_NEAR(halfway->right, -3.0, 1e-15);
    EXPECT_EQ(superelevationAt(corridor, 50.0), (Crossfalls{-3.0, -3.0}));
    EXPECT_EQ(superelevationAt(corridor, 100.0), (Crossfalls{3.0, -3.0}));
    EXPECT_FALSE(superelevationAt(corridor, 49.0).has_value());
    EXPECT_FALSE(superelevationAt(corridor, 101.0).has_value());

    const auto section = roadSection(street(), straight(), level(10.0), nullptr, 75.0, halfway);
    ASSERT_TRUE(section.ok());
    expectPoint(section->top, "left/lane/outer", 3.5, 10.0);
    expectPoint(section->top, "right/lane/outer", -3.5, 9.895);
    // The shoulder is superelevated too: level on the left.
    expectPoint(section->top, "left/shoulder/outer", 5.0, 10.0);
    // A component that is not superelevated keeps its own: the footpath.
    expectPoint(section->top, "left/footpath/outer", 6.675, 10.15 + 0.0375);
}

TEST(RoadSection, AnEdgeStripAndADrainBetweenSuperelevatedComponentsFollowTheTable)
{
    // A lane 3.5 m, a flush edge strip 0.15 m, a shoulder 1.5 m, each side; lane
    // and shoulder superelevated by default. The table at the station is (+6,
    // -6) outward percent. Left: the lane rises 3.5 x 6 % = 0.21 to 10.21, the
    // strip - when it is superelevated too - 0.15 x 6 % = 0.009 to 10.219, the
    // shoulder 1.5 x 6 % = 0.09 to 10.309. Right: 9.79, then 0.009 down to 9.781,
    // 0.09 more to 9.691. A strip left at its own 0 % was flat at 10.21, and the
    // shoulder's edge 9 mm low at 10.300.
    const auto stripped = [](bool superelevated) {
        RoadTemplate made;
        made.name = "strip";
        for (const Side side : {Side::Left, Side::Right}) {
            Component lane = defaultComponent(ComponentKind::Lane, side);
            lane.width = 3.5;
            lane.crossfall = -3.0;
            Component strip = defaultComponent(ComponentKind::Kerb, side);
            strip.shape = KerbShape::EdgeStrip;
            strip.crossfall = 0.0;
            strip.superelevated = superelevated;
            Component shoulder = defaultComponent(ComponentKind::Shoulder, side);
            shoulder.width = 1.5;
            shoulder.crossfall = -3.0;
            made.components.insert(made.components.end(), {lane, strip, shoulder});
        }
        return made;
    };
    const Crossfalls table{6.0, -6.0};
    const auto follows = roadSection(stripped(true), straight(), level(10.0), nullptr, 100.0, table);
    ASSERT_TRUE(follows.ok()) << follows.error().describe();
    expectPoint(follows->top, "left/lane/outer", 3.5, 10.21);
    expectPoint(follows->top, "left/kerb/outer", 3.65, 10.219);
    expectPoint(follows->top, "left/shoulder/outer", 5.15, 10.309);
    expectPoint(follows->top, "right/lane/outer", -3.5, 9.79);
    expectPoint(follows->top, "right/kerb/outer", -3.65, 9.781);
    expectPoint(follows->top, "right/shoulder/outer", -5.15, 9.691);
    const auto flat = roadSection(stripped(false), straight(), level(10.0), nullptr, 100.0, table);
    ASSERT_TRUE(flat.ok());
    expectPoint(flat->top, "left/kerb/outer", 3.65, 10.21);
    expectPoint(flat->top, "left/shoulder/outer", 5.15, 10.30);

    // A table drain 2 m wide with its invert 0.5 m down: its outer edge rises
    // with the table when it is superelevated: 2 x 6 % = 0.12 above the lane's
    // edge (10.21 + 0.12 = 10.33), and stays level with it (10.21) when it is not.
    const auto drained = [](bool superelevated) {
        RoadTemplate made;
        made.name = "drained";
        Component lane = defaultComponent(ComponentKind::Lane, Side::Left);
        lane.width = 3.5;
        lane.crossfall = -3.0;
        Component drain = defaultComponent(ComponentKind::Drain, Side::Left);
        drain.superelevated = superelevated;
        made.components = {lane, drain};
        return made;
    };
    const auto raised = roadSection(drained(true), straight(), level(10.0), nullptr, 100.0, table);
    ASSERT_TRUE(raised.ok());
    expectPoint(raised->top, "left/drain/invert", 4.5, 9.71);
    expectPoint(raised->top, "left/drain/outer", 5.5, 10.33);
    const auto level0 = roadSection(drained(false), straight(), level(10.0), nullptr, 100.0, table);
    ASSERT_TRUE(level0.ok());
    expectPoint(level0->top, "left/drain/outer", 5.5, 10.21);
    // With no table at the station nothing is superelevated: the own crossfall.
    const auto none = roadSection(stripped(true), straight(), level(10.0), nullptr, 100.0);
    ASSERT_TRUE(none.ok());
    expectPoint(none->top, "left/kerb/outer", 3.65, 9.895);
}

TEST(RoadSection, ACutBatterDaylightsOnAPlaneAtTheHandWorkedOffset)
{
    // A level design 1 m under flat ground, 8 m wide, 2:1 batters: daylight
    // 2 m out at +-6, the textbook trapezoid (12 + 8) / 2 = 10 m2 of cut.
    const auto ground = planeGround(0.0, 0.0, 0.0);
    const auto section =
        roadSection(laneTemplate(4.0, 0.0, {}), straight(), level(-1.0), &ground, 100.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_TRUE(section->complete);
    expectPoint(section->top, "left/batter/daylight", 6.0, 0.0);
    expectPoint(section->top, "right/batter/daylight", -6.0, 0.0);
    EXPECT_NEAR(section->cutArea, 10.0, 1e-9);
    EXPECT_NEAR(section->fillArea, 0.0, 1e-9);
    EXPECT_NEAR(section->leftCut, 5.0, 1e-9);
    EXPECT_NEAR(section->rightCut, 5.0, 1e-9);
}

TEST(RoadSection, AFillBatterMirrorsTheCut)
{
    const auto ground = planeGround(0.0, 0.0, 0.0);
    const auto section =
        roadSection(laneTemplate(4.0, 0.0, {}), straight(), level(1.0), &ground, 100.0);
    ASSERT_TRUE(section.ok());
    expectPoint(section->top, "left/batter/daylight", 6.0, 0.0);
    EXPECT_NEAR(section->fillArea, 10.0, 1e-9);
    EXPECT_NEAR(section->cutArea, 0.0, 1e-9);
}

TEST(RoadSection, OnSidelongGroundEachSideDaylightsAtItsOwnWidthAndKeepsItsOwnArea)
{
    // Ground rising 10 % to the left (z = 0.1 y), the design level at -1,
    // 4 m each side, 2:1 cut: the left batter meets the ground 3.5 m out
    // (at 7.5, z 0.75), the right 1 m out (at -5, z -0.5). Integrated by
    // hand: the left side is 4.8 under the lane plus 2.45 under the batter,
    // 7.25 m2; the right 3.2 plus 0.3, 3.5 m2; 10.75 in all.
    const auto ground = planeGround(0.0, 0.1, 0.0);
    const auto section =
        roadSection(laneTemplate(4.0, 0.0, {}), straight(), level(-1.0), &ground, 100.0);
    ASSERT_TRUE(section.ok());
    expectPoint(section->top, "left/batter/daylight", 7.5, 0.75);
    expectPoint(section->top, "right/batter/daylight", -5.0, -0.5);
    EXPECT_NEAR(section->leftCut, 7.25, 1e-9);
    EXPECT_NEAR(section->rightCut, 3.5, 1e-9);
    EXPECT_NEAR(section->cutArea, 10.75, 1e-9);
}

TEST(RoadSection, ThePavementBoxIsCutAndTheBatterStartsAtTheFinishedEdge)
{
    // The same cut with 300 mm of courses: the batter still starts at the
    // finished edge (4, -1) and daylights at 6, and the formation is 0.3
    // lower under the lane. Each side, by hand: 1.3 deep over 3.999 m
    // (5.1987), the 1 mm step from -1.3 up to -1 (0.001 x 1.15 = 0.00115),
    // the batter's triangle (1.0): 6.19985 m2; 12.3997 both.
    const auto ground = planeGround(0.0, 0.0, 0.0);
    const auto section = roadSection(
        laneTemplate(4.0, 0.0, {makeCourse("AC", 0.1, "asphalt"), makeCourse("base", 0.2, "rock")}),
        straight(), level(-1.0), &ground, 100.0);
    ASSERT_TRUE(section.ok());
    expectPoint(section->top, "left/batter/daylight", 6.0, 0.0);
    EXPECT_NEAR(section->leftCut, 6.19985, 1e-9);
    EXPECT_NEAR(section->cutArea, 12.3997, 1e-9);
}

TEST(RoadSection, ASideOfABatterAloneStartsItsFormationAtTheHingeAndMeasuresTheTriangleUnderIt)
{
    // Fill: ground at 9, design 10, a 1 in 4 fill batter. From the hinge on
    // the centreline (0, 10) it falls 1 over 4 m to the ground: daylight at
    // +-4, level 9. Each side is the triangle 0.5 x 4 x 1 = 2 m2 between the
    // batter and the ground; 4 m2 in all, and nothing in cut.
    const auto low = planeGround(0.0, 0.0, 9.0);
    const auto fill = roadSection(battersAlone(2.0, 4.0), straight(), level(10.0), &low, 100.0);
    ASSERT_TRUE(fill.ok()) << fill.error().describe();
    EXPECT_TRUE(fill->complete);
    expectPoint(fill->top, "left/batter/daylight", 4.0, 9.0);
    expectPoint(fill->top, "right/batter/daylight", -4.0, 9.0);
    EXPECT_NEAR(fill->fillArea, 4.0, 1e-9);
    EXPECT_NEAR(fill->leftFill, 2.0, 1e-9);
    EXPECT_NEAR(fill->rightFill, 2.0, 1e-9);
    EXPECT_NEAR(fill->cutArea, 0.0, 1e-9);
    // The formation is the batters' own line, through the hinge: the two
    // hinges are one point at the centreline.
    ASSERT_EQ(fill->formation.size(), 3u);
    EXPECT_NEAR(fill->formation[0].offset, 4.0, 1e-9);
    EXPECT_NEAR(fill->formation[1].offset, 0.0, 1e-12);
    EXPECT_NEAR(fill->formation[1].level, 10.0, 1e-12);
    EXPECT_NEAR(fill->formation[2].offset, -4.0, 1e-9);

    // Cut: ground at 11, a 1 in 2 cut batter climbs 1 over 2 m: daylight at
    // +-2, level 11, a triangle 0.5 x 2 x 1 = 1 m2 a side.
    const auto high = planeGround(0.0, 0.0, 11.0);
    const auto cut = roadSection(battersAlone(2.0, 4.0), straight(), level(10.0), &high, 100.0);
    ASSERT_TRUE(cut.ok());
    expectPoint(cut->top, "left/batter/daylight", 2.0, 11.0);
    EXPECT_NEAR(cut->cutArea, 2.0, 1e-9);
    EXPECT_NEAR(cut->leftCut, 1.0, 1e-9);
    EXPECT_NEAR(cut->rightCut, 1.0, 1e-9);
    EXPECT_NEAR(cut->fillArea, 0.0, 1e-9);

    // At grade the batter has no area to measure and the section is complete.
    const auto level10 = planeGround(0.0, 0.0, 10.0);
    const auto flat = roadSection(battersAlone(2.0, 4.0), straight(), level(10.0), &level10, 100.0);
    ASSERT_TRUE(flat.ok());
    EXPECT_TRUE(flat->complete);
    EXPECT_NEAR(flat->cutArea, 0.0, 1e-9);
    EXPECT_NEAR(flat->fillArea, 0.0, 1e-9);

    // No ground at all: the formation is the hinge alone, and nothing breaks.
    const auto bare = roadSection(battersAlone(2.0, 4.0), straight(), level(10.0), nullptr, 100.0);
    ASSERT_TRUE(bare.ok());
    EXPECT_FALSE(bare->complete);
    ASSERT_EQ(bare->formation.size(), 1u);
    EXPECT_NEAR(bare->formation[0].level, 10.0, 1e-12);
}

TEST(RoadSection, ABatterAloneBesideACoursedLaneKeepsItsOwnHingeAndItsOwnArea)
{
    // Left: a 4 m lane at 0 % with 0.1 + 0.2 m of courses, then a 1 in 4 fill
    // batter; right: the batter alone. Ground at 9, design 10.
    // Left formation: 9.7 under the lane to 3.999 (the corner stepped 1 mm
    // back), up to the hinge (4, 10), the batter down to the ground at 8.
    //   3.999 x 0.7 = 2.7993, the 1 mm step 0.001 x 0.85 = 0.00085, the
    //   triangle 0.5 x 4 x 1 = 2: 4.80015 m2.
    // Right: the hinge (0, 10) is higher than the lane's 9.7 at the centreline,
    // so it is the shallower corner and steps 1 mm out to -0.001: the step
    // 0.001 x 0.85 = 0.00085, the batter 0.5 x 3.999 x 1 = 1.9995: 2.00035 m2.
    // Taking the right batter from the lane's f0 (0, 9.7) to its daylight
    // (-4, 9) instead - the line without the hinge - gives 0.5 x 4 x 0.7 =
    // 1.4 m2, 0.6 m2 short.
    RoadTemplate made;
    made.name = "ramp";
    Component lane = defaultComponent(ComponentKind::Lane, Side::Left);
    lane.width = 4.0;
    lane.crossfall = 0.0;
    lane.courses = {makeCourse("AC", 0.1, "asphalt"), makeCourse("base", 0.2, "rock")};
    Component leftBatter = defaultComponent(ComponentKind::Batter, Side::Left);
    leftBatter.fillSlope = 4.0;
    Component rightBatter = defaultComponent(ComponentKind::Batter, Side::Right);
    rightBatter.fillSlope = 4.0;
    made.components = {lane, leftBatter, rightBatter};
    const auto ground = planeGround(0.0, 0.0, 9.0);
    const auto section = roadSection(made, straight(), level(10.0), &ground, 100.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_TRUE(section->complete);
    expectPoint(section->top, "left/batter/daylight", 8.0, 9.0);
    expectPoint(section->top, "right/batter/daylight", -4.0, 9.0);
    EXPECT_NEAR(section->leftFill, 4.80015, 1e-9);
    EXPECT_NEAR(section->rightFill, 2.00035, 1e-9);
    EXPECT_NEAR(section->fillArea, 6.8005, 1e-9);
    EXPECT_NEAR(section->cutArea, 0.0, 1e-9);
    // The formation keeps its function of offset: strictly falling across.
    for (std::size_t i = 0; i + 1 < section->formation.size(); ++i) {
        EXPECT_GT(section->formation[i].offset, section->formation[i + 1].offset) << i;
    }
}

TEST(RoadSection, ABenchedBatterHasItsBenchesAndDaylightsWhereTheyLeadIt)
{
    // 3 m of cut, a 2:1 batter benched 2 m wide every 1 m of rise: up 1 over
    // 2 m to -2 at 6, a bench to 8, up to -1 at 10, a bench to 12, up to the
    // ground at 14.
    RoadTemplate benched = laneTemplate(4.0, 0.0, {});
    for (Component& component : benched.components) {
        if (component.kind == ComponentKind::Batter) {
            component.benchHeight = 1.0;
            component.benchWidth = 2.0;
        }
    }
    const auto ground = planeGround(0.0, 0.0, 0.0);
    const auto section = roadSection(benched, straight(), level(-3.0), &ground, 100.0);
    ASSERT_TRUE(section.ok());
    expectPoint(section->top, "left/batter/bench1-in", 6.0, -2.0);
    expectPoint(section->top, "left/batter/bench1-out", 8.0, -2.0);
    expectPoint(section->top, "left/batter/bench2-in", 10.0, -1.0);
    expectPoint(section->top, "left/batter/bench2-out", 12.0, -1.0);
    const SectionPoint* foot = pointCoded(section->top, "left/batter/daylight");
    ASSERT_NE(foot, nullptr);
    EXPECT_NEAR(foot->offset, 14.0, 1e-9);
    EXPECT_NEAR(foot->level, 0.0, 1e-9);
}

TEST(RoadSection, ATemplateWithAMillimetreBenchIsRefusedBeforeASectionIsBuilt)
{
    // A 1 mm bench every 1 mm of rise over a 50 m reach would be about 17 000
    // benches a section (the cost grew with the square of that); 1e-300 never
    // ended. Both are refused by the check every section starts with.
    for (const double tiny : {0.001, 1e-300}) {
        RoadTemplate benched = laneTemplate(4.0, 0.0, {});
        for (Component& component : benched.components) {
            if (component.kind == ComponentKind::Batter) {
                component.benchHeight = tiny;
                component.benchWidth = tiny;
            }
        }
        const auto ground = planeGround(0.0, 0.0, 0.0);
        const auto section = roadSection(benched, straight(), level(-3.0), &ground, 100.0);
        ASSERT_FALSE(section.ok()) << tiny;
        EXPECT_EQ(section.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(section.error().message.find("benches"), std::string::npos)
            << section.error().message;
    }
}

TEST(RoadSection, ABatterThatMeetsNoGroundLeavesItsSideUnreached)
{
    // Ground only 3 m either side: the batters need 6 m.
    const auto narrow = planeGround(0.0, 0.0, 0.0, 3.0);
    const auto offTheEdge =
        roadSection(laneTemplate(4.0, 0.0, {}), straight(), level(-1.0), &narrow, 100.0);
    ASSERT_TRUE(offTheEdge.ok());
    EXPECT_FALSE(offTheEdge->leftReached);
    EXPECT_FALSE(offTheEdge->complete);
    EXPECT_EQ(offTheEdge->cutArea, 0.0);
    // Its pavement is still there: the hinge is the outermost point.
    EXPECT_EQ(offTheEdge->top.front().code, "left/lane/outer");

    // No surface at all: the same, every section.
    const auto none =
        roadSection(laneTemplate(4.0, 0.0, {}), straight(), level(-1.0), nullptr, 100.0);
    ASSERT_TRUE(none.ok());
    EXPECT_FALSE(none->rightReached);
    EXPECT_TRUE(none->ground.empty());
}

TEST(RoadSection, ABatterLimitThatIsNotAMultipleOfAStepStillFindsADaylightInsideIt)
{
    // A 3.5 m lane at -3 % from design 10 ends at the hinge (3.5, 9.895). On
    // ground at 9 the 1 in 4 fill batter meets it 0.895 x 4 = 3.58 m out, at
    // offset 7.08: inside a limit of 3.7 m, but past the last half-metre step
    // (3.5) the march used to try before it gave up.
    const auto withLimit = [](double limit) {
        RoadTemplate made = laneTemplate(3.5, -3.0, {}, 2.0, 4.0);
        for (Component& component : made.components) {
            if (component.kind == ComponentKind::Batter) {
                component.maximumWidth = limit;
            }
        }
        return made;
    };
    const auto ground = planeGround(0.0, 0.0, 9.0);
    const auto inside = roadSection(withLimit(3.7), straight(), level(10.0), &ground, 100.0);
    ASSERT_TRUE(inside.ok());
    EXPECT_TRUE(inside->complete);
    expectPoint(inside->top, "left/batter/daylight", 7.08, 9.0);
    expectPoint(inside->top, "right/batter/daylight", -7.08, 9.0);
    // Fill a side, between the ground (9) and the finished line: under the
    // lane a trapezoid of heights 1 and 0.895 over 3.5 m, 3.5 x (1 + 0.895) /
    // 2 = 3.31625; under the batter a triangle 0.5 x 3.58 x 0.895 = 1.60205:
    // 4.9183 m2.
    EXPECT_NEAR(inside->leftFill, 3.31625 + 1.60205, 1e-9);
    // A limit short of the crossing (3.5 m < 3.58 m) leaves the side
    // unreached, as it always did.
    const auto short_ = roadSection(withLimit(3.5), straight(), level(10.0), &ground, 100.0);
    ASSERT_TRUE(short_.ok());
    EXPECT_FALSE(short_->leftReached);
    EXPECT_FALSE(short_->complete);
    EXPECT_EQ(short_->cutArea, 0.0);

    // A limit under one step: ground at 9.85, the batter meets it 0.045 x 4 =
    // 0.18 m out (offset 3.68) - inside 0.3 m, outside 0.1 m.
    const auto high = planeGround(0.0, 0.0, 9.85);
    const auto brief = roadSection(withLimit(0.3), straight(), level(10.0), &high, 100.0);
    ASSERT_TRUE(brief.ok());
    EXPECT_TRUE(brief->complete);
    expectPoint(brief->top, "left/batter/daylight", 3.68, 9.85);
    const auto tooBrief = roadSection(withLimit(0.1), straight(), level(10.0), &high, 100.0);
    ASSERT_TRUE(tooBrief.ok());
    EXPECT_FALSE(tooBrief->leftReached);
}

TEST(RoadSection, AnEmptyTemplateIsTheCentrelineAlone)
{
    RoadTemplate empty;
    empty.name = "empty";
    const auto section = roadSection(empty, straight(), level(5.0), nullptr, 10.0);
    ASSERT_TRUE(section.ok());
    ASSERT_EQ(section->top.size(), 1u);
    EXPECT_EQ(section->top.front().code, "CL");
    EXPECT_TRUE(section->solids.empty());
}

TEST(RoadSection, RefusesAStationOffTheAlignmentOrProfileAndATemplateThatCannotBeBuilt)
{
    EXPECT_EQ(roadSection(street(), straight(), level(10.0), nullptr, 250.0).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(
        roadSection(street(), straight(), level(10.0, 0.0, 50.0), nullptr, 100.0).error().code,
        ErrorCode::InvalidArgument);
    RoadTemplate broken = street();
    broken.components[0].width = -1.0;
    EXPECT_EQ(roadSection(broken, straight(), level(10.0), nullptr, 100.0).error().code,
              ErrorCode::InvalidArgument);
}

TEST(RoadSection, StationsAreTheIntervalTheEndsAndTheKeysEachOnce)
{
    // 0 to 100 every 30, with keys at 45, at the end, outside, and within
    // a tenth of a micrometre of the start (made one with it).
    const auto stations = sectionStations(0.0, 100.0, 30.0, {45.0, 100.0, -5.0, 1e-8});
    EXPECT_EQ(stations, (std::vector<double>{0.0, 30.0, 45.0, 60.0, 90.0, 100.0}));
    EXPECT_EQ(sectionStations(0.0, 20.0, 7.0, {}), (std::vector<double>{0.0, 7.0, 14.0, 20.0}));
    EXPECT_TRUE(sectionStations(0.0, 20.0, 0.0, {}).empty());
    EXPECT_TRUE(sectionStations(20.0, 0.0, 5.0, {}).empty());
}

TEST(RoadSection, StationsInsideACurveAreCutUntilNoStepTurnsThroughMoreThanTheTolerancesAngle)
{
    // The quarter turn at 50 m: the arc runs from station 50 to 50 + 25 pi =
    // 128.5398 (50 x pi/2). Every 10 m with the keys 0, 50, 128.5398, 178.5398
    // the plain stations are 0, 10, ..., 120, 128.5398, 130, ..., 170,
    // 178.5398: 20. A tolerance of 0.001 allows a step to turn through
    // sqrt(6 x 0.001) = 0.07746 radians, 3.873 m on R = 50 m. A 10 m gap turns
    // through 10/50 = 0.2: 0.2/0.07746 = 2.58, three parts; the last gap on the
    // arc, 8.5398 m, turns 0.1708: 2.2, three parts. The arc holds seven gaps
    // of 10 m and that one: 8 x 2 = 16 stations more, 36.
    const auto alignment = quarterTurn();
    const std::vector<double> keys = alignment.keyStations();
    const std::vector<double> plain = sectionStations(0.0, alignment.endStation(), 10.0, keys);
    ASSERT_EQ(plain.size(), 20u);
    const auto cut = sectionStations(0.0, alignment.endStation(), 10.0, keys, alignment, 0.001);
    ASSERT_TRUE(cut.ok()) << cut.error().describe();
    EXPECT_EQ(cut->size(), 36u);
    // Every plain station is still there, ascending and each once.
    for (const double station : plain) {
        EXPECT_NE(std::ranges::find(*cut, station), cut->end()) << station;
    }
    EXPECT_TRUE(std::ranges::adjacent_find(*cut, std::greater_equal<>{}) == cut->end())
        << "not strictly ascending";
    // The first gap in thirds: 50, 50 + 10/3, 50 + 20/3, 60.
    const auto first = std::ranges::find(*cut, 50.0);
    ASSERT_NE(first, cut->end());
    ASSERT_GE(cut->end() - first, 4);
    EXPECT_NEAR(*(first + 1), 50.0 + 10.0 / 3.0, 1e-12);
    EXPECT_NEAR(*(first + 2), 50.0 + 20.0 / 3.0, 1e-12);
    EXPECT_EQ(*(first + 3), 60.0);
    // The property, from the directions: no step turns through more than the
    // angle - and the plain list does, which shows the cases are reached.
    const double angle = std::sqrt(6.0 * 0.001);
    double plainWorst = 0.0;
    for (std::size_t i = 0; i + 1 < plain.size(); ++i) {
        plainWorst = std::max(plainWorst, turnBetween(alignment, plain[i], plain[i + 1]));
    }
    EXPECT_GT(plainWorst, angle) << "the plain stations already satisfied it: nothing was tested";
    for (std::size_t i = 0; i + 1 < cut->size(); ++i) {
        EXPECT_LE(turnBetween(alignment, (*cut)[i], (*cut)[i + 1]), angle * (1.0 + 1e-9))
            << (*cut)[i] << " to " << (*cut)[i + 1];
    }
}

TEST(RoadSection, StationsOnASpiralAreHeldToTheSharperEndOfEachGap)
{
    // A spiral of 40 m into and out of the arc: its curvature rises from
    // nothing to 1/50 linearly, so a gap on it turns through the mean of its
    // ends' curvatures times its length, at most the sharper end's times it,
    // which is what is held to the angle. Measured from the directions.
    const auto alignment = quarterTurn(40.0);
    const std::vector<double> keys = alignment.keyStations();
    ASSERT_GE(keys.size(), 6u) << "a spiral in and out of an arc: TS SC CS ST";
    const double angle = std::sqrt(6.0 * 0.001);
    double worst = 0.0;
    for (const double interval : {10.0, 3.0}) {
        const std::vector<double> plain =
            sectionStations(0.0, alignment.endStation(), interval, keys);
        for (std::size_t i = 0; i + 1 < plain.size(); ++i) {
            worst = std::max(worst, turnBetween(alignment, plain[i], plain[i + 1]));
        }
        const auto cut =
            sectionStations(0.0, alignment.endStation(), interval, keys, alignment, 0.001);
        ASSERT_TRUE(cut.ok()) << cut.error().describe();
        // 3 m on the arc is 0.06 radians, under the 0.0775 allowed: nothing to
        // add. 10 m is 0.2 on the arc, and the spirals' sharper ends are cut too.
        if (interval == 3.0) {
            EXPECT_EQ(*cut, plain);
        } else {
            EXPECT_GT(cut->size(), plain.size());
        }
        for (const double station : plain) {
            EXPECT_NE(std::ranges::find(*cut, station), cut->end()) << station;
        }
        for (std::size_t i = 0; i + 1 < cut->size(); ++i) {
            EXPECT_LE(turnBetween(alignment, (*cut)[i], (*cut)[i + 1]), angle * (1.0 + 1e-9))
                << "interval " << interval << ": " << (*cut)[i] << " to " << (*cut)[i + 1];
        }
    }
    // The plain list at 10 m has a step that turns too far, so the property
    // above is not satisfied by luck.
    EXPECT_GT(worst, angle);
}

TEST(RoadSection, ATangentAddsNoStationsAndNoToleranceOrAnUnusableOneAddsNone)
{
    // The straight, and a curve with the tolerance off (0), negative, not a
    // number or infinite: the plain list, as sectionStations has always given.
    const auto straightLine = straight(100.0);
    const std::vector<double> plain = sectionStations(0.0, 100.0, 30.0, {45.0});
    EXPECT_EQ(sectionStations(0.0, 100.0, 30.0, {45.0}, straightLine, 0.001).value(), plain);
    const auto curve = quarterTurn();
    const std::vector<double> curvePlain =
        sectionStations(0.0, curve.endStation(), 10.0, curve.keyStations());
    for (const double tolerance :
         {0.0, -0.001, std::numeric_limits<double>::quiet_NaN(),
          std::numeric_limits<double>::infinity()}) {
        const auto same =
            sectionStations(0.0, curve.endStation(), 10.0, curve.keyStations(), curve, tolerance);
        ASSERT_TRUE(same.ok()) << tolerance;
        EXPECT_EQ(*same, curvePlain) << tolerance;
    }
    // No length, and no interval: nothing to cut.
    EXPECT_EQ(sectionStations(5.0, 5.0, 10.0, {}, curve, 0.001).value(),
              (std::vector<double>{5.0}));
    EXPECT_TRUE(sectionStations(5.0, 0.0, 10.0, {}, curve, 0.001).value().empty());
    EXPECT_TRUE(sectionStations(0.0, 50.0, 0.0, {}, curve, 0.001).value().empty());
    // An interval longer than the road: the two ends, and the gap between them
    // cut where the alignment curves.
    const auto wide = sectionStations(0.0, curve.endStation(), 1e9, {}, curve, 0.001);
    ASSERT_TRUE(wide.ok());
    EXPECT_EQ(wide->front(), 0.0);
    EXPECT_EQ(wide->back(), curve.endStation());
    EXPECT_GT(wide->size(), 2u);
}

TEST(RoadSection, ATolerancesStationsBeyondTheLimitAreRefusedBeforeOneIsMade)
{
    // 1e-12 allows a step of sqrt(6e-12) = 2.4e-6 radians, 1.2e-4 m on R = 50:
    // 650 000 sections round the arc alone, over the 100 000 a corridor makes.
    // And 1e-300, which asks for more parts than a size_t holds, is refused the
    // same way and not wrapped round.
    const auto curve = quarterTurn();
    for (const double tolerance : {1e-12, 1e-300}) {
        const auto refused = sectionStations(0.0, curve.endStation(), 10.0, curve.keyStations(),
                                             curve, tolerance);
        ASSERT_FALSE(refused.ok()) << tolerance;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(refused.error().message.find("100000"), std::string::npos)
            << refused.error().message;
    }
    // The straight has nothing to cut at any tolerance.
    EXPECT_TRUE(sectionStations(0.0, 100.0, 10.0, {}, straight(100.0), 1e-300).ok());
}
