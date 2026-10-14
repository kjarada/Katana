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
    const auto built = buildCorridor(corridorOf("test"), setOf({laneTemplate(4.0, 0.0, {})}),
                                     straight(), level(-1.0), &ground);
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
    EXPECT_EQ(left->layer, "road/earthworks/cut");
    // No fill anywhere: no solid of nothing.
    EXPECT_EQ(objectWithId(model, "corridor/Main/left/earthworks/fill/1"), nullptr);
    // The solid is drawn through 41 columns a side, every 0.15 m of its 6,
    // so it holds the end area's volume less the corner the columns at 3.9
    // and 4.05 cut off the lane's edge at 4: a triangle of 0.5 x (0.1 x
    // 0.025) = 0.00125 m2 a section, 0.25 m3 over 200 m - 999.75.
    EXPECT_NEAR(meshVolume(left->mesh), 999.75, 1e-6);
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
