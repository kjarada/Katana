// The road model as the IFC export takes it (src/katana_app/ifc_verbs.hpp,
// roadModelInput; docs/ifc.md, "Road model"): a cad::bim::BimModel made by
// hand - the corridor and road-object builders are other modules' and are
// not needed to judge the conversion - turned into the ifc layer's plain
// data field by field, and written. Both front ends call this one function
// (katana_cli's EXPORT and the window's), so a corridor's elements are the
// same objects whichever wrote the file.

#include <algorithm>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "ifc_verbs.hpp"
#include "katana/cad/bim/bim_object.hpp"
#include "katana/ifc/export.hpp"

namespace {

using katana::app::roadModelInput;
namespace bim = katana::cad::bim;

// A 10 x 1 x 0.5 slab: eight corners, two triangles a side, wound outwards.
katana::geometry::TriangleMesh slab()
{
    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {{0.0, 0.0, 0.0},  {10.0, 0.0, 0.0}, {10.0, 1.0, 0.0},  {0.0, 1.0, 0.0},
                     {0.0, 0.0, 0.5},  {10.0, 0.0, 0.5}, {10.0, 1.0, 0.5},  {0.0, 1.0, 0.5}};
    mesh.faces = {{0, 2, 1}, {0, 3, 2}, {4, 5, 6}, {4, 6, 7}, {0, 1, 5}, {0, 5, 4},
                  {3, 7, 6}, {3, 6, 2}, {0, 4, 7}, {0, 7, 3}, {1, 2, 6}, {1, 6, 5}};
    return mesh;
}

bim::BimObject course()
{
    bim::BimObject object;
    object.id = "corridor/Main/course/AC14/left/1";
    object.name = "AC14 wearing course";
    object.kind = "pavement";
    object.facility = "Main";
    object.part = "CARRIAGEWAY";
    object.ifcClass = "IfcCourse";
    object.predefinedType = "PAVEMENT";
    object.material = "Asphalt AC14";
    object.colour = katana::entity::Color{30, 30, 30, 255};
    object.mesh = slab();
    object.plan = {katana::geometry::Polyline2{{{0.0, 0.0}, {10.0, 0.0}, {10.0, 1.0}}, false}};
    object.quantities = {{"GrossVolume", 5.0, "m3"}, {"Length", 10.0, "m"}};
    object.properties = {{"Component", "lane"}, {"Thickness", "0.050"}};
    object.layer = "Road/Main/Pavement";
    return object;
}

} // namespace

TEST(IfcRoadModelInput, EveryFieldOfABimObjectReachesTheElement)
{
    bim::BimModel model;
    model.objects.push_back(course());
    const katana::ifc::RoadModelInput input = roadModelInput(model);
    ASSERT_EQ(input.elements.size(), 1u);
    const katana::ifc::RoadElement& element = input.elements.front();
    const bim::BimObject expected = course();
    EXPECT_EQ(element.id, expected.id);
    EXPECT_EQ(element.name, expected.name);
    EXPECT_EQ(element.kind, expected.kind);
    EXPECT_EQ(element.facility, expected.facility);
    EXPECT_EQ(element.part, expected.part);
    EXPECT_EQ(element.ifcClass, (katana::ifc::IfcClass{"IfcCourse", "PAVEMENT", ""}));
    EXPECT_EQ(element.material, expected.material);
    EXPECT_EQ(element.colour, expected.colour);
    EXPECT_EQ(element.mesh.vertices, expected.mesh.vertices);
    EXPECT_EQ(element.mesh.faces, expected.mesh.faces);
    EXPECT_EQ(element.plan, expected.plan);
    ASSERT_EQ(element.quantities.size(), 2u);
    EXPECT_EQ(element.quantities[0], (katana::ifc::RoadQuantity{"GrossVolume", 5.0, "m3"}));
    EXPECT_EQ(element.quantities[1], (katana::ifc::RoadQuantity{"Length", 10.0, "m"}));
    EXPECT_EQ(element.properties, expected.properties);
    EXPECT_EQ(element.layer, expected.layer);
    EXPECT_TRUE(element.host.empty());
    EXPECT_TRUE(element.ground.empty());
    EXPECT_TRUE(input.problems.empty());
}

// A USERDEFINED class must say what it is (each class's CorrectPredefinedType
// rule): the object's ObjectType property, in any letter case, else its kind
// in capitals. A listed predefined type takes no object type.
TEST(IfcRoadModelInput, AUserDefinedClassIsItsObjectTypePropertyElseItsKindInCapitals)
{
    bim::BimModel model;
    bim::BimObject named = course();
    named.ifcClass = "IfcWall";
    named.predefinedType = "USERDEFINED";
    named.kind = "wall";
    named.properties.push_back({"objecttype", "NOISEBARRIER"});
    bim::BimObject unnamed = named;
    unnamed.id = "object/wall-2";
    unnamed.properties.pop_back();
    bim::BimObject listed = course();
    listed.properties.push_back({"ObjectType", "ignored"});
    model.objects = {named, unnamed, listed};
    const auto input = roadModelInput(model);
    ASSERT_EQ(input.elements.size(), 3u);
    EXPECT_EQ(input.elements[0].ifcClass.objectType, "NOISEBARRIER");
    EXPECT_EQ(input.elements[1].ifcClass.objectType, "WALL");
    EXPECT_EQ(input.elements[2].ifcClass.objectType, "");
}

// A feature's host - the course a marking is painted on - and the surface a
// cut is taken from are properties of the object, read in any letter case;
// "Surface" and "Target Surface" name the ground as "Ground" does, the first
// of them given winning.
TEST(IfcRoadModelInput, AFeaturesHostAndGroundAreItsPropertiesInAnyLetterCase)
{
    bim::BimModel model;
    bim::BimObject marking = course();
    marking.ifcClass = "IfcSurfaceFeature";
    marking.predefinedType = "LINEMARKING";
    marking.properties = {{"HOST", "corridor/Main/course/AC14/left/1"}};
    bim::BimObject cut = course();
    cut.ifcClass = "IfcEarthworksCut";
    cut.predefinedType = "CUT";
    cut.properties = {{"target surface", "existing"}, {"Ground", "later"}};
    model.objects = {marking, cut};
    const auto input = roadModelInput(model);
    ASSERT_EQ(input.elements.size(), 2u);
    EXPECT_EQ(input.elements[0].host, "corridor/Main/course/AC14/left/1");
    EXPECT_TRUE(input.elements[0].ground.empty());
    EXPECT_EQ(input.elements[1].ground, "existing");
    // They are properties of the object still, written as it has them.
    EXPECT_EQ(input.elements[1].properties, cut.properties);
}

// What the model could not build is said where it is, so the export's
// warnings name the corridor or object.
TEST(IfcRoadModelInput, TheModelsProblemsSayWhereTheyAre)
{
    bim::BimModel model;
    model.problems = {{"corridor Main", "the alignment has no profile"},
                      {"", "no template is defined"}};
    const auto input = roadModelInput(model);
    EXPECT_EQ(input.problems, (std::vector<std::string>{"corridor Main: the alignment has no profile",
                                                        "no template is defined"}));
    EXPECT_TRUE(input.elements.empty());
}

// Converted and written: the road, its part and the course, as the export
// writes any road model; the model's problem among the warnings.
TEST(IfcRoadModelInput, AConvertedModelIsWrittenAsARoadItsPartAndItsElement)
{
    bim::BimModel model;
    model.objects.push_back(course());
    model.problems = {{"corridor Main", "the profile ends at 8"}};
    katana::ifc::ExportInput input;
    input.road = roadModelInput(model);
    katana::ifc::ExportOptions options;
    options.projectName = "Test";
    const auto written = katana::ifc::writeIfc(input, options);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->roads, 1u);
    EXPECT_EQ(written->roadParts, 1u);
    EXPECT_EQ(written->roadElements, 1u);
    EXPECT_EQ(written->classes.at("IfcCourse"), 1u);
    EXPECT_NE(written->text.find("=IFCROADPART("), std::string::npos);
    EXPECT_NE(written->text.find("'Main CARRIAGEWAY'"), std::string::npos);
    EXPECT_TRUE(std::find(written->warnings.begin(), written->warnings.end(),
                          "the road model: corridor Main: the profile ends at 8") !=
                written->warnings.end());
}
