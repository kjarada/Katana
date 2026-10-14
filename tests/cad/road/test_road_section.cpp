// One section of a road template (include/katana/cad/road/road_section.hpp),
// against offsets, levels and areas worked by hand from the template's
// widths, crossfalls and depths. The ground is a plane, which the TIN
// reproduces exactly, so the only tolerance is the daylight's bisection.

#include <gtest/gtest.h>

#include <algorithm>
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
