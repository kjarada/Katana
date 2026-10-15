// A whole corridor and the road model's objects made from it
// (include/katana/cad/road/road_section.hpp, corridorSolids): volumes against
// width x thickness x length, closed solids, stable ids, the earthworks
// against the end areas the old assembly measured, strings and surfaces.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <limits>
#include <map>
#include <numbers>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/math/numerics.hpp"
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

TEST(CorridorObjects, ADividedRoadsKerbAndLanesAreSetOutUnderTheMedianNotAboveIt)
{
    // The median kerb's cross-section, by hand: the top 0.15 wide and 0.25 thick
    // (level 0 on a base of -0.25) 0.0375 m2, and the face falling over 0.15 m
    // from 0.25 to 0.15 thick, 0.15 x 0.2 = 0.03 m2: 0.0675 m2, 6.75 m3 in 100
    // m, 0.3 m wide. Lane 1 starts under the profile (10): its top reaches 10 -
    // 0.1 = 9.9 at the inner edge, not 10.1.
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    const BimModel model = corridorSolids(buildCorridor(corridorOf("divided road"), templates,
                                                        straight(100.0), level(10.0, 0.0, 100.0),
                                                        nullptr));
    for (const std::string side : {"left", "right"}) {
        const BimObject* kerb = objectWithId(model, "corridor/Main/" + side + "/kerb/1");
        ASSERT_NE(kerb, nullptr) << side;
        EXPECT_NEAR(quantityOf(*kerb, "Width"), 0.3, 1e-12);
        EXPECT_NEAR(quantityOf(*kerb, "GrossVolume"), 6.75, 1e-9);
        EXPECT_GT(meshVolume(kerb->mesh), 0.0) << "turned outward";
        EXPECT_NE(std::ranges::find(kerb->properties,
                                    std::pair<std::string, std::string>{"mountable", "true"}),
                  kerb->properties.end());
        const BimObject* lane = objectWithId(model, "corridor/Main/" + side + "/lane1/AC14/1");
        ASSERT_NE(lane, nullptr) << side;
        EXPECT_NEAR(lane->mesh.bounds().max.z, 9.9, 1e-12);
    }
}

TEST(CorridorObjects, AFlushEdgeStripReportsHowThickItIsAndNoFaceHeight)
{
    // 0.15 m wide at 0 % and 0.15 m thick: 0.0225 m2, 2.25 m3 in 100 m. It has
    // no face: its Height was the kerb default 0.15 (or whatever was typed,
    // 5 m in the finding) and the IFC base quantity carried it.
    RoadTemplate made;
    made.name = "strips";
    for (const Side side : {Side::Left, Side::Right}) {
        Component lane = defaultComponent(ComponentKind::Lane, side);
        Component strip = defaultComponent(ComponentKind::Kerb, side);
        strip.shape = KerbShape::EdgeStrip;
        strip.crossfall = 0.0;
        strip.height = 5.0;
        made.components.insert(made.components.end(), {lane, strip});
    }
    const BimModel model = corridorSolids(buildCorridor(corridorOf("strips"), setOf({made}),
                                                        straight(100.0), level(10.0, 0.0, 100.0),
                                                        nullptr));
    const BimObject* strip = objectWithId(model, "corridor/Main/left/kerb/1");
    ASSERT_NE(strip, nullptr);
    EXPECT_NEAR(quantityOf(*strip, "Width"), 0.15, 1e-12);
    EXPECT_NEAR(quantityOf(*strip, "Depth"), 0.15, 1e-12);
    EXPECT_NEAR(quantityOf(*strip, "GrossVolume"), 2.25, 1e-9);
    EXPECT_TRUE(std::ranges::none_of(strip->quantities, [](const katana::cad::bim::Quantity& each) {
        return each.name == "Height";
    }));
    // A kerb with a face keeps its Height, the face.
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    const BimModel urban = corridorSolids(buildCorridor(
        corridorOf("urban street"), templates, straight(100.0), level(10.0, 0.0, 100.0), nullptr));
    EXPECT_NEAR(quantityOf(*objectWithId(urban, "corridor/Main/left/kerb/1"), "Height"), 0.15,
                1e-15);
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

TEST(CorridorObjects, ARegionsStringsRunOverItsOwnStationsAndTheSurfaceStepsAtItsEnds)
{
    // The rural two-lane road with the urban street from 40 to 60, on a 100 m
    // straight at 10 m: runs 0-40, 40-60 and 60-100, 5 + 3 + 5 = 13 sections.
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    Corridor corridor = corridorOf("rural two-lane");
    corridor.regions = {TemplateRegion{40.0, 60.0, "urban street"}};
    const auto built = buildCorridor(corridor, templates, straight(100.0), level(10.0, 0.0, 100.0),
                                     nullptr);
    ASSERT_EQ(built.runs.size(), 3u);
    EXPECT_EQ(built.sections, 13u);

    // Strings, by code: the first and last x of each string made.
    using Span = std::pair<double, double>;
    std::map<std::string, std::vector<Span>> spans;
    const auto strings = corridorStrings(built, false);
    for (const CorridorString& string : strings) {
        spans[string.code].emplace_back(string.points.front().x, string.points.back().x);
    }
    const auto expectSpans = [&](const std::string& code, const std::vector<Span>& expected) {
        const auto& found = spans[code];
        ASSERT_EQ(found.size(), expected.size()) << code;
        for (std::size_t i = 0; i < expected.size(); ++i) {
            EXPECT_NEAR(found[i].first, expected[i].first, 1e-9) << code << " string " << i;
            EXPECT_NEAR(found[i].second, expected[i].second, 1e-9) << code << " string " << i;
        }
    };
    // What only the region has runs its whole 40 to 60 (it began at 50: the
    // region's section at 40 was dropped for the rural run's).
    expectSpans("left/footpath/outer", {{40.0, 60.0}});
    expectSpans("right/kerb/back", {{40.0, 60.0}});
    // What only the rural template has stops at 40 and resumes at 60 (it
    // resumed at 70: the section at 60 was dropped for the urban run's).
    expectSpans("left/shoulder/outer", {{0.0, 40.0}, {60.0, 100.0}});
    expectSpans("right/drain/invert", {{0.0, 40.0}, {60.0, 100.0}});
    // What both have is broken at each join, not carried across one.
    expectSpans("left/lane/outer", {{0.0, 40.0}, {40.0, 60.0}, {60.0, 100.0}});
    expectSpans("CL", {{0.0, 40.0}, {40.0, 60.0}, {60.0, 100.0}});
    // The centreline and 4 + 4 rural codes (the lane's and shoulder's edges and
    // the drain's invert and outer edge) a side before and after the
    // region, and the urban street's centreline and 7 + 7 for the region: 9 + 15
    // + 9.
    EXPECT_EQ(strings.size(), 33u);

    // The finished surface: the urban street's kerb top (9.964: 10 less the
    // lane's 3.3 x 3 % = 0.099 and the parking's 2.3 x 3 % = 0.069 to the lip at
    // 9.832, the gutter 0.3 m falling 6 % to 9.814, the face up 0.15 to
    // 9.964) at 6 m out in the region, the rural drain (5.5 m at 9.835, its
    // invert at 6.5 m 0.5 down, so 9.585 at 6 m) outside it - and a step between,
    // not a blend: 1 mm either side of the join the surface is each template's.
    const auto finished = sectionsSurface(corridorSections(built), *built.alignment, false, false);
    ASSERT_TRUE(finished.ok()) << finished.error().describe();
    EXPECT_EQ(finished->sections, 13u) << "every section built, the join's both";
    const auto level6 = [&](double station) {
        const auto z = finished->surface.elevationAt(station, 6.0);
        EXPECT_TRUE(z.has_value()) << station;
        return z.value_or(0.0);
    };
    EXPECT_NEAR(level6(35.0), 9.585, 1e-9);
    EXPECT_NEAR(level6(39.999), 9.585, 1e-9);
    EXPECT_NEAR(level6(40.002), 9.964, 1e-9);
    EXPECT_NEAR(level6(45.0), 9.964, 1e-9);
    EXPECT_NEAR(level6(55.0), 9.964, 1e-9);
    EXPECT_NEAR(level6(59.999), 9.964, 1e-9);
    EXPECT_NEAR(level6(60.002), 9.585, 1e-9);
    EXPECT_NEAR(level6(65.0), 9.585, 1e-9);
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

// ---- courses and kerbs round a curve (the chord error, docs/road_design.md) ------------------

namespace {

// (0,0) to (100,0) to (100,100), the corner rounded at 50 m: a quarter turn to
// the left, 100 m of tangent and the arc from station 50 to 50 + 25 pi =
// 128.5398 between them, 178.5398 in all.
katana::geometry::SolvedAlignment quarterTurn()
{
    katana::geometry::HorizontalAlignment definition;
    definition.pis = {katana::geometry::AlignmentPI{katana::geometry::Point2(0, 0)},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(100, 0), 50.0},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(100, 100)}};
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok()) << solved.error().describe();
    return *solved;
}

// The AC14 course (50 mm) of the 3.5 m lanes round the quarter turn, with the
// corridor's curve tolerance as given: the left lane's volume, then the right's.
std::pair<double, double> laneVolumesOnTheQuarterTurn(double curveTolerance,
                                                      std::size_t* sections = nullptr)
{
    Corridor corridor = corridorOf("lanes");
    corridor.curveTolerance = curveTolerance;
    const auto built = buildCorridor(corridor, setOf({lanes()}), quarterTurn(),
                                     level(10.0, 0.0, 300.0), nullptr);
    EXPECT_TRUE(built.problems.empty()) << built.problems.front();
    if (sections != nullptr) {
        *sections = built.sections;
    }
    const BimModel model = corridorSolids(built);
    const BimObject* left = objectWithId(model, "corridor/Main/left/lane/AC14/1");
    const BimObject* right = objectWithId(model, "corridor/Main/right/lane/AC14/1");
    if (left == nullptr || right == nullptr) {
        ADD_FAILURE() << "a lane's AC14 is missing";
        return {};
    }
    // The solid's own volume and the plan outline's area say the same: the
    // volume is that area at a thickness of 0.05.
    EXPECT_NEAR(quantityOf(*left, "GrossVolume"), quantityOf(*left, "GrossArea") * 0.05, 1e-9);
    EXPECT_NEAR(quantityOf(*right, "GrossVolume"), quantityOf(*right, "GrossArea") * 0.05, 1e-9);
    return {quantityOf(*left, "GrossVolume"), quantityOf(*right, "GrossVolume")};
}

} // namespace

TEST(CorridorObjects, ACoursesVolumeRoundAQuarterCircleIsTheAnnulusSectorsToTheCurveTolerance)
{
    // The left lane is on the inside of a left turn: 3.5 m wide, from r = 46.5
    // to r = 50; the right lane from 50 to 53.5. Each AC14 is 50 mm thick and
    // lies over 100 m of tangent (3.5 x 100 x 0.05 = 17.5 m3) and a quarter of
    // an annulus, pi/4 (R2^2 - R1^2) x 0.05:
    //   left   pi/4 (2500 - 2162.25) = pi/4 x 337.75 = 265.268 m2   13.2634 m3
    //   right  pi/4 (2862.25 - 2500) = pi/4 x 362.25 = 284.510 m2   14.2255 m3
    // 30.7634 and 31.7255 m3 in all.
    const double pi = std::numbers::pi;
    const double leftArc = pi / 4.0 * (50.0 * 50.0 - 46.5 * 46.5) * 0.05;
    const double rightArc = pi / 4.0 * (53.5 * 53.5 - 50.0 * 50.0) * 0.05;
    const double leftExact = 17.5 + leftArc;
    const double rightExact = 17.5 + rightArc;
    std::size_t sections = 0;
    const auto [left, right] = laneVolumesOnTheQuarterTurn(kDefaultCurveTolerance, &sections);
    // A tolerance of 0.001 holds the arc's part to a thousandth - the chords
    // are short by theta^2 / 6 at most, theta = 0.0775 - and it is always
    // short, never over. (Without it the same arc is 0.7 % short: below.)
    EXPECT_LE(left, leftExact);
    EXPECT_LE(right, rightExact);
    EXPECT_GE(left, leftExact - 0.001 * leftArc);
    EXPECT_GE(right, rightExact - 0.001 * rightArc);
    // The sections: 20 the interval and the keys give (0, 10, ..., 120,
    // 128.5398, 130, ..., 170, 178.5398) and two more in each of the arc's
    // eight gaps (three parts each: 10 m turns 0.2 radians, over the 0.0775
    // allowed), 36.
    EXPECT_EQ(sections, 36u);
}

TEST(CorridorObjects, ACoursesVolumeRoundAQuarterCircleWithNoToleranceIsTheChordsOfTheIntervalStations)
{
    // curveTolerance 0 adds no section: the 20 of the interval and the keys,
    // and the quads between them. Round the arc the stations are 50, 60, ...,
    // 120 (seven steps of 10 m, theta = 0.2) and 128.5398 (8.5398 m, theta =
    // 0.17080). An annulus sector's chord quad is 0.5 sin(theta) (R2^2 - R1^2),
    // so the arc holds
    //   left   0.5 (7 sin 0.2 + sin 0.17080) 337.75 = 263.555 m2  13.1778 m3
    //   right  0.5 (7 sin 0.2 + sin 0.17080) 362.25 = 282.673 m2  14.1337 m3
    // against the exact 265.268 and 284.510: 0.65 % short, theta^2 / 6.
    const double pi = std::numbers::pi;
    double sines = 7.0 * std::sin(10.0 / 50.0);
    sines += std::sin((25.0 * pi - 70.0) / 50.0);
    const double leftChords = 17.5 + 0.5 * sines * (50.0 * 50.0 - 46.5 * 46.5) * 0.05;
    const double rightChords = 17.5 + 0.5 * sines * (53.5 * 53.5 - 50.0 * 50.0) * 0.05;
    std::size_t sections = 0;
    const auto [left, right] = laneVolumesOnTheQuarterTurn(0.0, &sections);
    EXPECT_NEAR(left, leftChords, 1e-9);
    EXPECT_NEAR(right, rightChords, 1e-9);
    EXPECT_EQ(sections, 20u);
    // And they are short of the curve by far more than the default allows.
    const double leftArc = pi / 4.0 * (50.0 * 50.0 - 46.5 * 46.5) * 0.05;
    EXPECT_LT(left, 17.5 + leftArc - 0.001 * leftArc);
}

TEST(CorridorObjects, AKerbsLengthRoundAQuarterCircleIsItsArcToAQuarterOfTheCurveTolerance)
{
    // The urban street's left kerb has its lip 3.3 + 2.3 = 5.6 m from the
    // centreline, r = 44.4 m on the inside of the turn: 100 m of tangent and
    // pi/2 x 44.4 = 69.7434 m of arc, 169.7434 in all. A chord holds
    // sin(theta/2)/(theta/2) of its arc, theta^2 / 24 short, so with
    // theta = 0.0775 the length is within 0.001/4 of the arc's 69.74: 0.0174 m.
    // Without the added sections the same kerb is 69.6306 m on the arc (the
    // 7 x 10 m and 8.5398 m chords, 2 r sin(theta/2) each), 0.113 short.
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    const double exact = 100.0 + std::numbers::pi / 2.0 * 44.4;
    Corridor corridor = corridorOf("urban street");
    const auto built = buildCorridor(corridor, templates, quarterTurn(), level(10.0, 0.0, 300.0),
                                     nullptr);
    const BimModel model = corridorSolids(built);
    const BimObject* kerb = objectWithId(model, "corridor/Main/left/kerb/1");
    ASSERT_NE(kerb, nullptr);
    const double length = quantityOf(*kerb, "Length");
    EXPECT_LE(length, exact);
    EXPECT_GE(length, exact - 0.001 / 4.0 * (std::numbers::pi / 2.0 * 44.4));
    corridor.curveTolerance = 0.0;
    const BimModel chords = corridorSolids(
        buildCorridor(corridor, templates, quarterTurn(), level(10.0, 0.0, 300.0), nullptr));
    const BimObject* chorded = objectWithId(chords, "corridor/Main/left/kerb/1");
    ASSERT_NE(chorded, nullptr);
    // 100 + the seven 10 m chords at theta = 0.2 and the 8.5398 m one.
    double arc = 7.0 * 2.0 * 44.4 * std::sin(0.1);
    arc += 2.0 * 44.4 * std::sin((25.0 * std::numbers::pi - 70.0) / 100.0);
    EXPECT_NEAR(quantityOf(*chorded, "Length"), 100.0 + arc, 1e-9);
}

// ---- solids that taper: no face of no area (docs/road_design.md, "A closed solid") --------

namespace {

// Every edge of `mesh` used by exactly two faces, once in each direction: the
// test the IFC writer applies to its indices for "Closed" (docs/ifc.md).
bool closedOnItsIndices(const katana::geometry::TriangleMesh& mesh)
{
    std::map<std::pair<std::uint32_t, std::uint32_t>, int> undirected;
    std::map<std::pair<std::uint32_t, std::uint32_t>, int> directed;
    for (const auto& face : mesh.faces) {
        for (std::size_t e = 0; e < 3; ++e) {
            const std::uint32_t a = face[e];
            const std::uint32_t b = face[(e + 1) % 3];
            ++undirected[{std::min(a, b), std::max(a, b)}];
            ++directed[{a, b}];
        }
    }
    return !mesh.faces.empty() &&
           std::ranges::all_of(undirected, [](const auto& e) { return e.second == 2; }) &&
           std::ranges::all_of(directed, [](const auto& e) { return e.second == 1; });
}

// The area of the smallest triangle: a face of no area is a sliver the mesh is
// closed with and a viewer may refuse.
double thinnestTriangle(const katana::geometry::TriangleMesh& mesh)
{
    double thinnest = std::numeric_limits<double>::infinity();
    for (std::size_t i = 0; i < mesh.triangleCount(); ++i) {
        thinnest = std::min(thinnest, mesh.triangle(i)->area());
    }
    return thinnest;
}

// How many separate pieces the faces hold: faces that share a vertex are one.
std::size_t shellsOf(const katana::geometry::TriangleMesh& mesh)
{
    std::vector<std::size_t> parent(mesh.vertices.size());
    for (std::size_t i = 0; i < parent.size(); ++i) {
        parent[i] = i;
    }
    const auto root = [&parent](std::size_t i) {
        while (parent[i] != i) {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        return i;
    };
    std::vector<bool> used(mesh.vertices.size(), false);
    for (const auto& face : mesh.faces) {
        for (const std::uint32_t index : face) {
            used[index] = true;
            parent[root(index)] = root(face[0]);
        }
    }
    std::set<std::size_t> roots;
    for (std::size_t i = 0; i < parent.size(); ++i) {
        if (used[i]) {
            roots.insert(root(i));
        }
    }
    return roots.size();
}

// A ground that depends on y alone (a TIN on a grid, as sawGround is on x): 10.5
// from the centreline to 0.5 out, falling to 9.5 at 1.5 out, level to 2.5,
// rising to 10.5 at 3.5 and level beyond, the same each side. Under a design
// level of 10, with a 4 m lane and 2:1 batters, each side has a CUT out to
// where the ground crosses 10 (1.0), a FILL to where it crosses back (3.0), and
// a cut again to where the cut batter, rising from the lane's edge at 4 m,
// meets the ground (10.5 at 5.0): two stretches of cut across every section,
// with a fill between them.
katana::terrain::TinSurface twoCutsAcrossGround()
{
    katana::terrain::TinInput input;
    const std::pair<double, double> profile[] = {{0.0, 10.5},  {0.5, 10.5},  {1.5, 9.5},
                                                 {2.5, 9.5},   {3.5, 10.5},  {100.0, 10.5}};
    for (const double x : {-10.0, 0.0, 50.0, 100.0, 110.0}) {
        for (const auto& [y, z] : profile) {
            input.points.emplace_back(x, y, z);
            if (y != 0.0) {
                input.points.emplace_back(x, -y, z);
            }
        }
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built->surface;
}

} // namespace

TEST(CorridorObjects, ASolidThatTapersToALineIsAWedgeWeldedToItWithNoFaceOfNoArea)
{
    // Three columns 1 m apart, 1 m thick at station 0 and nothing at station 10
    // (its top and bottom are the one line at level 0). Vertices: 6 at station
    // 0 and 3 at 10. Triangles: the top 4 and the bottom 4 over two cells, the
    // inner and the outer face one each (a quad whose far side is a line is one
    // triangle), the cap at 0 four, the cap at 10 none: 14. V - E + F = 9 - 21 +
    // 14 = 2, a sphere. The volume is half the end area, 2 x 1 = 2 m2, over 10
    // m: 10 m3 (a wedge).
    const auto alignment = straight(100.0);
    const auto wedge = loftSolid(
        alignment, {0.0, 10.0},
        {{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0}, SolidColumn{2, 1, 0}},
         {SolidColumn{0, 0, 0}, SolidColumn{1, 0, 0}, SolidColumn{2, 0, 0}}});
    ASSERT_TRUE(wedge.ok()) << wedge.error().describe();
    EXPECT_EQ(wedge->vertices.size(), 9u);
    EXPECT_EQ(wedge->faces.size(), 14u);
    EXPECT_TRUE(closedOnItsIndices(*wedge));
    EXPECT_NEAR(meshVolume(*wedge), 10.0, 1e-12);
    EXPECT_GT(thinnestTriangle(*wedge), 1e-3) << "a face of no area is left in";
}

TEST(CorridorObjects, AnEdgeColumnOfNoThicknessIsAWeldedLineAndMakesNoFaceOfNoArea)
{
    // Four columns, thick 1 m but the last, which is a line (ground meeting
    // the formation): two stations, 7 vertices each (the last column is one),
    // 14. Triangles: top 6, bottom 6, inner 2, outer 0 (all four corners are
    // the one line), the two caps 5 each (their last cell is one triangle): 24.
    // V - E + F = 14 - 36 + 24 = 2. The section is 1 + 1 + 0.5 = 2.5 m2, 25 m3.
    const auto alignment = straight(100.0);
    const std::vector<SolidColumn> section{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0},
                                           SolidColumn{2, 1, 0}, SolidColumn{3, 0, 0}};
    const auto solid = loftSolid(alignment, {0.0, 10.0}, {section, section});
    ASSERT_TRUE(solid.ok()) << solid.error().describe();
    EXPECT_EQ(solid->vertices.size(), 14u);
    EXPECT_EQ(solid->faces.size(), 24u);
    EXPECT_TRUE(closedOnItsIndices(*solid));
    EXPECT_NEAR(meshVolume(*solid), 25.0, 1e-12);
    EXPECT_GT(thinnestTriangle(*solid), 1e-3);

    // A thickness under the weld bound is a line too, at the top's level (5
    // vertices a station); over it, a face (6). 0.00005 m is under the model's
    // 0.1 mm, 0.0002 over it.
    for (const auto& [thickness, welds] : {std::pair{0.00005, true}, std::pair{0.0002, false}}) {
        const std::vector<SolidColumn> thin{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0},
                                            SolidColumn{2, 1, 1 - thickness}};
        const auto made = loftSolid(alignment, {0.0, 10.0}, {thin, thin}, 1e-4);
        ASSERT_TRUE(made.ok());
        EXPECT_EQ(made->vertices.size(), welds ? 10u : 12u) << thickness;
        EXPECT_TRUE(closedOnItsIndices(*made)) << thickness;
    }
}

TEST(CorridorObjects, TwoStretchesWithNothingBetweenThemAreTwoShellsAndTheGapHasNoFace)
{
    // Six columns, 1 m thick but the middle two (a gap), on two stations that
    // are both ends of the solid: the cell across the gap has nothing between
    // its top and its bottom at its four corners, two faces on one surface, and
    // is left out. Vertices: 10 a station (the middle two are one each), 20.
    // Triangles: top 8, bottom 8, inner 4, outer 4, the caps 6 each: 32. Two
    // spheres: V - E + F = 20 - 48 + 32 = 4. Each shell is 1 + 0.5 = 1.5 m2:
    // 30 m3 in all, over 10 m.
    const auto alignment = straight(100.0);
    const std::vector<SolidColumn> section{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0},
                                           SolidColumn{2, 0, 0}, SolidColumn{3, 0, 0},
                                           SolidColumn{4, 1, 0}, SolidColumn{5, 1, 0}};
    const auto solid = loftSolid(alignment, {0.0, 10.0}, {section, section});
    ASSERT_TRUE(solid.ok()) << solid.error().describe();
    EXPECT_EQ(solid->vertices.size(), 20u);
    EXPECT_EQ(solid->faces.size(), 32u);
    EXPECT_TRUE(closedOnItsIndices(*solid));
    EXPECT_EQ(shellsOf(*solid), 2u);
    EXPECT_NEAR(meshVolume(*solid), 30.0, 1e-12);
    EXPECT_GT(thinnestTriangle(*solid), 1e-3);
}

TEST(CorridorObjects, AnInteriorColumnOfNoThicknessIsNotWeldedSoASolidNeverPinchesToALine)
{
    // Three stations, 1 m thick everywhere but the middle column of the middle
    // station, which is nothing. It is not welded: welded, the line it makes
    // with a body on each side would be an edge four faces use. So the solid has
    // 6 vertices a station, 18, and 32 triangles (top 8, bottom 8, inner 4, outer
    // 4, the two caps 4 each), V - E + F = 18 - 48 + 32 = 2.
    // Its volume is the plan area of each of the four cells' two triangles, 5 m2
    // a triangle, times the mean thickness at its corners: the cells have
    // 2/3, 2/3; 1, 2/3; 2/3, 1; 2/3, 2/3: 5 x (4/3 + 5/3 + 5/3 + 4/3) = 30 m3.
    const auto alignment = straight(100.0);
    const std::vector<SolidColumn> thick{SolidColumn{0, 1, 0}, SolidColumn{1, 1, 0},
                                         SolidColumn{2, 1, 0}};
    const std::vector<SolidColumn> pinched{SolidColumn{0, 1, 0}, SolidColumn{1, 0, 0},
                                           SolidColumn{2, 1, 0}};
    const auto solid = loftSolid(alignment, {0.0, 10.0, 20.0}, {thick, pinched, thick});
    ASSERT_TRUE(solid.ok()) << solid.error().describe();
    EXPECT_EQ(solid->vertices.size(), 18u);
    EXPECT_EQ(solid->faces.size(), 32u);
    EXPECT_TRUE(closedOnItsIndices(*solid));
    EXPECT_EQ(shellsOf(*solid), 1u);
    EXPECT_NEAR(meshVolume(*solid), 30.0, 1e-12);
    EXPECT_GT(thinnestTriangle(*solid), 1e-3);
}

TEST(CorridorObjects, ATaperingEarthworksPieceEndsInALineAndItsVolumeIsThatOfTheSolidItWas)
{
    // The sawtooth ground (as EarthworksSplitWhere...): the fill beside the lane
    // is 5 m thick to 4 m out and falls to nothing at 14 (2:1), at sections 0, 20
    // and 40, and there is none at 10 and 30. fill/1 runs from 0 (the thick
    // section) to 10 (a line along the formation), fill/1-2 from 10 to 30 with its
    // thick section at 20, fill/1-3 from 30 to 40.
    //   fill/1     41 columns of 0.35 m, 41 + 40 vertices at station 0 (the last
    //              is a line) and 41 at 10: 122; top 80 + bottom 80 + inner 1 +
    //              outer 0 + cap at 0 79 (its last cell is one triangle) + cap at
    //              10 none: 240 triangles. V - E + F = 122 - 360 + 240 = 2.
    //   fill/1-2   a line, the thick section, a line: 41 + 81 + 41 = 163 vertices,
    //              2 x 160 + inner 2 = 322 triangles: 163 - 483 + 322 = 2.
    // The mesh volume is Sum of plan area x mean thickness of each triangle's
    // corners, the plan cells 0.35 x 10 split a to c: for a section thick t_i
    // at its columns and a line beyond it that is 1.75 (2 t_i + t_(i+1)) / 3 a
    // cell, and the other way round 1.75 (2 t_(i+1) + t_i) / 3. With t = 5 to
    // 4 m out and 5 - (out - 4) / 2 beyond: 226.42083 and 223.50417, 449.925 for
    // the two ends together - the end area's 225, 225 and 450 less and plus a
    // diagonal's worth, as the old solid (a face of no area at its end) was.
    const auto ground = sawGround();
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}), straight(40.0),
                                     level(10.0, 0.0, 40.0), &ground);
    const BimModel model = corridorSolids(built);
    const BimObject* first = objectWithId(model, "corridor/Main/left/earthworks/fill/1");
    const BimObject* middle = objectWithId(model, "corridor/Main/left/earthworks/fill/1-2");
    const BimObject* last = objectWithId(model, "corridor/Main/left/earthworks/fill/1-3");
    ASSERT_NE(first, nullptr);
    ASSERT_NE(middle, nullptr);
    ASSERT_NE(last, nullptr);
    EXPECT_EQ(first->mesh.vertices.size(), 122u);
    EXPECT_EQ(first->mesh.faces.size(), 240u);
    EXPECT_EQ(middle->mesh.vertices.size(), 163u);
    EXPECT_EQ(middle->mesh.faces.size(), 322u);
    EXPECT_NEAR(meshVolume(first->mesh), 226.42083333333, 1e-6);
    EXPECT_NEAR(meshVolume(middle->mesh), 449.925, 1e-6);
    EXPECT_NEAR(meshVolume(last->mesh), 223.50416666667, 1e-6);
    // The quantities are the end areas', as they were.
    EXPECT_NEAR(quantityOf(*first, "GrossVolume"), 225.0, 1e-5);
    EXPECT_NEAR(quantityOf(*middle, "GrossVolume"), 450.0, 1e-5);
    EXPECT_NEAR(quantityOf(*first, "GrossArea"), 70.0, 1e-5);
    EXPECT_NEAR(quantityOf(*middle, "GrossArea"), 140.0, 1e-5);
    for (const BimObject* object : {first, middle, last}) {
        EXPECT_TRUE(closedOnItsIndices(object->mesh)) << object->id;
        EXPECT_GT(meshVolume(object->mesh), 0.0) << object->id << " is turned inward";
        EXPECT_GT(thinnestTriangle(object->mesh), katana::math::tolerance::kAbsolute)
            << object->id << " has a face of no area";
    }
}

TEST(CorridorObjects, ASectionWithCutFillAndCutAcrossItIsTwoShellsOfCutAndOneOfFill)
{
    // twoCutsAcrossGround: each side has, at every section, cut from the
    // centreline to 1.0 (0.5 thick to 0.5 out, then falling to nothing: 0.25 +
    // 0.125 = 0.375 m2), fill from 1.0 to 3.0 (0.125 + 0.5 + 0.125 = 0.75 m2),
    // and cut again from 3.0 to 5.0 (0.125 + 0.25 + 0.25 = 0.625 m2). Over 100
    // m: cut 100 m3, fill 75 m3 a side. The ground's kinks and the lane's edge
    // are on columns of 0.025 and 0.05 m (the cut's stretches are 1 and 2 m wide,
    // the fill's 2, each in 40 steps), so the solids hold exactly that. The
    // cut was one solid whose columns spanned the 2 m gap with no thickness; it
    // is two shells now, and the fill one.
    const auto ground = twoCutsAcrossGround();
    Corridor corridor = corridorOf("test");
    corridor.surface = "ground";
    const auto built = buildCorridor(corridor, setOf({laneTemplate(4.0, 0.0, {})}), straight(100.0),
                                     level(10.0, 0.0, 100.0), &ground);
    EXPECT_EQ(built.incompleteSections, 0u);
    EXPECT_NEAR(built.cut, 200.0, 1e-5);
    EXPECT_NEAR(built.fill, 150.0, 1e-5);
    const BimModel model = corridorSolids(built);
    ASSERT_EQ(model.objects.size(), 4u) << "a cut and a fill a side, one piece each";
    for (const std::string side : {"left", "right"}) {
        const BimObject* cut = objectWithId(model, "corridor/Main/" + side + "/earthworks/cut/1");
        const BimObject* fill = objectWithId(model, "corridor/Main/" + side + "/earthworks/fill/1");
        ASSERT_NE(cut, nullptr) << side;
        ASSERT_NE(fill, nullptr) << side;
        EXPECT_NEAR(quantityOf(*cut, "GrossVolume"), 100.0, 1e-5) << side;
        EXPECT_NEAR(quantityOf(*fill, "GrossVolume"), 75.0, 1e-5) << side;
        EXPECT_NEAR(meshVolume(cut->mesh), 100.0, 1e-5) << side;
        EXPECT_NEAR(meshVolume(fill->mesh), 75.0, 1e-5) << side;
        EXPECT_EQ(shellsOf(cut->mesh), 2u) << side;
        EXPECT_EQ(shellsOf(fill->mesh), 1u) << side;
        for (const BimObject* object : {cut, fill}) {
            EXPECT_TRUE(closedOnItsIndices(object->mesh)) << object->id;
            EXPECT_GT(thinnestTriangle(object->mesh), katana::math::tolerance::kAbsolute)
                << object->id;
        }
        // Drawn and measured as their hull (a plan outline is one polygon): the
        // cut from the centreline to 5 m out, the fill from 1 to 3.
        EXPECT_NEAR(quantityOf(*cut, "GrossArea"), 500.0, 1e-5) << side;
        EXPECT_NEAR(quantityOf(*fill, "GrossArea"), 200.0, 1e-5) << side;
    }
}

TEST(CorridorObjects, NoEarthworksSolidOfTheStandardTemplatesHasAFaceOfNoAreaOrIsOpen)
{
    // The three standard templates over ground that tilts across the road (cut
    // and fill on a side), over the sawtooth ground (pieces that taper away
    // along the road) and over the ground with two cuts across: every earthworks
    // solid is closed on its own indices, turned outward, and has no triangle
    // of less area than math::tolerance::kAbsolute (1e-12 m2): a face of no area
    // is 0, or a sliver at the daylight a few 1e-13 m thick. The cases are
    // reached: the solids are counted that have fewer vertices than 41 columns
    // top and bottom on every station would (2 x 41 a station), which every
    // solid whose edge or end is a line has, and each ground must have some.
    RoadTemplateSet templates;
    templates.templates = standardTemplates();
    struct Case {
        const char* name;
        katana::terrain::TinSurface ground;
        double length;
    };
    std::vector<Case> cases;
    cases.push_back({"tilted", planeGround(0.0, 0.25, 9.5), 100.0});
    cases.push_back({"sawtooth", sawGround(), 40.0});
    cases.push_back({"two cuts across", twoCutsAcrossGround(), 100.0});
    for (const Case& each : cases) {
        std::size_t seen = 0;
        std::size_t welded = 0;
        for (const RoadTemplate& roadTemplate : templates.templates) {
            Corridor corridor = corridorOf(roadTemplate.name);
            corridor.surface = "ground";
            const auto built = buildCorridor(corridor, templates, straight(each.length),
                                             level(10.0, 0.0, each.length), &each.ground);
            for (const BimObject& object : corridorSolids(built).objects) {
                if (object.kind != "earthworks") {
                    continue;
                }
                ++seen;
                EXPECT_TRUE(closedOnItsIndices(object.mesh))
                    << each.name << " " << roadTemplate.name << " " << object.id;
                EXPECT_GT(meshVolume(object.mesh), 0.0) << object.id << " is turned inward";
                EXPECT_GT(thinnestTriangle(object.mesh), katana::math::tolerance::kAbsolute)
                    << each.name << " " << roadTemplate.name << " " << object.id;
                // One shell of 41 columns would have 82 vertices a station,
                // 41 at a line and 81 where one end is.
                const double stations = quantityOf(object, "Length") / 10.0 + 1.0;
                if (static_cast<double>(object.mesh.vertices.size()) < 82.0 * stations) {
                    ++welded;
                }
            }
        }
        EXPECT_GT(seen, 0u) << each.name << " made no earthworks: nothing was tested";
        EXPECT_GT(welded, 0u) << each.name << " welded nothing: no taper or edge was reached";
    }
}
