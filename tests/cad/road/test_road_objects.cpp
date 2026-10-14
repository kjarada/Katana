// Road objects (include/katana/cad/road/road_objects.hpp): each kind made at
// the place and the size worked out by hand, its solids closed, its ids
// stable, its definition kept as JSON, and what cannot be placed refused or
// listed - never drawn at a guessed level.
//
// The site most tests stand on: alignment Main, straight from (0,0) to
// (100,0) - so station s, offset o is the point (s, o), left of the
// centreline being +y - with a flat design profile at level 10 (or rising
// 2 m over its length, 2 percent, where a test says so).

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <memory>
#include <numbers>
#include <string>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/road_object_mesh.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::bim::BimModel;
using katana::cad::bim::BimObject;
using katana::cad::road::buildRoadObject;
using katana::cad::road::culvertEnds;
using katana::cad::road::effectiveParameters;
using katana::cad::road::isClosedMesh;
using katana::cad::road::meshVolume;
using katana::cad::road::parseParameter;
using katana::cad::road::PlacedVertex;
using katana::cad::road::placementContext;
using katana::cad::road::PlacementKind;
using katana::cad::road::RoadObject;
using katana::cad::road::RoadObjectKind;
using katana::cad::road::RoadObjectSet;
using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Point3;

namespace {

struct Site {
    explicit Site(bool withProfile = true, double rise = 0.0)
    {
        katana::entity::Alignment main;
        main.name = "Main";
        main.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0, 0)},
                               katana::geometry::AlignmentPI{Point2(100, 0)}};
        if (withProfile) {
            main.vertical = katana::geometry::VerticalAlignment{
                {katana::geometry::ProfilePVI{0.0, 10.0},
                 katana::geometry::ProfilePVI{100.0, 10.0 + rise}}};
        }
        const auto status = document.execute(katana::commands::createAlignment(main));
        EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    }

    std::string ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }

    katana::core::Error fails(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }

    // The object's 3D objects as the drawing makes them now.
    BimModel built(const std::string& id)
    {
        const RoadObject* object = document.roadObjects().find(id);
        EXPECT_NE(object, nullptr) << id;
        if (object == nullptr) {
            return {};
        }
        return buildRoadObject(placementContext(document), *object);
    }

    Document document;
    CommandInterpreter interpreter{document};
};

const katana::cad::bim::Quantity* quantity(const BimObject& object, std::string_view name)
{
    const auto found =
        std::ranges::find(object.quantities, name, &katana::cad::bim::Quantity::name);
    return found == object.quantities.end() ? nullptr : &*found;
}

double quantityValue(const BimObject& object, std::string_view name)
{
    const auto* found = quantity(object, name);
    EXPECT_NE(found, nullptr) << object.id << " has no " << name;
    return found == nullptr ? std::nan("") : found->value;
}

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

// A plane z = 100 + 0.01 x over x -10..110, y -10..10: two triangles, wound
// counter-clockwise.
std::shared_ptr<const katana::terrain::TinSurface> slopingGround()
{
    const auto z = [](double x) { return 100.0 + 0.01 * x; };
    auto tin = katana::terrain::TinSurface::create(
        {Point3(-10, -10, z(-10)), Point3(110, -10, z(110)), Point3(110, 10, z(110)),
         Point3(-10, 10, z(-10))},
        {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(tin.ok());
    return std::make_shared<const katana::terrain::TinSurface>(std::move(*tin));
}

} // namespace

TEST(RoadObjects, ASignAtStation50Offset4StandsOnTheProfileFacingTheTraffic)
{
    // By hand: the spot is (50, 4) at level 10. One 75 mm post centred on
    // it; the 900 x 600 plate on its front, facing back along the
    // alignment (-x) at the traffic coming up it, its back 37.5 mm in
    // front of the spot (x = 49.9625) and 3 mm thick (to 49.9595), across
    // y 4 -+ 0.45, from 1.5 m above the ground to 2.1 m.
    Site site;
    const std::string reply = site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    EXPECT_TRUE(contains(reply, "added id=sign-1 kind=sign placement=alignment")) << reply;
    const BimModel model = site.built("sign-1");
    ASSERT_TRUE(model.problems.empty());
    ASSERT_EQ(model.objects.size(), 1u);
    const BimObject& sign = model.objects.front();
    EXPECT_EQ(sign.id, "object/sign-1/1");
    EXPECT_EQ(sign.ifcClass, "IfcSign");
    EXPECT_EQ(sign.predefinedType, "PICTORAL");
    EXPECT_EQ(sign.layer, "road/objects/sign");
    EXPECT_TRUE(isClosedMesh(sign.mesh));
    const auto box = sign.mesh.bounds();
    EXPECT_NEAR(box.min.x, 49.9595, 1e-9);
    EXPECT_NEAR(box.max.x, 50.0375, 1e-9);
    EXPECT_NEAR(box.min.y, 3.55, 1e-9);
    EXPECT_NEAR(box.max.y, 4.45, 1e-9);
    EXPECT_NEAR(box.min.z, 10.0, 1e-9);
    EXPECT_NEAR(box.max.z, 12.1, 1e-9);
    EXPECT_NEAR(quantityValue(sign, "Height"), 2.1, 1e-12);
    EXPECT_NEAR(quantityValue(sign, "GrossArea"), 0.9 * 0.6, 1e-12);
    EXPECT_EQ(quantityValue(sign, "Count"), 1.0);
}

TEST(RoadObjects, AnObjectOnAGradedProfileTakesItsStationsLevelAndTheCrossfall)
{
    // 10 rising 2 over 100: 11 at station 50; 4 m out at -3 percent falls
    // 0.12 to 10.88.
    Site site(true, 2.0);
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 crossfall=-3");
    const BimModel model = site.built("sign-1");
    ASSERT_EQ(model.objects.size(), 1u);
    EXPECT_NEAR(model.objects.front().mesh.bounds().min.z, 10.88, 1e-9);
}

TEST(RoadObjects, ABarrierRunsItsStationsWithAPostEverySpacingAndAnEndPost)
{
    // 10 to 60 is 50 m; posts every 1.905 m: 26 whole spacings make 27
    // posts to 49.53, and the end post at 50 makes 28. Left of the
    // centreline at 3, its traffic is on its right, so the 83 mm rail is on
    // the face of the 150 mm posts towards y = 0: y 2.842 to 2.925, the
    // posts 2.925 to 3.075; the top of rail 787 mm above level 10.
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=60 offset=3");
    const BimModel model = site.built("barrier-1");
    ASSERT_EQ(model.objects.size(), 1u);
    const BimObject& barrier = model.objects.front();
    EXPECT_EQ(barrier.id, "object/barrier-1");
    EXPECT_EQ(barrier.ifcClass, "IfcRailing");
    EXPECT_EQ(barrier.predefinedType, "GUARDRAIL");
    EXPECT_TRUE(isClosedMesh(barrier.mesh));
    EXPECT_NEAR(quantityValue(barrier, "Length"), 50.0, 1e-9);
    EXPECT_EQ(quantityValue(barrier, "Count"), 28.0);
    EXPECT_NEAR(quantityValue(barrier, "Height"), 0.787, 1e-12);
    const auto box = barrier.mesh.bounds();
    EXPECT_NEAR(box.min.y, 3.0 - 0.075 - 0.083, 1e-9);
    EXPECT_NEAR(box.max.y, 3.0 + 0.075, 1e-9);
    EXPECT_NEAR(box.max.z, 10.787, 1e-9);
    ASSERT_EQ(barrier.plan.size(), 1u);
    EXPECT_EQ(barrier.plan.front().vertices.front(), Point2(10, 3));
    EXPECT_EQ(barrier.plan.front().vertices.back(), Point2(60, 3));
}

TEST(RoadObjects, ASingleSlopeBarriersVolumeIsItsSectionTimesItsLength)
{
    // A trapezoid 0.610 at the base, 0.203 at the top, 1.067 tall:
    // (0.610 + 0.203) / 2 x 1.067 = 0.4337355 m2, over 20 m 8.67471 m3.
    Site site;
    site.ok("ROADOBJECT ADD barrier type=singleslope ALIGNMENT Main station=0 to=20 offset=-2");
    const BimModel model = site.built("barrier-1");
    ASSERT_EQ(model.objects.size(), 1u);
    const BimObject& barrier = model.objects.front();
    EXPECT_TRUE(isClosedMesh(barrier.mesh));
    EXPECT_EQ(barrier.material, "concrete");
    EXPECT_NEAR(quantityValue(barrier, "GrossVolume"), 8.67471, 1e-9);
    EXPECT_EQ(quantity(barrier, "Count"), nullptr) << "a concrete barrier has no posts";
}

TEST(RoadObjects, AnFShapeBarrierHasThePublishedFaceAngles)
{
    // Worked from the shape's definition, not the code's: a 76 mm reveal,
    // 55 degrees from the horizontal to 254 mm, 84 degrees to the top at
    // 813 mm, on a 610 mm base. Half-widths: 0.305 at the foot, at the
    // knee 0.305 - 0.178 / tan 55, at the top that less 0.559 / tan 84.
    const double pi = std::numbers::pi;
    const double base = 0.305;
    const double knee = base - (0.254 - 0.076) / std::tan(55.0 * pi / 180.0);
    const double top = knee - (0.813 - 0.254) / std::tan(84.0 * pi / 180.0);
    const double area = 2.0 * (base * 0.076 + (base + knee) / 2.0 * (0.254 - 0.076) +
                               (knee + top) / 2.0 * (0.813 - 0.254));
    Site site;
    site.ok("ROADOBJECT ADD barrier type=fshape ALIGNMENT Main station=30 to=40");
    const BimModel model = site.built("barrier-1");
    ASSERT_EQ(model.objects.size(), 1u);
    const BimObject& barrier = model.objects.front();
    EXPECT_TRUE(isClosedMesh(barrier.mesh));
    EXPECT_NEAR(quantityValue(barrier, "GrossVolume"), area * 10.0, 1e-9);
    // Centred on the alignment: its foot is 0.305 either side.
    const auto box = barrier.mesh.bounds();
    EXPECT_NEAR(box.max.y, base, 1e-9);
    EXPECT_NEAR(box.max.z, 10.813, 1e-9);
}

TEST(RoadObjects, ARetainingWallStandsOnItsStripFootingAndItsFaceIsLengthTimesHeight)
{
    // 30 m of the default: a 2.0 m stem 0.25 thick, 0.25 x 2.0 x 30 = 15 m3,
    // its face 30 x 2 = 60 m2; and below the ground its 1.2 x 0.4 footing,
    // an IfcFooting of its own, 1.2 x 0.4 x 30 = 14.4 m3.
    Site site;
    site.ok("ROADOBJECT ADD wall ALIGNMENT Main station=0 to=30 offset=-5");
    const BimModel model = site.built("wall-1");
    ASSERT_EQ(model.objects.size(), 2u);
    const BimObject& wall = model.objects[0];
    EXPECT_EQ(wall.id, "object/wall-1");
    EXPECT_EQ(wall.ifcClass, "IfcWall");
    EXPECT_EQ(wall.predefinedType, "RETAININGWALL");
    EXPECT_TRUE(isClosedMesh(wall.mesh));
    EXPECT_NEAR(quantityValue(wall, "Length"), 30.0, 1e-9);
    EXPECT_NEAR(quantityValue(wall, "GrossArea"), 60.0, 1e-9);
    EXPECT_NEAR(quantityValue(wall, "GrossVolume"), 15.0, 1e-9);
    EXPECT_NEAR(wall.mesh.bounds().min.z, 10.0, 1e-9);
    const BimObject& footing = model.objects[1];
    EXPECT_EQ(footing.id, "object/wall-1/footing");
    EXPECT_EQ(footing.ifcClass, "IfcFooting");
    EXPECT_EQ(footing.predefinedType, "STRIP_FOOTING");
    EXPECT_TRUE(isClosedMesh(footing.mesh));
    EXPECT_NEAR(quantityValue(footing, "GrossVolume"), 14.4, 1e-9);
    EXPECT_NEAR(footing.mesh.bounds().min.z, 9.6, 1e-9);
    EXPECT_NEAR(footing.mesh.bounds().max.z, 10.0, 1e-9);
    // Of the site, as no road was named: no part of one.
    EXPECT_TRUE(wall.facility.empty());
    EXPECT_TRUE(wall.part.empty());
}

TEST(RoadObjects, ANoiseWallIsAUserDefinedWallSaidToBeANoiseBarrierWithNoFooting)
{
    Site site;
    site.ok("ROADOBJECT ADD wall type=noise facility=\"Main Road\" ALIGNMENT Main station=0 "
            "to=10 offset=8");
    const BimModel model = site.built("wall-1");
    ASSERT_EQ(model.objects.size(), 1u);
    const BimObject& wall = model.objects.front();
    EXPECT_EQ(wall.predefinedType, "USERDEFINED");
    const std::pair<std::string, std::string> noise{"ObjectType", "NOISEBARRIER"};
    EXPECT_NE(std::ranges::find(wall.properties, noise), wall.properties.end());
    // A part of the road named.
    EXPECT_EQ(wall.facility, "Main Road");
    EXPECT_EQ(wall.part, "ROADSIDE");
    // 4.0 x 0.2 x 10 = 8 m3, standing on the ground.
    EXPECT_NEAR(quantityValue(wall, "GrossVolume"), 8.0, 1e-9);
    EXPECT_NEAR(wall.mesh.bounds().min.z, 10.0, 1e-9);
}

TEST(RoadObjects, ACulvertSkewedThirtyDegreesHasItsEndsWhereTheSkewPutsThem)
{
    // Across Main at station 50, 20 m long, turned 30 degrees from square:
    // its axis is the left normal (0,1) turned 30 degrees counter-clockwise,
    // (-sin 30, cos 30) = (-0.5, 0.8660254), so the inlet is 10 m back along
    // it at (55, -8.660254) and the outlet at (45, 8.660254). The default
    // 900 mm pipe with a 100 mm wall under 600 mm of cover on level 10: the
    // invert is 10 - 0.6 - 0.1 - 0.9 = 8.4 at both ends.
    Site site;
    site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=50 skew=30");
    const RoadObject* culvert = site.document.roadObjects().find("culvert-1");
    ASSERT_NE(culvert, nullptr);
    const auto ends = culvertEnds(placementContext(site.document), *culvert);
    ASSERT_TRUE(ends.ok()) << ends.error().describe();
    const double half = 10.0 * std::cos(std::numbers::pi / 6.0);
    EXPECT_NEAR(ends->inlet.x, 55.0, 1e-9);
    EXPECT_NEAR(ends->inlet.y, -half, 1e-9);
    EXPECT_NEAR(ends->inlet.z, 8.4, 1e-9);
    EXPECT_NEAR(ends->outlet.x, 45.0, 1e-9);
    EXPECT_NEAR(ends->outlet.y, half, 1e-9);
    EXPECT_NEAR(ends->outlet.z, 8.4, 1e-9);

    const BimModel model = site.built("culvert-1");
    ASSERT_EQ(model.objects.size(), 3u) << "the barrel and two headwalls";
    const BimObject& barrel = model.objects[0];
    EXPECT_EQ(barrel.id, "object/culvert-1/barrel");
    EXPECT_EQ(barrel.ifcClass, "IfcPipeSegment");
    EXPECT_EQ(barrel.predefinedType, "CULVERT");
    EXPECT_TRUE(isClosedMesh(barrel.mesh));
    EXPECT_NEAR(quantityValue(barrel, "Length"), 20.0, 1e-9);
    // The ring between two 24-gons of circumradius 0.55 and 0.45:
    // 24/2 x sin 15 deg x (0.55^2 - 0.45^2), over 20 m.
    const double ring = 12.0 * std::sin(std::numbers::pi / 12.0) * (0.55 * 0.55 - 0.45 * 0.45);
    EXPECT_NEAR(quantityValue(barrel, "GrossVolume"), ring * 20.0, 1e-9);
    EXPECT_EQ(model.objects[1].id, "object/culvert-1/headwall/inlet");
    EXPECT_EQ(model.objects[2].id, "object/culvert-1/headwall/outlet");
    for (const BimObject& headwall : {model.objects[1], model.objects[2]}) {
        EXPECT_TRUE(isClosedMesh(headwall.mesh)) << headwall.id;
        // 0.25 thick, the barrel's 1.1 and 0.3 either side wide, 1.1 and
        // 0.3 above tall: 0.25 x 1.7 x 1.4.
        EXPECT_NEAR(quantityValue(headwall, "GrossVolume"), 0.25 * 1.7 * 1.4, 1e-9);
        EXPECT_NEAR(headwall.mesh.bounds().min.z, 8.3, 1e-9);
    }
}

TEST(RoadObjects, ACulvertAtTwoPointsRunsFromTheFirstToTheSecondAtTheirInverts)
{
    Site site;
    site.ok("ROADOBJECT ADD culvert AT 0,20,5 10,20,4.5 headwalls=off");
    const BimModel model = site.built("culvert-1");
    ASSERT_EQ(model.objects.size(), 1u);
    // The 3D length of a barrel falling 0.5 over 10: sqrt(100.25).
    EXPECT_NEAR(quantityValue(model.objects.front(), "Length"), std::sqrt(100.25), 1e-12);
    const RoadObject* culvert = site.document.roadObjects().find("culvert-1");
    ASSERT_NE(culvert, nullptr);
    const auto ends = culvertEnds(placementContext(site.document), *culvert);
    ASSERT_TRUE(ends.ok());
    EXPECT_EQ(ends->inlet.z, 5.0);
    EXPECT_EQ(ends->outlet.z, 4.5);
}

TEST(RoadObjects, ACulvertAtOnePointIsRefused)
{
    Site site;
    const auto error = site.fails("ROADOBJECT ADD culvert AT 0,20,5");
    EXPECT_TRUE(contains(error.message, "two")) << error.describe();
    EXPECT_TRUE(site.document.roadObjects().objects.empty());
}

TEST(RoadObjects, GuidePostsAtASpacingStandOnlyAtWholeSpacings)
{
    // 0 to 25 every 10: at 0, 10 and 20, not at the end.
    Site site;
    site.ok("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=25 spacing=10 offset=-4");
    const BimModel model = site.built("guidepost-1");
    ASSERT_EQ(model.objects.size(), 3u);
    for (std::size_t i = 0; i < 3; ++i) {
        const BimObject& post = model.objects[i];
        EXPECT_EQ(post.id, "object/guidepost-1/" + std::to_string(i + 1));
        EXPECT_TRUE(isClosedMesh(post.mesh));
        const auto box = post.mesh.bounds();
        EXPECT_NEAR((box.min.x + box.max.x) / 2.0, 10.0 * static_cast<double>(i), 1e-9);
        EXPECT_NEAR((box.min.y + box.max.y) / 2.0, -4.0, 1e-9);
        EXPECT_NEAR(box.max.z - box.min.z, 1.0, 1e-12);
    }
}

TEST(RoadObjects, ALightPolesArmReachesTowardsTheCentreline)
{
    // At (20, 6), left of the centreline: the 1.5 m arm reaches to (20, 4.5)
    // and the 600 x 300 x 150 lantern hangs under its 80 mm arm, from
    // 10 + 10 - 0.08 - 0.15 = 19.77 to 19.92.
    Site site;
    site.ok("ROADOBJECT ADD light ALIGNMENT Main station=20 offset=6");
    const BimModel model = site.built("light-1");
    ASSERT_EQ(model.objects.size(), 2u);
    EXPECT_EQ(model.objects[0].id, "object/light-1/1/pole");
    EXPECT_EQ(model.objects[1].id, "object/light-1/1/luminaire");
    EXPECT_EQ(model.objects[1].ifcClass, "IfcLightFixture");
    EXPECT_TRUE(isClosedMesh(model.objects[0].mesh));
    EXPECT_TRUE(isClosedMesh(model.objects[1].mesh));
    EXPECT_NEAR(model.objects[0].mesh.bounds().max.z, 20.0, 1e-9);
    const auto lantern = model.objects[1].mesh.bounds();
    EXPECT_NEAR(lantern.min.x, 19.85, 1e-9);
    EXPECT_NEAR(lantern.max.x, 20.15, 1e-9);
    EXPECT_NEAR(lantern.min.y, 4.2, 1e-9);
    EXPECT_NEAR(lantern.max.y, 4.8, 1e-9);
    EXPECT_NEAR(lantern.min.z, 19.77, 1e-9);
    EXPECT_NEAR(lantern.max.z, 19.92, 1e-9);
}

TEST(RoadObjects, ALaneLinesMarksFollowTheThreeAndNinePattern)
{
    // 100 m from a mark at 0: marks start at 0, 12 ... 96, nine of them, the
    // last 96 to 99: 27 m marked, 100 mm wide, 2.7 m2.
    Site site;
    site.ok("ROADOBJECT ADD marking type=lane ALIGNMENT Main station=0 to=100 offset=1.75");
    const BimModel model = site.built("marking-1");
    ASSERT_EQ(model.objects.size(), 1u);
    const BimObject& line = model.objects.front();
    EXPECT_EQ(line.ifcClass, "IfcSurfaceFeature");
    EXPECT_TRUE(isClosedMesh(line.mesh));
    EXPECT_EQ(quantityValue(line, "Count"), 9.0);
    EXPECT_NEAR(quantityValue(line, "Length"), 27.0, 1e-9);
    EXPECT_NEAR(quantityValue(line, "GrossArea"), 2.7, 1e-9);
    EXPECT_EQ(line.plan.size(), 9u);
}

TEST(RoadObjects, AnEdgeLineIsOneContinuousMark)
{
    Site site;
    site.ok("ROADOBJECT ADD marking ALIGNMENT Main station=0 to=100 offset=-3.5");
    const BimModel model = site.built("marking-1");
    ASSERT_EQ(model.objects.size(), 1u);
    EXPECT_EQ(quantityValue(model.objects.front(), "Count"), 1.0);
    EXPECT_NEAR(quantityValue(model.objects.front(), "GrossArea"), 100.0 * 0.15, 1e-9);
}

TEST(RoadObjects, AFenceRoundAClosedPolylineGoesAllTheWayRoundWithOnePostAtItsStart)
{
    // A 10 m square at level 5.5: 40 m of fence; posts every 3 m at 0 ...
    // 39, and the end post would stand on the first, so 14.
    Site site;
    site.ok("PLINE 0,0 10,0 10,10 0,10 CLOSE");
    site.ok("SELECT ALL");
    site.ok("PROP SET elevation 5.5");
    const std::string reply = site.ok("ROADOBJECT ADD fence ALONG #1");
    EXPECT_TRUE(contains(reply, "placement=line points=5 source=#1 heights=line")) << reply;
    const BimModel model = site.built("fence-1");
    ASSERT_EQ(model.objects.size(), 1u);
    EXPECT_TRUE(isClosedMesh(model.objects.front().mesh));
    EXPECT_NEAR(quantityValue(model.objects.front(), "Length"), 40.0, 1e-9);
    EXPECT_EQ(quantityValue(model.objects.front(), "Count"), 14.0);
    EXPECT_NEAR(model.objects.front().mesh.bounds().min.z, 5.5, 1e-12);
}

TEST(RoadObjects, ASignOnASurfaceStandsOnTheGroundUnderIt)
{
    // On the plane z = 100 + 0.01 x, (50, 4) is at 100.5.
    Site site(false);
    katana::terrain::SurfaceStore store;
    ASSERT_TRUE(store.add({"ground", slopingGround(), "test"}).ok());
    ASSERT_TRUE(site.document.setSurfaces(store).ok());
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 surface=ground");
    const BimModel model = site.built("sign-1");
    ASSERT_EQ(model.objects.size(), 1u);
    EXPECT_NEAR(model.objects.front().mesh.bounds().min.z, 100.5, 1e-9);
}

TEST(RoadObjects, AnAlignmentWithNoProfileRefusesProfileHeights)
{
    Site site(false);
    const auto error = site.fails("ROADOBJECT ADD sign ALIGNMENT Main station=50");
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(error.message, "no design profile")) << error.describe();
}

TEST(RoadObjects, AStationOffTheAlignmentIsRefused)
{
    Site site;
    const auto error = site.fails("ROADOBJECT ADD sign ALIGNMENT Main station=150");
    EXPECT_TRUE(contains(error.message, "off alignment Main")) << error.describe();
    EXPECT_TRUE(site.document.roadObjects().objects.empty());
}

TEST(RoadObjects, APointWithNoHeightIsRefusedRatherThanStoodAtZero)
{
    Site site;
    const auto error = site.fails("ROADOBJECT ADD sign AT 5,5");
    EXPECT_TRUE(contains(error.message, "no height")) << error.describe();
}

TEST(RoadObjects, AnObjectWhoseAlignmentIsGoneIsListedAsAProblemAndNotDrawn)
{
    Site site;
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    site.ok("ALIGN DELETE Main");
    const BimModel model = katana::cad::bim::placedObjects(site.document);
    EXPECT_TRUE(model.objects.empty());
    ASSERT_EQ(model.problems.size(), 1u);
    EXPECT_EQ(model.problems.front().source, "object sign-1");
    EXPECT_TRUE(contains(model.problems.front().message, "alignment Main does not exist"));
    // And the verb says so.
    EXPECT_TRUE(contains(site.ok("ROADOBJECT PROBLEMS"), "problem source=\"object sign-1\""));
}

TEST(RoadObjects, TheSameDefinitionMakesTheSameObjectsWithTheSameIds)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=40 offset=4");
    site.ok("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=40 spacing=20 offset=-4");
    const BimModel first = katana::cad::bim::placedObjects(site.document);
    const BimModel second = katana::cad::bim::placedObjects(site.document);
    ASSERT_EQ(first.objects.size(), 4u);
    ASSERT_EQ(second.objects.size(), first.objects.size());
    for (std::size_t i = 0; i < first.objects.size(); ++i) {
        EXPECT_EQ(first.objects[i].id, second.objects[i].id);
        EXPECT_EQ(first.objects[i].mesh.vertices, second.objects[i].mesh.vertices);
        EXPECT_EQ(first.objects[i].mesh.faces, second.objects[i].mesh.faces);
    }
}

TEST(RoadObjects, IdsAreNeverGivenTwiceEvenAfterADelete)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    site.ok("ROADOBJECT DELETE sign-2");
    const std::string reply = site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=60 offset=4");
    EXPECT_TRUE(contains(reply, "id=sign-3")) << reply;
    site.ok("ROADOBJECT DELETE ALL");
    EXPECT_TRUE(site.document.roadObjects().objects.empty());
    EXPECT_EQ(site.document.roadObjects().nextNumber, 4u);
}

TEST(RoadObjects, TheJsonGivesBackTheSameSet)
{
    RoadObjectSet set;
    set.nextNumber = 9;
    RoadObject barrier;
    barrier.id = "barrier-1";
    barrier.kind = RoadObjectKind::Barrier;
    barrier.name = "Bridge approach, east";
    barrier.layer = "road/safety";
    barrier.colour = katana::entity::Color{10, 20, 30, 255};
    barrier.material = "galvanised steel";
    barrier.facility = "Main Road";
    barrier.placement.alignment = "Main";
    barrier.placement.station = 12.345678901234567;
    barrier.placement.to = 98.7;
    barrier.placement.offset = -3.1;
    barrier.placement.crossfall = -2.5;
    barrier.placement.dz = 0.15;
    barrier.parameters["type"] = std::string("thriebeam");
    barrier.parameters["height"] = 0.9;
    RoadObject fence;
    fence.id = "fence-4";
    fence.kind = RoadObjectKind::Fence;
    fence.placement.kind = PlacementKind::Line;
    fence.placement.vertices = {PlacedVertex{Point2(1.5, 2.5), 3.25}, PlacedVertex{Point2(4, 5)}};
    fence.placement.source = 17;
    fence.placement.heights = katana::cad::road::HeightSource::Surface;
    fence.placement.surface = "ground";
    RoadObject sign;
    sign.id = "sign-8";
    sign.kind = RoadObjectKind::Sign;
    sign.placement.kind = PlacementKind::Points;
    sign.placement.vertices = {PlacedVertex{Point2(0, 0), 1.0}};
    sign.placement.spacing = 5.0;
    sign.parameters["code"] = std::string("R1-1 \"stop\"");
    set.objects = {barrier, fence, sign};

    const auto json = katana::cad::road::roadObjectsToJson(set);
    ASSERT_TRUE(json.ok()) << json.error().describe();
    const auto back = katana::cad::road::roadObjectsFromJson(*json);
    ASSERT_TRUE(back.ok()) << back.error().describe() << "\n" << *json;
    EXPECT_EQ(*back, set) << *json;
}

TEST(RoadObjects, ANewerVersionIsRefusedAsUnsupportedAndAnotherFormatAsNotOurs)
{
    const auto newer = katana::cad::road::roadObjectsFromJson(
        R"({"format": "katana-road-objects", "version": 2, "objects": []})");
    ASSERT_FALSE(newer.ok());
    EXPECT_EQ(newer.error().code, ErrorCode::Unsupported);
    const auto other = katana::cad::road::roadObjectsFromJson(R"({"format": "katana-sheets"})");
    ASSERT_FALSE(other.ok());
    EXPECT_EQ(other.error().code, ErrorCode::ParseFailure);
    // A value out of range in the file is refused as a typed one would be.
    const auto edited = katana::cad::road::roadObjectsFromJson(
        R"({"format": "katana-road-objects", "version": 1, "objects": [{"id": "barrier-1",
           "kind": "barrier", "placement": {"kind": "alignment", "alignment": "Main",
           "station": 0, "to": 10}, "parameters": {"height": 9}}]})");
    ASSERT_FALSE(edited.ok());
    EXPECT_EQ(edited.error().code, ErrorCode::ParseFailure);
}

TEST(RoadObjects, AParameterIsReadByItsKindsRules)
{
    EXPECT_TRUE(parseParameter(RoadObjectKind::Barrier, "height", "0.9").ok());
    const auto high = parseParameter(RoadObjectKind::Barrier, "height", "5");
    ASSERT_FALSE(high.ok());
    EXPECT_EQ(high.error().code, ErrorCode::InvalidArgument);
    const auto word = parseParameter(RoadObjectKind::Barrier, "height", "tall");
    ASSERT_FALSE(word.ok());
    EXPECT_EQ(word.error().code, ErrorCode::ParseFailure);
    const auto half = parseParameter(RoadObjectKind::Barrier, "ropes", "2.5");
    ASSERT_FALSE(half.ok());
    EXPECT_TRUE(contains(half.error().message, "whole number"));
    const auto choice = parseParameter(RoadObjectKind::Barrier, "type", "WBEAM");
    ASSERT_TRUE(choice.ok());
    EXPECT_EQ(std::get<std::string>(*choice), "wbeam");
    const auto unknown = parseParameter(RoadObjectKind::Barrier, "diameter", "1");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);
}

TEST(RoadObjects, ChangingABarriersTypeGivesTheNewTypesDimensions)
{
    RoadObject barrier;
    barrier.kind = RoadObjectKind::Barrier;
    EXPECT_EQ(std::get<double>(effectiveParameters(barrier).at("height")), 0.787);
    EXPECT_FALSE(effectiveParameters(barrier).contains("width")) << "a W-beam has no base width";
    ASSERT_TRUE(katana::cad::road::applyKey(barrier, "type", "singleslope").ok());
    const auto made = effectiveParameters(barrier);
    EXPECT_EQ(std::get<double>(made.at("height")), 1.067);
    EXPECT_EQ(std::get<double>(made.at("width")), 0.610);
    EXPECT_EQ(std::get<double>(made.at("topwidth")), 0.203);
    // A key given again empty goes back to the default.
    ASSERT_TRUE(katana::cad::road::applyKey(barrier, "height", "1.2").ok());
    EXPECT_EQ(std::get<double>(effectiveParameters(barrier).at("height")), 1.2);
    ASSERT_TRUE(katana::cad::road::applyKey(barrier, "height", "").ok());
    EXPECT_EQ(std::get<double>(effectiveParameters(barrier).at("height")), 1.067);
}

TEST(RoadObjects, AnAlignmentKeyOnAnObjectPlacedAlongALineIsRefused)
{
    RoadObject fence;
    fence.kind = RoadObjectKind::Fence;
    fence.placement.kind = PlacementKind::Line;
    const auto refused = katana::cad::road::applyKey(fence, "station", "10");
    ASSERT_FALSE(refused.ok());
    EXPECT_TRUE(contains(refused.error().message, "along an alignment"));
    EXPECT_FALSE(katana::cad::road::applyKey(fence, "colour", "not a colour").ok());
    EXPECT_TRUE(katana::cad::road::applyKey(fence, "colour", "red").ok());
    EXPECT_FALSE(katana::cad::road::applyKey(fence, "layer", "a//b").ok());
}
