// A whole corridor and the road model's objects made from it
// (include/katana/cad/road/road_section.hpp, corridorSolids): volumes against
// width x thickness x length, closed solids, stable ids, the earthworks
// against the end areas the old assembly measured, strings and surfaces.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <map>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"
#include "road_fixtures.hpp"

using namespace katana::cad::road;
using namespace katana::cad::road::testing;
using katana::cad::bim::BimModel;
using katana::cad::bim::BimObject;

namespace {

Corridor corridorOf(std::string templateName, double interval = 10.0)
{
    Corridor corridor;
    corridor.name = "Main";
    corridor.alignment = "road";
    corridor.templateName = std::move(templateName);
    corridor.interval = interval;
    return corridor;
}

// A 3.5 m lane at -3 % each side, AC14 50 mm over a 200 mm base.
RoadTemplate lanes()
{
    RoadTemplate made;
    made.name = "lanes";
    for (const Side side : {Side::Left, Side::Right}) {
        Component lane = defaultComponent(ComponentKind::Lane, side);
        lane.width = 3.5;
        lane.crossfall = -3.0;
        lane.courses = {makeCourse("AC14", 0.05, "asphalt"), makeCourse("base", 0.2, "rock")};
        made.components.push_back(lane);
    }
    return made;
}

RoadTemplateSet setOf(std::vector<RoadTemplate> templates)
{
    RoadTemplateSet set;
    set.templates = std::move(templates);
    return set;
}

const BimObject* objectWithId(const BimModel& model, const std::string& id)
{
    const auto found = std::ranges::find(model.objects, id, &BimObject::id);
    return found == model.objects.end() ? nullptr : &*found;
}

double quantityOf(const BimObject& object, const std::string& name)
{
    for (const auto& quantity : object.quantities) {
        if (quantity.name == name) {
            return quantity.value;
        }
    }
    ADD_FAILURE() << object.id << " has no " << name;
    return 0.0;
}

// A curve gentle enough to stay on the test ground: (0,0) to (100,0) to
// (200,40), rounded at 100 m radius.
katana::geometry::SolvedAlignment curved()
{
    katana::geometry::HorizontalAlignment definition;
    definition.pis = {katana::geometry::AlignmentPI{katana::geometry::Point2(0, 0)},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(100, 0), 100.0},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(200, 40)}};
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok()) << solved.error().describe();
    return *solved;
}

// Ground that depends on x alone, on a grid so that every triangle lies in one
// cell of it: 5 m at x = 0, 20 and 40, 15 m at x = 10 and 30, and 5 m beyond.
// Under a design level of 10 the sections at 0, 20 and 40 are in fill and the
// ones at 10 and 30 in cut.
katana::terrain::TinSurface sawGround()
{
    katana::terrain::TinInput input;
    for (int i = -10; i <= 40; ++i) {
        const double x = 10.0 * i;
        const double z = (i >= 0 && i <= 4 && i % 2 == 1) ? 15.0 : 5.0;
        for (const double y : {-100.0, -50.0, 0.0, 50.0, 100.0}) {
            input.points.emplace_back(x, y, z);
        }
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built->surface;
}

} // namespace

TEST(CorridorObjects, CourseVolumesOnAStraightAreWidthTimesThicknessTimesLength)
{
    // 100 m of a 3.5 m lane: the AC14 is 3.5 x 0.05 x 100 = 17.5 m3 a side,
    // the base 3.5 x 0.2 x 100 = 70 m3; each covers 350 m2 in plan. The
    // crossfall tilts the section into a parallelogram of the same area.
    const auto built = buildCorridor(corridorOf("lanes"), setOf({lanes()}), straight(100.0),
                                     level(10.0, 0.0, 100.0), nullptr);
    EXPECT_TRUE(built.problems.empty()) << built.problems.front();
    ASSERT_EQ(built.runs.size(), 1u);
    EXPECT_EQ(built.runs.front().sections.size(), 11u); // 0, 10, ..., 100
    const BimModel model = corridorSolids(built);
    EXPECT_TRUE(model.problems.empty());
    ASSERT_EQ(model.objects.size(), 4u);
    for (const std::string side : {"left", "right"}) {
        const BimObject* wearing = objectWithId(model, "corridor/Main/" + side + "/lane/AC14/1");
        ASSERT_NE(wearing, nullptr) << side;
        EXPECT_NEAR(quantityOf(*wearing, "GrossVolume"), 17.5, 1e-9);
        EXPECT_NEAR(meshVolume(wearing->mesh), 17.5, 1e-9);
        EXPECT_NEAR(quantityOf(*wearing, "GrossArea"), 350.0, 1e-9);
        EXPECT_NEAR(quantityOf(*wearing, "Length"), 100.0, 1e-12);
        EXPECT_NEAR(quantityOf(*wearing, "Width"), 3.5, 1e-12);
        EXPECT_NEAR(quantityOf(*wearing, "Depth"), 0.05, 1e-15);
        EXPECT_EQ(wearing->ifcClass, "IfcCourse");
        EXPECT_EQ(wearing->predefinedType, "PAVEMENT");
        EXPECT_EQ(wearing->kind, "pavement");
        EXPECT_EQ(wearing->part, "TRAFFICLANE");
        EXPECT_EQ(wearing->facility, "Main");
        EXPECT_EQ(wearing->material, "asphalt");
        EXPECT_EQ(wearing->layer, "road/pavement");
        const BimObject* base = objectWithId(model, "corridor/Main/" + side + "/lane/base/1");
        ASSERT_NE(base, nullptr);
        EXPECT_NEAR(quantityOf(*base, "GrossVolume"), 70.0, 1e-9);
    }
}

TEST(CorridorObjects, EverySolidIsClosedWithEachEdgeSharedByExactlyTwoFacesTurnedOutward)
{
    // The urban street - lanes, parking, kerb and gutter, footpath, verge,
    // batter - round a curve, in fill above a plane: every course, kerb and
    // earthworks solid, both sides.
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    const auto ground = planeGround(0.0, 0.0, 8.0);
    katana::geometry::VerticalAlignment profile;
    profile.pvis = {katana::geometry::ProfilePVI{0.0, 10.0},
                    katana::geometry::ProfilePVI{300.0, 10.0}};
    const auto solvedProfile = katana::geometry::solveProfile(profile);
    ASSERT_TRUE(solvedProfile.ok());
    const auto built =
        buildCorridor(corridorOf("urban street"), templates, curved(), *solvedProfile, &ground);
    const BimModel model = corridorSolids(built);
    ASSERT_GT(model.objects.size(), 10u);
    bool sawFill = false;
    for (const BimObject& object : model.objects) {
        ASSERT_FALSE(object.mesh.empty()) << object.id;
        std::map<std::pair<std::uint32_t, std::uint32_t>, int> undirected;
        std::map<std::pair<std::uint32_t, std::uint32_t>, int> directed;
        for (const auto& face : object.mesh.faces) {
            for (int e = 0; e < 3; ++e) {
                const std::uint32_t a = face[static_cast<std::size_t>(e)];
                const std::uint32_t b = face[static_cast<std::size_t>((e + 1) % 3)];
                ++undirected[{std::min(a, b), std::max(a, b)}];
                ++directed[{a, b}];
            }
        }
        for (const auto& [edge, count] : undirected) {
            ASSERT_EQ(count, 2) << object.id << " edge " << edge.first << "-" << edge.second;
        }
        // Turned consistently: each edge walked once each way.
        for (const auto& [edge, count] : directed) {
            ASSERT_EQ(count, 1) << object.id;
        }
        EXPECT_GT(meshVolume(object.mesh), 0.0) << object.id << " is turned inward";
        sawFill = sawFill || object.ifcClass == "IfcEarthworksFill";
        EXPECT_FALSE(object.plan.empty()) << object.id;
    }
    EXPECT_TRUE(sawFill);
}

TEST(CorridorObjects, ObjectIdsAreStableAndNameTheirDefinition)
{
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    const auto ground = planeGround(0.0, 0.0, 9.0);
    const auto first = corridorSolids(
        buildCorridor(corridorOf("urban street"), templates, straight(), level(10.0), &ground));
    const auto second = corridorSolids(
        buildCorridor(corridorOf("urban street"), templates, straight(), level(10.0), &ground));
    EXPECT_EQ(first, second);
    for (const std::string id :
         {"corridor/Main/left/lane/AC14/1", "corridor/Main/right/parking/base/1",
          "corridor/Main/left/kerb/1", "corridor/Main/left/footpath/concrete/1",
          "corridor/Main/left/verge/topsoil/1", "corridor/Main/left/earthworks/fill/1",
          "corridor/Main/right/earthworks/fill/1"}) {
        EXPECT_NE(objectWithId(first, id), nullptr) << id;
    }
    // A footpath's top course is the path, a slab; what is under it a course.
    EXPECT_EQ(objectWithId(first, "corridor/Main/left/footpath/concrete/1")->ifcClass, "IfcSlab");
    EXPECT_EQ(objectWithId(first, "corridor/Main/left/footpath/concrete/1")->predefinedType,
              "SIDEWALK");
    ASSERT_NE(objectWithId(first, "corridor/Main/left/footpath/bedding/1"), nullptr);
    EXPECT_EQ(objectWithId(first, "corridor/Main/left/footpath/bedding/1")->ifcClass, "IfcCourse");
    EXPECT_EQ(objectWithId(first, "corridor/Main/left/kerb/1")->predefinedType, "");
    EXPECT_EQ(objectWithId(first, "corridor/Main/left/earthworks/fill/1")->predefinedType,
              "EMBANKMENT");
    std::vector<std::string> ids;
    for (const BimObject& object : first.objects) {
        ids.push_back(object.id);
    }
    std::ranges::sort(ids);
    EXPECT_EQ(std::ranges::adjacent_find(ids), ids.end()) << "two objects share an id";
}

TEST(CorridorObjects, AKerbsLengthIsItsLipLineAndItsShapeIsItsWidth)
{
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    const BimModel model = corridorSolids(buildCorridor(
        corridorOf("urban street"), templates, straight(100.0), level(10.0, 0.0, 100.0), nullptr));
    const BimObject* kerb = objectWithId(model, "corridor/Main/left/kerb/1");
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->kind, "kerb");
    EXPECT_EQ(kerb->ifcClass, "IfcKerb");
    EXPECT_NEAR(quantityOf(*kerb, "Length"), 100.0, 1e-9);
    EXPECT_NEAR(quantityOf(*kerb, "Width"), 0.475, 1e-12); // kerb and gutter
    EXPECT_NEAR(quantityOf(*kerb, "Height"), 0.15, 1e-15);
    // Its cross-section, column to column: the channel 0.3 wide and 0.15
    // thick (0.045), the face 0.025 wide from 0.15 thick to 0.3 (0.005625),
    // the top 0.15 wide and 0.3 thick (0.045): 0.095625 m2, 9.5625 m3.
    EXPECT_NEAR(quantityOf(*kerb, "GrossVolume"), 9.5625, 1e-9);
}

TEST(CorridorObjects, TheEarthworksAreTheEndAreasTheOldAssemblyMeasured)
{
    // The textbook cut - 1 m under flat ground, 8 m wide, 2:1 batters, 10
    // m2 a section - over 200 m: 1000 m3 each side, 2000 in all, as
    // corridorQuantities has always given it.
    const auto ground = planeGround(0.0, 0.0, 0.0);
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}), straight(),
                                     level(-1.0), &ground);
    EXPECT_NEAR(built.cut, 2000.0, 1e-6);
    EXPECT_NEAR(built.fill, 0.0, 1e-9);
    EXPECT_EQ(built.incompleteSections, 0u);
    const BimModel model = corridorSolids(built);
    const BimObject* left = objectWithId(model, "corridor/Main/left/earthworks/cut/1");
    const BimObject* right = objectWithId(model, "corridor/Main/right/earthworks/cut/1");
    ASSERT_NE(left, nullptr);
    ASSERT_NE(right, nullptr);
    EXPECT_NEAR(quantityOf(*left, "GrossVolume"), 1000.0, 1e-6);
    EXPECT_NEAR(quantityOf(*right, "GrossVolume"), 1000.0, 1e-6);
    EXPECT_EQ(left->ifcClass, "IfcEarthworksCut");
    EXPECT_EQ(left->predefinedType, "CUT");
    // The surface the cut is taken from, which the IFC export reads.
    EXPECT_NE(std::ranges::find(left->properties,
                                std::pair<std::string, std::string>{"Target Surface", "ground"}),
              left->properties.end());
    EXPECT_EQ(left->layer, "road/earthworks/cut");
    // No fill anywhere: no solid of nothing.
    EXPECT_EQ(objectWithId(model, "corridor/Main/left/earthworks/fill/1"), nullptr);
    // The solid is drawn through 41 columns a side, every 0.15 m of its 6,
    // so it holds the end area's volume less the corner the columns at 3.9
    // and 4.05 cut off the lane's edge at 4: a triangle of 0.5 x (0.1 x
    // 0.025) = 0.00125 m2 a section, 0.25 m3 over 200 m - 999.75.
    EXPECT_NEAR(meshVolume(left->mesh), 999.75, 1e-6);
}

TEST(CorridorObjects, AnEmbankmentOfBattersAloneIsTheTrianglesUnderTheirSlopesAndItsSubgradeRises)
{
    // Ground at 9, design 10, a 1 in 4 fill batter each side and nothing
    // between: a triangle 0.5 x 4 x 1 = 2 m2 a side a section, over 100 m
    // 200 m3 a side and 400 in all. The subgrade is the batters' own line, so
    // it reaches the design level at the centreline and falls 1 over 4 m
    // either side: 9.5 two metres out.
    const auto low = planeGround(0.0, 0.0, 9.0);
    Corridor corridor = corridorOf("batters");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({battersAlone(2.0, 4.0)}), straight(100.0),
                                     level(10.0, 0.0, 100.0), &low);
    EXPECT_TRUE(built.problems.empty()) << built.problems.front();
    EXPECT_NEAR(built.fill, 400.0, 1e-6);
    EXPECT_NEAR(built.cut, 0.0, 1e-9);
    const BimModel model = corridorSolids(built);
    for (const std::string side : {"left", "right"}) {
        const BimObject* fill = objectWithId(model, "corridor/Main/" + side + "/earthworks/fill/1");
        ASSERT_NE(fill, nullptr) << side;
        EXPECT_NEAR(quantityOf(*fill, "GrossVolume"), 200.0, 1e-6);
        EXPECT_NEAR(meshVolume(fill->mesh), 200.0, 1e-6); // a straight batter needs no columns
    }
    const auto subgrade = sectionsSurface(corridorSections(built), *built.alignment, true, false);
    ASSERT_TRUE(subgrade.ok()) << subgrade.error().describe();
    EXPECT_NEAR(*subgrade->surface.elevationAt(50.0, 0.0), 10.0, 1e-9);
    EXPECT_NEAR(*subgrade->surface.elevationAt(50.0, 2.0), 9.5, 1e-9);
    EXPECT_NEAR(*subgrade->surface.elevationAt(50.0, -2.0), 9.5, 1e-9);

    // The same in cut: ground at 11, a 1 in 2 cut batter climbs 1 over 2 m,
    // 0.5 x 2 x 1 = 1 m2 a side, 100 m3 a side; the subgrade is 10 at the
    // centreline and 10.5 one metre out, and not the flat 11 it read when the
    // line ran from daylight to daylight.
    const auto high = planeGround(0.0, 0.0, 11.0);
    const auto dug = buildCorridor(corridor, setOf({battersAlone(2.0, 4.0)}), straight(100.0),
                                   level(10.0, 0.0, 100.0), &high);
    EXPECT_NEAR(dug.cut, 200.0, 1e-6);
    EXPECT_NEAR(dug.fill, 0.0, 1e-9);
    const auto trench = sectionsSurface(corridorSections(dug), *dug.alignment, true, false);
    ASSERT_TRUE(trench.ok());
    EXPECT_NEAR(*trench->surface.elevationAt(50.0, 0.0), 10.0, 1e-9);
    EXPECT_NEAR(*trench->surface.elevationAt(50.0, 1.0), 10.5, 1e-9);
}

TEST(CorridorObjects, ASideWithCutAndFillGivesEachItsOwnFootprintAndTheTwoAddUpToTheSide)
{
    // Ground z = 9.5 + 0.25 y, rising to the left; design 10; a 4 m lane at 0 %
    // with 2:1 batters. Left of the centreline the ground is below the lane
    // out to y = 2 (z 10) and above it beyond: fill thick 0.5 at the
    // centreline down to nothing at 2 m (0.5 x 2 x 0.5 = 0.5 m2); cut thick
    // nothing at 2 m, 0.5 at the hinge (4, 10, the ground 10.5), nothing again
    // where the batter (10 + d/2) meets the ground (10.5 + d/4) at d = 2, out
    // 6: two triangles of 0.5 x 2 x 0.5, 1.0 m2. Right of the centreline the
    // ground falls away: fill 0.5 at the centreline and 1.5 at the hinge
    // (ground 8.5), (0.5 + 1.5) / 2 x 4 = 4 m2 under the lane; the batter
    // 10 - d/2 meets 8.5 - d/4 at d = 6, out 10: 0.5 x 6 x 1.5 = 4.5 m2. Over
    // 100 m: volumes 50, 100 and 850 m3; footprints 2, 4 and 10 m wide.
    const auto ground = planeGround(0.0, 0.25, 9.5);
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}), straight(100.0),
                                     level(10.0, 0.0, 100.0), &ground);
    EXPECT_TRUE(built.problems.empty()) << built.problems.front();
    EXPECT_NEAR(built.cut, 100.0, 1e-6);
    EXPECT_NEAR(built.fill, 900.0, 1e-6);
    const BimModel model = corridorSolids(built);
    ASSERT_EQ(model.objects.size(), 3u) << "left cut, left fill, right fill; no right cut";
    EXPECT_EQ(objectWithId(model, "corridor/Main/right/earthworks/cut/1"), nullptr);

    const BimObject* leftFill = objectWithId(model, "corridor/Main/left/earthworks/fill/1");
    const BimObject* leftCut = objectWithId(model, "corridor/Main/left/earthworks/cut/1");
    const BimObject* rightFill = objectWithId(model, "corridor/Main/right/earthworks/fill/1");
    ASSERT_NE(leftFill, nullptr);
    ASSERT_NE(leftCut, nullptr);
    ASSERT_NE(rightFill, nullptr);
    EXPECT_NEAR(quantityOf(*leftFill, "GrossVolume"), 50.0, 1e-6);
    EXPECT_NEAR(quantityOf(*leftCut, "GrossVolume"), 100.0, 1e-6);
    EXPECT_NEAR(quantityOf(*rightFill, "GrossVolume"), 850.0, 1e-6);
    // The footprints: fill 0 to 2 m out, cut 2 to 6, so together the 6 m
    // from the centreline to the daylight and not 6 m twice; the right
    // fill 10 m. The solids are drawn over the same stretches, so a straight
    // thickness needs no columns to be exact.
    EXPECT_NEAR(quantityOf(*leftFill, "GrossArea"), 2.0 * 100.0, 1e-6);
    EXPECT_NEAR(quantityOf(*leftCut, "GrossArea"), 4.0 * 100.0, 1e-6);
    EXPECT_NEAR(quantityOf(*rightFill, "GrossArea"), 10.0 * 100.0, 1e-6);
    EXPECT_NEAR(quantityOf(*leftFill, "GrossArea") + quantityOf(*leftCut, "GrossArea"), 600.0, 1e-6);
    EXPECT_NEAR(meshVolume(leftFill->mesh), 50.0, 1e-6);
    EXPECT_NEAR(meshVolume(leftCut->mesh), 100.0, 1e-6);
    EXPECT_NEAR(meshVolume(rightFill->mesh), 850.0, 1e-6);
    // The plan outline is the footprint too: the cut's is a strip 4 m wide
    // from 2 m out.
    const auto box = leftCut->plan.front().boundingBox();
    EXPECT_NEAR(box.min.y, 2.0, 1e-9);
    EXPECT_NEAR(box.max.y, 6.0, 1e-9);
}

TEST(CorridorObjects, AFillThatEndsAlongTheRoadIsASolidOfItsOwnStationsAndTheCutBeginsWhereItEnds)
{
    // Ground z = 5 + 0.1 x under a design level of 10: it meets the design at
    // x = 50, in fill before it and in cut after. A 4 m lane, 2:1 batters.
    // At 10 m sections the fill depth D = 5, 4, 3, 2, 1, 0 at x = 0 ... 50
    // gives, a side, 4 D + D2 = 45, 32, 21, 12, 5, 0 m2 (the lane, and the
    // batter's triangle 0.5 x 2D x D) and a footprint 4 + 2 D = 14, 12, 10, 8,
    // 6 m wide. End areas: (45 + 32) 5 + (32 + 21) 5 + (21 + 12) 5 + (12 + 5) 5
    // + (5 + 0) 5 = 925 m3. Plan area: the trapezoids (14 + 12) 5 + (12 + 10)
    // 5 + (10 + 8) 5 + (8 + 6) 5 = 130 + 110 + 90 + 70 = 400, and the last
    // interval narrowing from 6 m to a point, 0.5 x 6 x 10 = 30: 430 m2.
    // The cut mirrors it from x = 50 to 100.
    const auto ground = planeGround(0.1, 0.0, 5.0);
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}), straight(100.0),
                                     level(10.0, 0.0, 100.0), &ground);
    EXPECT_NEAR(built.cut, 1850.0, 1e-5);
    EXPECT_NEAR(built.fill, 1850.0, 1e-5);
    const BimModel model = corridorSolids(built);
    ASSERT_EQ(model.objects.size(), 4u);
    const auto property = [](const BimObject& object, const std::string& name) {
        for (const auto& [key, value] : object.properties) {
            if (key == name) {
                return value;
            }
        }
        return std::string("(none)");
    };
    for (const std::string side : {"left", "right"}) {
        const BimObject* fill = objectWithId(model, "corridor/Main/" + side + "/earthworks/fill/1");
        const BimObject* cut = objectWithId(model, "corridor/Main/" + side + "/earthworks/cut/1");
        ASSERT_NE(fill, nullptr) << side;
        ASSERT_NE(cut, nullptr) << side;
        EXPECT_EQ(property(*fill, "from"), "0");
        EXPECT_EQ(property(*fill, "to"), "50");
        EXPECT_EQ(property(*cut, "from"), "50");
        EXPECT_EQ(property(*cut, "to"), "100");
        EXPECT_NEAR(quantityOf(*fill, "Length"), 50.0, 1e-9);
        EXPECT_NEAR(quantityOf(*cut, "Length"), 50.0, 1e-9);
        EXPECT_NEAR(quantityOf(*fill, "GrossVolume"), 925.0, 1e-5);
        EXPECT_NEAR(quantityOf(*cut, "GrossVolume"), 925.0, 1e-5);
        EXPECT_NEAR(quantityOf(*fill, "GrossArea"), 430.0, 1e-5);
        EXPECT_NEAR(quantityOf(*cut, "GrossArea"), 430.0, 1e-5);
    }
}

TEST(CorridorObjects, EarthworksSplitWhereTheirThicknessEndsAndTheSecondPieceCarriesItsNumber)
{
    // The sawtooth ground, 40 m of road at 10 m sections: fill at 0, 20 and 40
    // (D = 5: 45 m2 a side, 14 m wide), cut at 10 and 30 (45 m2, 14 m wide),
    // nothing at the sections between. Each piece runs over its sections and
    // one more at each end, where its end area tapers to nothing:
    //   fill 1    x 0-10    (45 + 0) 5 = 225 m3,  triangle 0.5 x 14 x 10 = 70 m2
    //   fill 1-2  x 10-30   450 m3, a diamond 20 long and 14 wide, 140 m2
    //   fill 1-3  x 30-40   225 m3, 70 m2
    //   cut 1     x 0-20    450 m3, 140 m2;  cut 1-2  x 20-40  450 m3, 140 m2.
    const auto ground = sawGround();
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}), straight(40.0),
                                     level(10.0, 0.0, 40.0), &ground);
    EXPECT_EQ(built.incompleteSections, 0u);
    const BimModel model = corridorSolids(built);
    struct Expected {
        const char* id;
        double length;
        double volume;
        double area;
    };
    for (const Expected& expected :
         {Expected{"corridor/Main/left/earthworks/fill/1", 10.0, 225.0, 70.0},
          Expected{"corridor/Main/left/earthworks/fill/1-2", 20.0, 450.0, 140.0},
          Expected{"corridor/Main/left/earthworks/fill/1-3", 10.0, 225.0, 70.0},
          Expected{"corridor/Main/left/earthworks/cut/1", 20.0, 450.0, 140.0},
          Expected{"corridor/Main/left/earthworks/cut/1-2", 20.0, 450.0, 140.0}}) {
        const BimObject* object = objectWithId(model, expected.id);
        ASSERT_NE(object, nullptr) << expected.id;
        EXPECT_NEAR(quantityOf(*object, "Length"), expected.length, 1e-9) << expected.id;
        EXPECT_NEAR(quantityOf(*object, "GrossVolume"), expected.volume, 1e-5) << expected.id;
        EXPECT_NEAR(quantityOf(*object, "GrossArea"), expected.area, 1e-5) << expected.id;
    }
    EXPECT_EQ(objectWithId(model, "corridor/Main/left/earthworks/fill/1-4"), nullptr);
    EXPECT_EQ(objectWithId(model, "corridor/Main/left/earthworks/cut/1-3"), nullptr);
    // Five pieces a side, ten in all, and the pieces' volumes are the
    // corridor's: 900 m3 of fill and 900 of cut a side.
    EXPECT_EQ(model.objects.size(), 10u);
    EXPECT_NEAR(built.fill, 1800.0, 1e-5);
    EXPECT_NEAR(built.cut, 1800.0, 1e-5);
}

TEST(CorridorObjects, GroundOnTheOtherSideOfTheDesignByNothingMakesNoEarthworksAtAll)
{
    // The daylight is found by bisection, to about 1e-13 m, so it lands on
    // either side of the ground by that much and leaves a sliver of the other
    // kind of earthworks, about 1e-26 m2, that a test of "area > 0" took for
    // a solid: a full-length cut object with the whole half-section's area
    // beside a road in fill, and a fill beside one in cut. Ground tilted
    // across the road, so the daylight is never at a round number.
    for (const double tilt : {0.03, 0.05, 0.07, 0.11}) {
        Corridor corridor = corridorOf("test");
        corridor.surface = "ground";
        const auto below = planeGround(0.0, tilt, 5.0); // under the design of 10
        const auto fill = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}),
                                        straight(100.0), level(10.0, 0.0, 100.0), &below);
        EXPECT_EQ(fill.incompleteSections, 0u) << tilt;
        EXPECT_EQ(fill.cut, 0.0) << "no cut at all, not 1e-26 of it, tilt " << tilt;
        EXPECT_GT(fill.fill, 0.0);
        const BimModel fillModel = corridorSolids(fill);
        ASSERT_EQ(fillModel.objects.size(), 2u) << tilt;
        for (const BimObject& object : fillModel.objects) {
            EXPECT_EQ(object.ifcClass, "IfcEarthworksFill") << object.id << " tilt " << tilt;
        }

        const auto above = planeGround(0.0, tilt, 15.0); // over it
        const auto cut = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}),
                                       straight(100.0), level(10.0, 0.0, 100.0), &above);
        EXPECT_EQ(cut.fill, 0.0) << "no fill at all, tilt " << tilt;
        EXPECT_GT(cut.cut, 0.0);
        const BimModel cutModel = corridorSolids(cut);
        ASSERT_EQ(cutModel.objects.size(), 2u) << tilt;
        for (const BimObject& object : cutModel.objects) {
            EXPECT_EQ(object.ifcClass, "IfcEarthworksCut") << object.id << " tilt " << tilt;
        }
    }

    // Ground at the design level: nothing to build, no object, no area.
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto flat = planeGround(0.0, 0.0, 10.0);
    const auto atGrade = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}),
                                       straight(100.0), level(10.0, 0.0, 100.0), &flat);
    EXPECT_EQ(atGrade.cut, 0.0);
    EXPECT_EQ(atGrade.fill, 0.0);
    EXPECT_TRUE(corridorSolids(atGrade).objects.empty());

    // The tolerance is the model's 0.1 mm (the TIN's duplicate tolerance): a
    // sheet of cut 0.05 mm thick is not earthworks, 0.2 mm is. A side of it:
    // 4 m of lane at 0.0002 m, 8e-4 m2, and the batter 2:1 up to the ground
    // 0.0004 m out, 0.5 x 0.0004 x 0.0002 = 4e-8 m2: 0.080004 m3 over 100 m,
    // 0.160008 m3 both sides.
    const auto thin = planeGround(0.0, 0.0, 10.00005);
    const auto sheet = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}),
                                     straight(100.0), level(10.0, 0.0, 100.0), &thin);
    EXPECT_EQ(sheet.cut, 0.0);
    EXPECT_TRUE(corridorSolids(sheet).objects.empty());
    const auto thick = planeGround(0.0, 0.0, 10.0002);
    const auto layer = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}),
                                     straight(100.0), level(10.0, 0.0, 100.0), &thick);
    EXPECT_NEAR(layer.cut, 0.160008, 1e-6);
    EXPECT_EQ(corridorSolids(layer).objects.size(), 2u);
}

TEST(CorridorObjects, ARegionIsARunOfItsOwnNumberedInStationOrder)
{
    // The lanes template, with a region of the textbook cut template from 40
    // to 60: three runs, 1 (0-40), 2 (40-60), 3 (60-100).
    Corridor corridor = corridorOf("lanes");
    corridor.regions = {TemplateRegion{40.0, 60.0, "test"}};
    const auto built = buildCorridor(corridor, setOf({lanes(), laneTemplate(4.0, 0.0, {})}),
                                     straight(100.0), level(10.0, 0.0, 100.0), nullptr);
    ASSERT_EQ(built.runs.size(), 3u);
    EXPECT_EQ(built.runs[1].region, 2u);
    EXPECT_EQ(built.runs[1].roadTemplate.name, "test");
    EXPECT_EQ(built.runs[1].sections.front().station, 40.0);
    EXPECT_EQ(built.runs[1].sections.back().station, 60.0);
    const BimModel model = corridorSolids(built);
    const BimObject* first = objectWithId(model, "corridor/Main/left/lane/AC14/1");
    const BimObject* last = objectWithId(model, "corridor/Main/left/lane/AC14/3");
    ASSERT_NE(first, nullptr);
    ASSERT_NE(last, nullptr);
    EXPECT_NEAR(quantityOf(*first, "GrossVolume"), 3.5 * 0.05 * 40.0, 1e-9);
    EXPECT_NEAR(quantityOf(*last, "GrossVolume"), 3.5 * 0.05 * 40.0, 1e-9);
    // The region's template has no courses: run 2 has no course solids.
    EXPECT_EQ(objectWithId(model, "corridor/Main/left/lane/AC14/2"), nullptr);
}

TEST(CorridorObjects, WhatCannotBeBuiltIsListedAndTheRestIsBuilt)
{
    // No surface for the batters; a region naming a template that is not
    // there; the rest of the corridor still has its pavement.
    Corridor corridor = corridorOf("test");
    corridor.regions = {TemplateRegion{40.0, 60.0, "nowhere"}};
    const auto built = buildCorridor(
        corridor, setOf({laneTemplate(4.0, 0.0, {makeCourse("AC", 0.05, "asphalt")})}),
        straight(100.0), level(10.0, 0.0, 100.0), nullptr);
    const auto mentions = [&](const std::string& words) {
        return std::ranges::any_of(built.problems, [&](const std::string& problem) {
            return problem.find(words) != std::string::npos;
        });
    };
    EXPECT_TRUE(mentions("nowhere")) << "the missing template";
    EXPECT_TRUE(mentions("no target surface")) << "the batters";
    EXPECT_EQ(built.runs.size(), 2u); // 0-40 and 60-100
    const BimModel model = corridorSolids(built);
    EXPECT_NE(objectWithId(model, "corridor/Main/left/lane/AC/1"), nullptr);
    EXPECT_NE(objectWithId(model, "corridor/Main/left/lane/AC/3"), nullptr);
    EXPECT_EQ(model.problems.size(), built.problems.size());
    EXPECT_EQ(model.problems.front().source, "corridor Main");
}

TEST(CorridorObjects, AnAlignmentAndProfileThatDoNotOverlapBuildNothingAndSaySo)
{
    const auto built = buildCorridor(corridorOf("lanes"), setOf({lanes()}), straight(100.0),
                                     level(10.0, 300.0, 400.0), nullptr);
    EXPECT_TRUE(built.runs.empty());
    ASSERT_FALSE(built.problems.empty());
    EXPECT_NE(built.problems.front().find("do not overlap"), std::string::npos);
    EXPECT_TRUE(corridorSolids(built).objects.empty());
}

TEST(CorridorObjects, StringsRunAlongEachCodeAndTheSurfacesLieOnTheSections)
{
    const auto built = buildCorridor(corridorOf("lanes"), setOf({lanes()}), straight(100.0),
                                     level(10.0, 0.0, 100.0), nullptr);
    const auto strings = corridorStrings(built, false);
    std::map<std::string, std::size_t> points;
    for (const CorridorString& string : strings) {
        points[string.code] = string.points.size();
    }
    EXPECT_EQ(points, (std::map<std::string, std::size_t>{
                          {"CL", 11}, {"left/lane/outer", 11}, {"right/lane/outer", 11}}));

    const auto finished = sectionsSurface(corridorSections(built), *built.alignment, false, false);
    ASSERT_TRUE(finished.ok()) << finished.error().describe();
    // On the centreline the design level; halfway across the lane 1.75 x
    // 3 % = 0.0525 below it.
    EXPECT_NEAR(*finished->surface.elevationAt(50.0, 0.0), 10.0, 1e-9);
    EXPECT_NEAR(*finished->surface.elevationAt(50.0, 1.75), 9.9475, 1e-9);
    const auto formation = sectionsSurface(corridorSections(built), *built.alignment, true, false);
    ASSERT_TRUE(formation.ok()) << formation.error().describe();
    EXPECT_NEAR(*formation->surface.elevationAt(50.0, 0.0), 9.75, 1e-9);
    EXPECT_EQ(formation->sections, 11u);
}

TEST(CorridorObjects, TheSolidsLoftAtTheSectionsAndRefuseTooFewOfThem)
{
    const auto alignment = straight(100.0);
    EXPECT_FALSE(loftSolid(alignment, {0.0}, {{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0}}}).ok());
    EXPECT_FALSE(loftSolid(alignment, {0.0, 10.0},
                           {{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0}}, {SolidColumn{0, 1, 0}}})
                     .ok());
    // A 1 x 1 x 10 box: 8 corners, 12 triangles, 10 m3.
    const auto box = loftSolid(alignment, {0.0, 10.0},
                               {{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0}},
                                {SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0}}});
    ASSERT_TRUE(box.ok());
    EXPECT_EQ(box->vertices.size(), 8u);
    EXPECT_EQ(box->faces.size(), 12u);
    EXPECT_NEAR(meshVolume(*box), 10.0, 1e-12);
}
