// The road model in IFC 4.3 (src/katana_ifc/road.cpp, docs/ifc.md "Road
// model"): a synthetic model - two pavement courses, a kerb, a fill and a cut
// batter, a line marking, a sign and a noise wall, each a box whose volume is
// worked out by hand - exported, and the file read back instance by instance.
// Nothing here waits on the corridor and road-object builders: the input is
// the plain data katana_app makes of them (RoadModelInput).

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <map>
#include <set>
#include <string>
#include <vector>

#include "katana/ifc/classification.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/step.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace ifc = katana::ifc;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Polyline2;
using katana::geometry::TriangleMesh;

namespace {

// ---- the input ------------------------------------------------------------------------

// The box [x0, x1] x [y0, y1] x [z0, z1] as twelve triangles wound outwards:
// each face's (b - a) x (c - a) points away from the box (bottom -z, top +z,
// front -y, back +y, left -x, right +x - each worked through by hand), and
// each edge is used once in each direction.
TriangleMesh box(double x0, double y0, double z0, double x1, double y1, double z1)
{
    TriangleMesh mesh;
    mesh.vertices = {{x0, y0, z0}, {x1, y0, z0}, {x1, y1, z0}, {x0, y1, z0},
                     {x0, y0, z1}, {x1, y0, z1}, {x1, y1, z1}, {x0, y1, z1}};
    mesh.faces = {{0, 2, 1}, {0, 3, 2}, {4, 5, 6}, {4, 6, 7}, {0, 1, 5}, {0, 5, 4},
                  {3, 7, 6}, {3, 6, 2}, {0, 4, 7}, {0, 7, 3}, {1, 2, 6}, {1, 6, 5}};
    return mesh;
}

ifc::RoadElement element(std::string id, std::string kind, ifc::IfcClass ifcClass,
                         std::string facility, std::string part, TriangleMesh mesh)
{
    ifc::RoadElement out;
    out.id = std::move(id);
    out.name = out.id;
    out.kind = std::move(kind);
    out.ifcClass = std::move(ifcClass);
    out.facility = std::move(facility);
    out.part = std::move(part);
    out.mesh = std::move(mesh);
    out.layer = "Road/" + out.kind;
    return out;
}

const std::string kWearing = "corridor/Main/course/AC14/left/1";
const std::string kBase = "corridor/Main/course/BASE/left/1";
const std::string kKerb = "corridor/Main/kerb/left/1";
const std::string kFill = "corridor/Main/batter/fill/left/1";
const std::string kCut = "corridor/Main/batter/cut/right/1";
const std::string kMarking = "object/marking-1";
const std::string kSign = "object/sign-1";
const std::string kWall = "object/wall-1";

// One corridor, Main, 100 m long from x = 1000: a 3.5 m lane of a 50 mm
// wearing course (100 x 3.5 x 0.05 = 17.5 m3) on a 200 mm base (70 m3), a
// kerb in the road itself, a fill batter and a cut batter at the roadside, a
// line marking on the lane, a noise wall; and a sign of the site.
ifc::RoadModelInput roadModel()
{
    ifc::RoadModelInput road;
    auto wearing = element(kWearing, "pavement", {"IfcCourse", "PAVEMENT", {}}, "Main",
                           "CARRIAGEWAY", box(1000.0, 2000.0, 9.95, 1100.0, 2003.5, 10.0));
    wearing.material = "Asphalt AC14";
    wearing.colour = katana::entity::Color{40, 40, 40, 255};
    wearing.quantities = {{"Length", 100.0, "m"},
                          {"Width", 3.5, "m"},
                          {"Thickness", 0.05, "m"},
                          {"GrossVolume", 17.5, "m3"},
                          {"Area", 350.0, "m2"}};
    wearing.properties = {{"Component", "lane"}, {"Stations", "0 to 100"}};
    road.elements.push_back(wearing);

    auto base = element(kBase, "pavement", {"IfcCourse", "PAVEMENT", {}}, "Main", "CARRIAGEWAY",
                        box(1000.0, 2000.0, 9.75, 1100.0, 2003.5, 9.95));
    base.material = "Crushed rock";
    base.quantities = {{"GrossVolume", 70.0, "m3"}};
    road.elements.push_back(base);

    auto kerb = element(kKerb, "kerb", {"IfcKerb", "NOTDEFINED", {}}, "Main", "",
                        box(1000.0, 2003.5, 9.85, 1100.0, 2003.65, 10.15));
    kerb.material = "Concrete";
    kerb.quantities = {{"Length", 100.0, "m"}, {"Weight", 1.5, "t"}};
    road.elements.push_back(kerb);

    auto fill = element(kFill, "batter", {"IfcEarthworksFill", "SLOPEFILL", {}}, "Main",
                        "ROADSIDE", box(1000.0, 2003.65, 9.0, 1100.0, 2006.0, 9.75));
    fill.quantities = {{"CompactedVolume", 12.5, "m3"}};
    road.elements.push_back(fill);

    auto cut = element(kCut, "batter", {"IfcEarthworksCut", "CUT", {}}, "Main", "ROADSIDE",
                       box(1000.0, 1997.0, 9.75, 1100.0, 2000.0, 10.5));
    cut.ground = "ground";
    cut.material = "Clay"; // what it removes: a void takes no material
    cut.quantities = {{"UndisturbedVolume", 20.0, "m3"}};
    road.elements.push_back(cut);

    auto marking = element(kMarking, "marking", {"IfcSurfaceFeature", "LINEMARKING", {}}, "Main",
                           "CARRIAGEWAY", box(1000.0, 2001.7, 10.0, 1100.0, 2001.8, 10.003));
    road.elements.push_back(marking);

    auto sign = element(kSign, "sign", {"IfcSign", "PICTORAL", {}}, "", "",
                        box(1050.0, 2008.0, 10.0, 1050.6, 2008.05, 12.1));
    sign.quantities = {{"Height", 0.9, "m"}, {"Width", 0.6, "m"}, {"Count", 1.0, ""}};
    road.elements.push_back(sign);

    auto wall = element(kWall, "wall", {"IfcWall", "USERDEFINED", "NOISEBARRIER"}, "Main",
                        "ROADSIDE", box(1000.0, 2009.0, 10.0, 1100.0, 2009.2, 13.0));
    road.elements.push_back(wall);

    road.problems = {"corridor Main: the profile ends at 100"};
    return road;
}

katana::terrain::TinSurface ground()
{
    auto surface = katana::terrain::TinSurface::create(
        {{990.0, 1990.0, 10.0}, {1110.0, 1990.0, 10.0}, {990.0, 2020.0, 10.0},
         {1110.0, 2020.0, 10.0}},
        {{0, 1, 3}, {0, 3, 2}});
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : katana::terrain::TinSurface{};
}

ifc::ExportOptions options()
{
    ifc::ExportOptions out;
    out.projectName = "Test";
    return out;
}

ifc::IfcExport exported(const ifc::ExportInput& input, const ifc::ExportOptions& with = options())
{
    auto result = ifc::writeIfc(input, with);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : ifc::IfcExport{};
}

ifc::ExportInput roadInput(const katana::terrain::TinSurface* surface = nullptr)
{
    ifc::ExportInput input;
    if (surface != nullptr) {
        input.surfaces.push_back({"ground", surface});
    }
    input.road = roadModel();
    return input;
}

// ---- the file, read back ------------------------------------------------------------

// The top-level arguments of an instance's attribute list, as written.
std::vector<std::string> argumentsOf(std::string_view list)
{
    std::vector<std::string> out;
    std::string current;
    int depth = 0;
    bool quoted = false;
    for (const char c : list) {
        if (c == '\'') {
            quoted = !quoted;
        } else if (!quoted && c == '(') {
            ++depth;
        } else if (!quoted && c == ')') {
            --depth;
        } else if (!quoted && depth == 0 && c == ',') {
            out.push_back(current);
            current.clear();
            continue;
        }
        current.push_back(c);
    }
    out.push_back(current);
    return out;
}

struct Instance {
    std::string type;
    std::vector<std::string> args;
    std::string line;
};

using Instances = std::map<int, Instance>;

Instances instancesIn(const std::string& text)
{
    Instances out;
    std::size_t at = 0;
    while ((at = text.find("\n#", at)) != std::string::npos) {
        const std::size_t end = text.find(";\n", at + 1);
        const std::string line = text.substr(at + 1, end - at - 1);
        at = end;
        const std::size_t equals = line.find('=');
        const std::size_t open = line.find('(', equals);
        Instance instance;
        instance.type = line.substr(equals + 1, open - equals - 1);
        instance.args = argumentsOf(std::string_view(line).substr(open + 1, line.size() - open - 2));
        instance.line = line;
        out.emplace(std::stoi(line.substr(1, equals - 1)), std::move(instance));
    }
    return out;
}

int refOf(const std::string& argument)
{
    return argument.starts_with('#') ? std::stoi(argument.substr(1)) : 0;
}

std::vector<int> refsOf(const std::string& list)
{
    std::vector<int> out;
    for (const std::string& item : argumentsOf(std::string_view(list).substr(1, list.size() - 2))) {
        out.push_back(refOf(item));
    }
    return out;
}

std::vector<int> ofType(const Instances& all, const std::string& type)
{
    std::vector<int> out;
    for (const auto& [id, instance] : all) {
        if (instance.type == type) {
            out.push_back(id);
        }
    }
    return out;
}

// The element whose Tag (its eighth attribute) is `tag`; 0 when none is.
int byTag(const Instances& all, const std::string& tag)
{
    for (const auto& [id, instance] : all) {
        if (instance.args.size() >= 8 && instance.args[7] == "'" + tag + "'") {
            return id;
        }
    }
    return 0;
}

bool contains(const std::vector<int>& ids, int id)
{
    return std::find(ids.begin(), ids.end(), id) != ids.end();
}

// The spatial element `element` is contained in; 0 when none.
int containerOf(const Instances& all, int element)
{
    for (const int rel : ofType(all, "IFCRELCONTAINEDINSPATIALSTRUCTURE")) {
        if (contains(refsOf(all.at(rel).args[4]), element)) {
            return refOf(all.at(rel).args[5]);
        }
    }
    return 0;
}

// What `relating` aggregates, in the relationship's order.
std::vector<int> partsOf(const Instances& all, int relating)
{
    for (const int rel : ofType(all, "IFCRELAGGREGATES")) {
        if (refOf(all.at(rel).args[4]) == relating) {
            return refsOf(all.at(rel).args[5]);
        }
    }
    return {};
}

// The ObjectPlacement's PlacementRelTo of a product.
int placementOf(const Instances& all, int product)
{
    return refOf(all.at(product).args[5]);
}

int relativeTo(const Instances& all, int placement)
{
    return refOf(all.at(placement).args[0]);
}

// The property or quantity sets defining `object`, by name.
std::map<std::string, int> setsOf(const Instances& all, int object)
{
    std::map<std::string, int> out;
    for (const int rel : ofType(all, "IFCRELDEFINESBYPROPERTIES")) {
        if (contains(refsOf(all.at(rel).args[4]), object)) {
            const int set = refOf(all.at(rel).args[5]);
            out.emplace(all.at(set).args[2], set);
        }
    }
    return out;
}

// The quantities of an IfcElementQuantity as written: "IFCQUANTITYLENGTH('Length',$,$,100.,$)".
std::set<std::string> quantitiesOf(const Instances& all, int set)
{
    std::set<std::string> out;
    for (const int quantity : refsOf(all.at(set).args[5])) {
        const std::string& line = all.at(quantity).line;
        out.insert(line.substr(line.find('=') + 1));
    }
    return out;
}

// The face set of a product's Body, and its points as written.
struct FaceSet {
    bool closed = false;
    std::vector<std::array<double, 3>> points;
    std::vector<std::array<int, 3>> triangles;
};

std::vector<double> realsOf(const std::string& list)
{
    std::vector<double> out;
    for (const std::string& item : argumentsOf(std::string_view(list).substr(1, list.size() - 2))) {
        out.push_back(std::stod(item));
    }
    return out;
}

FaceSet bodyOf(const Instances& all, int product)
{
    FaceSet out;
    const int shape = refOf(all.at(product).args[6]);
    for (const int representation : refsOf(all.at(shape).args[2])) {
        if (all.at(representation).args[1] != "'Body'") {
            continue;
        }
        const Instance& faces = all.at(refsOf(all.at(representation).args[3]).front());
        EXPECT_EQ(faces.type, "IFCTRIANGULATEDFACESET");
        out.closed = faces.args[2] == ".T.";
        const Instance& points = all.at(refOf(faces.args[0]));
        const std::string& list = points.args[0];
        for (const std::string& point : argumentsOf(std::string_view(list).substr(1, list.size() - 2))) {
            const auto xyz = realsOf(point);
            out.points.push_back({xyz[0], xyz[1], xyz[2]});
        }
        const std::string& indices = faces.args[3];
        for (const std::string& triangle :
             argumentsOf(std::string_view(indices).substr(1, indices.size() - 2))) {
            const auto abc = realsOf(triangle);
            out.triangles.push_back(
                {static_cast<int>(abc[0]), static_cast<int>(abc[1]), static_cast<int>(abc[2])});
        }
    }
    return out;
}

// The volume a face set encloses, by the divergence theorem: the sum over
// its triangles of a . (b x c) / 6, positive when they are wound outwards.
double volumeOf(const FaceSet& faces)
{
    double volume = 0.0;
    for (const auto& t : faces.triangles) {
        const auto& a = faces.points[static_cast<std::size_t>(t[0] - 1)];
        const auto& b = faces.points[static_cast<std::size_t>(t[1] - 1)];
        const auto& c = faces.points[static_cast<std::size_t>(t[2] - 1)];
        volume += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) +
                  a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    return volume / 6.0;
}

bool warned(const ifc::IfcExport& out, const std::string& words)
{
    return std::any_of(out.warnings.begin(), out.warnings.end(),
                       [&](const std::string& warning) { return warning.find(words) != std::string::npos; });
}

} // namespace

// ---- the spatial structure ---------------------------------------------------------------

// IfcSite -> IfcRoad (ELEMENT) -> IfcRoadPart (PARTIAL), each by IfcRelAggregates,
// the parts in the order the model first names them: CARRIAGEWAY, then ROADSIDE.
TEST(IfcExportRoad, EachFacilityIsAnIfcRoadInTheSiteAndEachPartAnIfcRoadPartInItsRoad)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    EXPECT_EQ(out.roads, 1u);
    EXPECT_EQ(out.roadParts, 2u);
    EXPECT_EQ(out.roadElements, 8u);
    EXPECT_EQ(out.roadElementsSkipped, 0u);
    ASSERT_EQ(ofType(all, "IFCROAD").size(), 1u);
    const int road = ofType(all, "IFCROAD").front();
    EXPECT_EQ(all.at(road).args[2], "'Main'");
    EXPECT_EQ(all.at(road).args[8], ".ELEMENT.");
    EXPECT_EQ(all.at(road).args.size(), 10u); // IfcRoad has ten attributes (7.9.3.3)

    const int site = ofType(all, "IFCSITE").front();
    EXPECT_EQ(partsOf(all, site), std::vector<int>{road});
    const std::vector<int> parts = partsOf(all, road);
    ASSERT_EQ(parts.size(), 2u);
    for (const int part : parts) {
        EXPECT_EQ(all.at(part).type, "IFCROADPART");
        EXPECT_EQ(all.at(part).args.size(), 11u); // IfcRoadPart has eleven (7.9.3.4)
        EXPECT_EQ(all.at(part).args[8], ".PARTIAL.");
        // Both are parts of the cross-section: "lateral" in the
        // enumeration's own definitions (7.9.2.2).
        EXPECT_EQ(all.at(part).args[9], ".LATERAL.");
    }
    EXPECT_EQ(all.at(parts[0]).args[10], ".CARRIAGEWAY.");
    EXPECT_EQ(all.at(parts[0]).args[2], "'Main CARRIAGEWAY'");
    EXPECT_EQ(all.at(parts[1]).args[10], ".ROADSIDE.");
    EXPECT_EQ(out.classes.at("IfcRoad"), 1u);
    EXPECT_EQ(out.classes.at("IfcRoadPart"), 2u);
}

// Each element in its part, in its road when it names no part, in the site
// when it names no road; each placed relative to its container, the part to
// its road and the road to the site (IfcLocalPlacement, 8.7.3.14).
TEST(IfcExportRoad, EveryElementIsContainedInItsPartItsRoadOrTheSiteAndPlacedRelativeToIt)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    const int site = ofType(all, "IFCSITE").front();
    const int road = ofType(all, "IFCROAD").front();
    const std::vector<int> parts = partsOf(all, road);
    ASSERT_EQ(parts.size(), 2u);
    const int carriageway = parts[0];
    const int roadside = parts[1];

    const int wearing = byTag(all, kWearing);
    const int kerb = byTag(all, kKerb);
    const int fill = byTag(all, kFill);
    const int sign = byTag(all, kSign);
    const int wall = byTag(all, kWall);
    ASSERT_TRUE(wearing && kerb && fill && sign && wall);
    EXPECT_EQ(all.at(wearing).type, "IFCCOURSE");
    EXPECT_EQ(all.at(wearing).args[8], ".PAVEMENT.");
    EXPECT_EQ(containerOf(all, wearing), carriageway);
    EXPECT_EQ(containerOf(all, byTag(all, kBase)), carriageway);
    EXPECT_EQ(containerOf(all, kerb), road);
    EXPECT_EQ(containerOf(all, fill), roadside);
    EXPECT_EQ(containerOf(all, wall), roadside);
    EXPECT_EQ(containerOf(all, sign), site);
    // A USERDEFINED wall says what it is (IfcWall's CorrectPredefinedType).
    EXPECT_EQ(all.at(wall).args[4], "'NOISEBARRIER'");
    EXPECT_EQ(all.at(wall).args[8], ".USERDEFINED.");

    EXPECT_EQ(relativeTo(all, placementOf(all, road)), placementOf(all, site));
    EXPECT_EQ(relativeTo(all, placementOf(all, carriageway)), placementOf(all, road));
    EXPECT_EQ(relativeTo(all, placementOf(all, wearing)), placementOf(all, carriageway));
    EXPECT_EQ(relativeTo(all, placementOf(all, kerb)), placementOf(all, road));
    EXPECT_EQ(relativeTo(all, placementOf(all, fill)), placementOf(all, roadside));
}

// ---- the body ------------------------------------------------------------------------------

// A box is 8 points and 12 triangles, closed, enclosing what its sides make:
// the wearing course 100 x 3.5 x 0.05 = 17.5 m3, the kerb 100 x 0.15 x 0.3
// = 4.5 m3.
TEST(IfcExportRoad, ABodyIsAClosedTriangulatedFaceSetEnclosingTheMeshesVolume)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    const FaceSet wearing = bodyOf(all, byTag(all, kWearing));
    EXPECT_TRUE(wearing.closed);
    EXPECT_EQ(wearing.points.size(), 8u);
    EXPECT_EQ(wearing.triangles.size(), 12u);
    // The coordinates are written to the last bit (stepReal), so the only
    // error is the sum's: its terms are products of order 1e3 x 2e3 x 10 =
    // 2e7, whose ulp is 4e-9, and twelve of them cancel to 6 x 17.5.
    EXPECT_NEAR(volumeOf(wearing), 17.5, 1e-6);
    EXPECT_NEAR(volumeOf(bodyOf(all, byTag(all, kKerb))), 100.0 * 0.15 * 0.3, 1e-6);
    // Representation type and context: the Body of a tessellation.
    EXPECT_NE(out.text.find("'Body','Tessellation'"), std::string::npos);
}

// The schema's closed face set has its normals pointing out of the material
// (8.8.3.47): a mesh wound inwards is written wound outwards, so its volume
// is still positive.
TEST(IfcExportRoad, AClosedMeshWoundInwardsIsWrittenWoundOutwards)
{
    ifc::ExportInput input;
    auto inward = element("inward", "pavement", {"IfcCourse", "PAVEMENT", {}}, "Main",
                          "CARRIAGEWAY", box(0.0, 0.0, 0.0, 1.0, 2.0, 3.0));
    for (auto& face : inward.mesh.faces) {
        std::swap(face[1], face[2]);
    }
    input.road.elements.push_back(inward);
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    const FaceSet body = bodyOf(all, byTag(all, "inward"));
    EXPECT_TRUE(body.closed);
    EXPECT_NEAR(volumeOf(body), 6.0, 1e-12); // 1 x 2 x 3
}

// A face set is Closed only when every edge is used once each way; one
// triangle is not, and is written open and said.
TEST(IfcExportRoad, AnOpenMeshIsWrittenOpenAndSaid)
{
    ifc::ExportInput input;
    TriangleMesh sheet;
    sheet.vertices = {{0.0, 0.0, 0.0}, {1.0, 0.0, 0.0}, {0.0, 1.0, 0.0}};
    sheet.faces = {{0, 1, 2}};
    input.road.elements.push_back(
        element("sheet", "marking", {"IfcCourse", "PAVEMENT", {}}, "Main", "", sheet));
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    const FaceSet body = bodyOf(all, byTag(all, "sheet"));
    EXPECT_FALSE(body.closed);
    EXPECT_EQ(body.triangles.size(), 1u);
    EXPECT_TRUE(warned(out, "1 road elements are not closed solids"));
}

// A mesh that repeats each corner once a face (36 vertices for a box's 12
// triangles) is still the box: points at exactly one place are written once,
// so it is closed; a face naming one place twice is dropped.
TEST(IfcExportRoad, CoincidentVerticesAreWrittenOnceAndADegenerateFaceIsDropped)
{
    const TriangleMesh shared = box(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
    TriangleMesh split;
    for (const auto& face : shared.faces) {
        const auto first = static_cast<std::uint32_t>(split.vertices.size());
        for (const std::uint32_t index : face) {
            split.vertices.push_back(shared.vertices[index]);
        }
        split.faces.push_back({first, first + 1, first + 2});
    }
    split.faces.push_back({0, 0, 1}); // degenerate: names vertex 0 twice
    ASSERT_EQ(split.vertices.size(), 36u);
    ifc::ExportInput input;
    input.road.elements.push_back(
        element("split", "pavement", {"IfcCourse", "PAVEMENT", {}}, "", "", split));
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    const FaceSet body = bodyOf(all, byTag(all, "split"));
    EXPECT_EQ(body.points.size(), 8u);
    EXPECT_EQ(body.triangles.size(), 12u);
    EXPECT_TRUE(body.closed);
    EXPECT_NEAR(volumeOf(body), 8.0, 1e-12);
}

// The plan outline given is the FootPrint; without one, the mesh's plan hull
// is - for a box, its rectangle, closed by repeating its first point.
TEST(IfcExportRoad, TheFootPrintIsThePlanOutlineOrTheMeshesHull)
{
    ifc::ExportInput input;
    auto outlined = element("outlined", "kerb", {"IfcKerb", "NOTDEFINED", {}}, "", "",
                            box(0.0, 0.0, 0.0, 10.0, 1.0, 1.0));
    outlined.plan = {Polyline2{{{0.0, 0.5}, {10.0, 0.5}}, false}};
    input.road.elements.push_back(outlined);
    input.road.elements.push_back(element("hulled", "kerb", {"IfcKerb", "NOTDEFINED", {}}, "", "",
                                          box(20.0, 0.0, 0.0, 30.0, 1.0, 1.0)));
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    const auto footPrintOf = [&](int product) {
        std::vector<std::size_t> pointCounts;
        const int shape = refOf(all.at(product).args[6]);
        for (const int representation : refsOf(all.at(shape).args[2])) {
            if (all.at(representation).args[1] == "'FootPrint'") {
                EXPECT_EQ(all.at(representation).args[2], "'Curve2D'");
                for (const int curve : refsOf(all.at(representation).args[3])) {
                    pointCounts.push_back(refsOf(all.at(curve).args[0]).size());
                }
            }
        }
        return pointCounts;
    };
    EXPECT_EQ(footPrintOf(byTag(all, "outlined")), std::vector<std::size_t>{2u});
    EXPECT_EQ(footPrintOf(byTag(all, "hulled")), std::vector<std::size_t>{5u}); // 4 corners + 1
}

// ---- material, quantities, properties ------------------------------------------------------

TEST(IfcExportRoad, EachMaterialIsWrittenOnceAndAssociatedWithEveryElementOfIt)
{
    ifc::RoadModelInput road = roadModel();
    road.elements[1].material = "Asphalt AC14"; // the base, now of the wearing course's material
    ifc::ExportInput input;
    input.road = road;
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    std::map<std::string, int> materials;
    for (const int material : ofType(all, "IFCMATERIAL")) {
        EXPECT_TRUE(materials.emplace(all.at(material).args[0], material).second);
    }
    EXPECT_EQ(materials.size(), 2u); // Asphalt AC14, Concrete
    ASSERT_TRUE(materials.contains("'Asphalt AC14'"));
    bool found = false;
    for (const int rel : ofType(all, "IFCRELASSOCIATESMATERIAL")) {
        if (refOf(all.at(rel).args[5]) == materials.at("'Asphalt AC14'")) {
            found = true;
            EXPECT_EQ(refsOf(all.at(rel).args[4]),
                      (std::vector<int>{byTag(all, kWearing), byTag(all, kBase)}));
        }
    }
    EXPECT_TRUE(found);
}

// What a class's base quantity set names, with the type it gives it, goes in
// that set (Qto_CourseBaseQuantities, 6.6.5.2: Length, Width, Thickness,
// Volume, GrossVolume, Weight), measured as 'BaseQuantities'; the rest -
// a course's Area, a sign's Count - in Katana_Quantities. A weight in tonnes
// is written in kilograms, under its own unit, since the project's units have
// none for mass.
TEST(IfcExportRoad, QuantitiesTheTemplateNamesAreBaseQuantitiesAndTheRestKatanas)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    const auto wearing = setsOf(all, byTag(all, kWearing));
    ASSERT_TRUE(wearing.contains("'Qto_CourseBaseQuantities'"));
    const int base = wearing.at("'Qto_CourseBaseQuantities'");
    EXPECT_EQ(all.at(base).type, "IFCELEMENTQUANTITY");
    EXPECT_EQ(all.at(base).args[4], "'BaseQuantities'");
    EXPECT_EQ(quantitiesOf(all, base),
              (std::set<std::string>{"IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                                     "IFCQUANTITYLENGTH('Width',$,$,3.5,$)",
                                     "IFCQUANTITYLENGTH('Thickness',$,$,0.05,$)",
                                     "IFCQUANTITYVOLUME('GrossVolume',$,$,17.5,$)"}));
    ASSERT_TRUE(wearing.contains("'Katana_Quantities'"));
    const int own = wearing.at("'Katana_Quantities'");
    EXPECT_EQ(all.at(own).args[4], "$");
    EXPECT_EQ(quantitiesOf(all, own),
              std::set<std::string>{"IFCQUANTITYAREA('Area',$,$,350.,$)"});

    const auto kerb = setsOf(all, byTag(all, kKerb));
    ASSERT_TRUE(kerb.contains("'Qto_KerbBaseQuantities'"));
    const int kilogram = ofType(all, "IFCSIUNIT").back();
    EXPECT_EQ(all.at(kilogram).line.substr(all.at(kilogram).line.find('=') + 1),
              "IFCSIUNIT(*,.MASSUNIT.,.KILO.,.GRAM.)");
    EXPECT_EQ(quantitiesOf(all, kerb.at("'Qto_KerbBaseQuantities'")),
              (std::set<std::string>{"IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                                     "IFCQUANTITYWEIGHT('Weight',$,#" + std::to_string(kilogram) +
                                         ",1500.,$)"}));

    const auto sign = setsOf(all, byTag(all, kSign));
    EXPECT_EQ(quantitiesOf(all, sign.at("'Qto_SignBaseQuantities'")),
              (std::set<std::string>{"IFCQUANTITYLENGTH('Height',$,$,0.9,$)",
                                     "IFCQUANTITYLENGTH('Width',$,$,0.6,$)"}));
    EXPECT_EQ(quantitiesOf(all, sign.at("'Katana_Quantities'")),
              std::set<std::string>{"IFCQUANTITYCOUNT('Count',$,$,1.,$)"});

    // The fill's compacted volume is its template's (6.6.5.4); the wall,
    // whose class has no template here, has no quantity set at all.
    EXPECT_TRUE(setsOf(all, byTag(all, kFill)).contains("'Qto_EarthworksFillBaseQuantities'"));
    EXPECT_FALSE(setsOf(all, byTag(all, kWall)).contains("'Katana_Quantities'"));
}

// A quantity in a unit IFC's simple quantities do not take, a negative one
// (each IfcQuantity*'s WR: the value is not negative), and a name repeated
// on one element (IfcElementQuantity's UniqueQuantityNames) are left out,
// each kind said once.
TEST(IfcExportRoad, AQuantityTheSchemaWouldRefuseIsLeftOutAndSaid)
{
    ifc::ExportInput input;
    auto kerb = element("k", "kerb", {"IfcKerb", "NOTDEFINED", {}}, "", "",
                        box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0));
    kerb.quantities = {{"Length", 3.0, "ft"},
                       {"Height", -0.1, "m"},
                       {"Width", 0.15, "m"},
                       {"Width", 0.2, "m"}};
    input.road.elements.push_back(kerb);
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    EXPECT_EQ(quantitiesOf(all, setsOf(all, byTag(all, "k")).at("'Qto_KerbBaseQuantities'")),
              std::set<std::string>{"IFCQUANTITYLENGTH('Width',$,$,0.15,$)"});
    EXPECT_TRUE(warned(out, "quantities in \"ft\" are left out"));
    EXPECT_TRUE(warned(out, "1 road element quantities are negative"));
    EXPECT_TRUE(warned(out, "repeat a name on their element"));
}

// What Katana knows of the element (Katana_RoadElement) and the element's own
// properties, as they are (Katana_RoadProperties).
TEST(IfcExportRoad, AnElementCarriesWhatItIsAndItsOwnProperties)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    const auto sets = setsOf(all, byTag(all, kWearing));
    ASSERT_TRUE(sets.contains("'Katana_RoadElement'") && sets.contains("'Katana_RoadProperties'"));
    const auto valuesOf = [&](int set) {
        std::map<std::string, std::string> values;
        for (const int property : refsOf(all.at(set).args[4])) {
            values.emplace(all.at(property).args[0], all.at(property).args[2]);
        }
        return values;
    };
    const auto own = valuesOf(sets.at("'Katana_RoadElement'"));
    EXPECT_EQ(own.at("'Id'"), "IFCIDENTIFIER('" + kWearing + "')");
    EXPECT_EQ(own.at("'Kind'"), "IFCLABEL('pavement')");
    EXPECT_EQ(own.at("'Facility'"), "IFCLABEL('Main')");
    EXPECT_EQ(own.at("'Part'"), "IFCLABEL('CARRIAGEWAY')");
    EXPECT_EQ(own.at("'ClosedSolid'"), "IFCBOOLEAN(.T.)");
    const auto properties = valuesOf(sets.at("'Katana_RoadProperties'"));
    EXPECT_EQ(properties.at("'Component'"), "IFCLABEL('lane')");
    EXPECT_EQ(properties.at("'Stations'"), "IFCLABEL('0 to 100')");
}

// ---- features ------------------------------------------------------------------------------

// A cut is the void "from modification of existing terrain" (6.6.3.4): it
// voids the surface it was taken from, is contained in nothing (5.4.3.30),
// and is placed relative to the ground it voids.
TEST(IfcExportRoad, ACutVoidsTheGroundItIsTakenFromAndIsContainedNowhere)
{
    const auto surface = ground();
    const auto out = exported(roadInput(&surface));
    const Instances all = instancesIn(out.text);
    const int cut = byTag(all, kCut);
    ASSERT_NE(cut, 0);
    EXPECT_EQ(all.at(cut).type, "IFCEARTHWORKSCUT");
    EXPECT_EQ(all.at(cut).args[8], ".CUT.");
    EXPECT_EQ(containerOf(all, cut), 0);
    int terrain = 0;
    for (const int element : ofType(all, "IFCGEOGRAPHICELEMENT")) {
        if (all.at(element).args[2] == "'ground'") {
            terrain = element;
        }
    }
    ASSERT_NE(terrain, 0);
    ASSERT_EQ(ofType(all, "IFCRELVOIDSELEMENT").size(), 1u);
    const Instance& voids = all.at(ofType(all, "IFCRELVOIDSELEMENT").front());
    EXPECT_EQ(refOf(voids.args[4]), terrain);
    EXPECT_EQ(refOf(voids.args[5]), cut);
    EXPECT_EQ(relativeTo(all, placementOf(all, cut)), placementOf(all, terrain));
    EXPECT_EQ(out.classes.at("IfcGeographicElement"), 1u); // the surface; no stand-in

    // A void takes no material (IfcRelAssociatesMaterial's NoVoidElement):
    // the cut's is said in its Katana_RoadElement, and no IfcMaterial 'Clay'
    // is written for nothing to use.
    for (const int rel : ofType(all, "IFCRELASSOCIATESMATERIAL")) {
        EXPECT_FALSE(contains(refsOf(all.at(rel).args[4]), cut));
    }
    for (const int material : ofType(all, "IFCMATERIAL")) {
        EXPECT_NE(all.at(material).args[0], "'Clay'");
    }
    const int own = setsOf(all, cut).at("'Katana_RoadElement'");
    bool said = false;
    for (const int property : refsOf(all.at(own).args[4])) {
        said = said || (all.at(property).args[0] == "'Material'" &&
                         all.at(property).args[2] == "IFCLABEL('Clay')");
    }
    EXPECT_TRUE(said);
}

// Without the ground's surface in the file, the cut voids an
// IfcGeographicElement TERRAIN that stands for that ground with no shape, so
// that the void the schema requires has an element to be in.
TEST(IfcExportRoad, WithoutItsSurfaceACutVoidsAGroundThatStandsForIt)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    const auto grounds = ofType(all, "IFCGEOGRAPHICELEMENT");
    ASSERT_EQ(grounds.size(), 1u);
    const Instance& stand = all.at(grounds.front());
    EXPECT_EQ(stand.args[2], "'ground'");
    EXPECT_EQ(stand.args[6], "$"); // no shape
    EXPECT_EQ(stand.args[8], ".TERRAIN.");
    EXPECT_EQ(refOf(all.at(ofType(all, "IFCRELVOIDSELEMENT").front()).args[4]), grounds.front());
}

// A marking adheres to the course it is painted on (IfcRelAdheresToElement,
// 5.4.3.45): of the wearing course (top 10.00) and the base (top 9.95) under
// it, the wearing course.
TEST(IfcExportRoad, AMarkingAdheresToTheHighestCourseUnderItAndIsContainedNowhere)
{
    const auto out = exported(roadInput());
    const Instances all = instancesIn(out.text);
    const int marking = byTag(all, kMarking);
    ASSERT_NE(marking, 0);
    EXPECT_EQ(all.at(marking).type, "IFCSURFACEFEATURE");
    EXPECT_EQ(all.at(marking).args[8], ".LINEMARKING.");
    EXPECT_EQ(containerOf(all, marking), 0);
    ASSERT_EQ(ofType(all, "IFCRELADHERESTOELEMENT").size(), 1u);
    const Instance& adheres = all.at(ofType(all, "IFCRELADHERESTOELEMENT").front());
    EXPECT_EQ(refOf(adheres.args[4]), byTag(all, kWearing));
    EXPECT_EQ(refsOf(adheres.args[5]), std::vector<int>{marking});
    EXPECT_EQ(relativeTo(all, placementOf(all, marking)), placementOf(all, byTag(all, kWearing)));
}

// A host named outright is taken; a marking with nothing under it has nothing
// to adhere to, and an IfcSurfaceFeature adhering to nothing would break the
// schema's mandatory inverse: it is not written, and said.
TEST(IfcExportRoad, AMarkingWithNothingUnderItIsNotWrittenAndSaid)
{
    ifc::ExportInput input;
    input.road.elements.push_back(element("lonely", "marking",
                                          {"IfcSurfaceFeature", "LINEMARKING", {}}, "Main", "",
                                          box(0.0, 0.0, 0.0, 10.0, 0.1, 0.003)));
    auto named = element("named", "marking", {"IfcSurfaceFeature", "LINEMARKING", {}}, "Other",
                         "", box(500.0, 0.0, 0.0, 510.0, 0.1, 0.003));
    named.host = "slab";
    input.road.elements.push_back(named);
    input.road.elements.push_back(element("slab", "pavement", {"IfcSlab", "PAVING", {}}, "Main",
                                          "", box(100.0, 100.0, 0.0, 110.0, 110.0, 0.2)));
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    EXPECT_EQ(byTag(all, "lonely"), 0);
    EXPECT_EQ(out.roadElements, 2u);
    EXPECT_EQ(out.roadElementsSkipped, 1u);
    EXPECT_TRUE(warned(out, "road element \"lonely\" is not written: a surface feature adheres"));
    ASSERT_NE(byTag(all, "named"), 0);
    EXPECT_EQ(refOf(all.at(ofType(all, "IFCRELADHERESTOELEMENT").front()).args[4]),
              byTag(all, "slab"));
}

// A road object has a road only when one is given (facility=), so the marking
// a person places with no road is the site's: it adheres to the course of
// whichever road is under it, as a marking of a road adheres to that road's,
// and is not dropped for want of a road it was never told.
TEST(IfcExportRoad, AMarkingThatNamesNoRoadAdheresToTheCourseUnderIt)
{
    ifc::ExportInput input = roadInput();
    for (auto& each : input.road.elements) {
        if (each.id == kMarking) {
            each.facility.clear();
            each.part.clear();
        }
    }
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    const int marking = byTag(all, kMarking);
    ASSERT_NE(marking, 0) << "the marking was dropped";
    EXPECT_EQ(out.roadElementsSkipped, 0u);
    ASSERT_EQ(ofType(all, "IFCRELADHERESTOELEMENT").size(), 1u);
    const Instance& adheres = all.at(ofType(all, "IFCRELADHERESTOELEMENT").front());
    EXPECT_EQ(refOf(adheres.args[4]), byTag(all, kWearing));
    EXPECT_EQ(refsOf(adheres.args[5]), std::vector<int>{marking});
}

// With nothing under it the reason given is the true one: it names no road AND
// no course of any road lies under it.
TEST(IfcExportRoad, AMarkingThatNamesNoRoadAndHasNoCourseUnderItSaysSo)
{
    ifc::ExportInput input;
    input.road.elements.push_back(element("lonely", "marking",
                                          {"IfcSurfaceFeature", "LINEMARKING", {}}, "", "",
                                          box(0.0, 0.0, 0.0, 10.0, 0.1, 0.003)));
    input.road.elements.push_back(element("slab", "pavement", {"IfcSlab", "PAVING", {}}, "Main",
                                          "", box(100.0, 100.0, 0.0, 110.0, 110.0, 0.2)));
    const auto out = exported(input);
    EXPECT_EQ(out.roadElementsSkipped, 1u);
    EXPECT_TRUE(warned(out, "road element \"lonely\" is not written: a surface feature adheres to "
                            "an element, and it names no road (facility=)"));
}

// Two closed boxes that touch - the cells of a box culvert side by side, with
// no gap between their walls - share positions without sharing an edge. Each
// is closed by its own indices, so the face set is closed: merging them by
// position would make four faces of the shared edges and write a closed solid
// as an open face set with a warning.
TEST(IfcExportRoad, ClosedShellsThatTouchAreOneClosedFaceSetNotAnOpenOne)
{
    const TriangleMesh left = box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    const TriangleMesh right = box(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
    TriangleMesh both = left;
    const auto offset = static_cast<std::uint32_t>(both.vertices.size());
    both.vertices.insert(both.vertices.end(), right.vertices.begin(), right.vertices.end());
    for (const auto& face : right.faces) {
        both.faces.push_back({face[0] + offset, face[1] + offset, face[2] + offset});
    }
    ifc::ExportInput input;
    input.road.elements.push_back(
        element("cells", "culvert", {"IfcPipeSegment", "CULVERT", {}}, "", "", both));
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    const FaceSet body = bodyOf(all, byTag(all, "cells"));
    EXPECT_TRUE(body.closed);
    // The positions are not merged: 2 x 8 points and 2 x 12 triangles.
    EXPECT_EQ(body.points.size(), 16u);
    EXPECT_EQ(body.triangles.size(), 24u);
    // Two unit cubes.
    EXPECT_NEAR(volumeOf(body), 2.0, 1e-12);
    EXPECT_FALSE(warned(out, "not closed solids"));
}

// What is not closed by its indices is still judged as it always was: merged by
// position, and open when an edge is left over. A pair of touching boxes with a
// face missing is open.
TEST(IfcExportRoad, TouchingShellsWithAFaceMissingAreStillWrittenOpenAndSaid)
{
    const TriangleMesh left = box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    const TriangleMesh right = box(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
    TriangleMesh both = left;
    const auto offset = static_cast<std::uint32_t>(both.vertices.size());
    both.vertices.insert(both.vertices.end(), right.vertices.begin(), right.vertices.end());
    // The second box without its last two triangles (its +x face).
    for (std::size_t i = 0; i + 2 < right.faces.size(); ++i) {
        const auto& face = right.faces[i];
        both.faces.push_back({face[0] + offset, face[1] + offset, face[2] + offset});
    }
    ifc::ExportInput input;
    input.road.elements.push_back(
        element("cells", "culvert", {"IfcPipeSegment", "CULVERT", {}}, "", "", both));
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    EXPECT_FALSE(bodyOf(all, byTag(all, "cells")).closed);
    EXPECT_TRUE(warned(out, "1 road elements are not closed solids"));
}

// ---- what is refused -----------------------------------------------------------------------

// Everything that would make an invalid file is left out, each element said
// by its id, and the rest is written.
TEST(IfcExportRoad, WhatWouldMakeAnInvalidFileIsLeftOutAndSaid)
{
    const auto unit = box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    ifc::ExportInput input;
    auto& elements = input.road.elements;
    elements.push_back(element("good", "kerb", {"IfcKerb", "NOTDEFINED", {}}, "", "", unit));
    elements.push_back(element("", "kerb", {"IfcKerb", "NOTDEFINED", {}}, "", "", unit));
    elements.push_back(element("good", "kerb", {"IfcKerb", "NOTDEFINED", {}}, "", "", unit));
    elements.push_back(element("asphalt", "pavement", {"IfcCourse", "ASPHALT", {}}, "", "", unit));
    elements.push_back(element("made-up", "thing", {"IfcRoadThing", "", {}}, "", "", unit));
    elements.push_back(element("nameless", "thing", {"", "", {}}, "", "", unit));
    elements.push_back(element("note", "thing", {"IfcAnnotation", "", {}}, "", "", unit));
    elements.push_back(element("vague", "wall", {"IfcWall", "USERDEFINED", {}}, "", "", unit));
    auto broken = element("broken", "kerb", {"IfcKerb", "", {}}, "", "", unit);
    broken.mesh.vertices[3].z = std::nan("");
    elements.push_back(broken);
    elements.push_back(element("empty", "kerb", {"IfcKerb", "", {}}, "", "", TriangleMesh{}));
    auto badPlan = element("bad-plan", "kerb", {"IfcKerb", "", {}}, "", "", unit);
    badPlan.plan = {Polyline2{{{0.0, 0.0}, {INFINITY, 0.0}}, false}};
    elements.push_back(badPlan);

    const auto out = exported(input);
    EXPECT_EQ(out.roadElements, 1u);
    EXPECT_EQ(out.roadElementsSkipped, 10u);
    for (const std::string& words :
         {std::string("road element \"\" is not written: it has no id"),
          std::string("road element \"good\" is not written: an earlier element has the same id"),
          std::string("\"ASPHALT\" is not a predefined type of IfcCourse"),
          std::string("IfcRoadThing is not a class this export writes"),
          std::string("road element \"nameless\" is not written: it names no IFC class"),
          std::string("IfcAnnotation is not a class this export writes"),
          std::string("road element \"vague\" is not written: it is USERDEFINED"),
          std::string("road element \"broken\" is not written: its mesh is not valid"),
          std::string("road element \"empty\" is not written: its mesh has no face"),
          std::string("road element \"bad-plan\" is not written: its plan outline")}) {
        EXPECT_TRUE(warned(out, words)) << words;
    }
    // Each is in the account too, as not written.
    std::size_t notWritten = 0;
    for (const ifc::ClassTally& row : out.tally) {
        if (row.entity.empty() && row.why.starts_with("not written: ")) {
            notWritten += row.count;
        }
    }
    EXPECT_EQ(notWritten, 10u);
}

// A part the enumeration lists is its own type; ROADSEGMENT is the one
// longitudinal part ("Longitudinal, linear segment of a road"), an
// INTERSECTION a region; a part it does not list is USERDEFINED of that name,
// its usage NOTDEFINED.
TEST(IfcExportRoad, APartsUsageIsWhatTheEnumerationSaysOfItAndAnUnlistedPartIsUserDefined)
{
    const auto unit = box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    ifc::ExportInput input;
    for (const char* part : {"ROADSEGMENT", "INTERSECTION", "BUSWAY"}) {
        input.road.elements.push_back(element(std::string("in-") + part, "pavement",
                                              {"IfcCourse", "PAVEMENT", {}}, "Main", part, unit));
    }
    const auto out = exported(input);
    const Instances all = instancesIn(out.text);
    std::map<std::string, std::vector<std::string>> parts;
    for (const int part : ofType(all, "IFCROADPART")) {
        const auto& args = all.at(part).args;
        parts[args[2]] = {args[4], args[9], args[10]};
    }
    EXPECT_EQ(parts.at("'Main ROADSEGMENT'"),
              (std::vector<std::string>{"$", ".LONGITUDINAL.", ".ROADSEGMENT."}));
    EXPECT_EQ(parts.at("'Main INTERSECTION'"),
              (std::vector<std::string>{"$", ".REGION.", ".INTERSECTION."}));
    EXPECT_EQ(parts.at("'Main BUSWAY'"),
              (std::vector<std::string>{"'BUSWAY'", ".NOTDEFINED.", ".USERDEFINED."}));
}

// A drawing entity cannot be a feature of anything: a rule naming one is
// refused when its file is read and when it is handed to the writer.
TEST(IfcRulesFile, ARuleMayNotNameAFeatureClass)
{
    for (const char* feature : {"IfcEarthworksCut", "IfcSurfaceFeature"}) {
        const auto read = ifc::parseClassificationRules(
            std::string("rule,words,class,predefined_type\nx,X,") + feature + ",\n");
        ASSERT_FALSE(read.ok()) << feature;
        EXPECT_NE(read.error().describe().find("is a feature of another element"),
                  std::string::npos)
            << read.error().describe();

        ifc::ExportOptions with = options();
        with.rules = {ifc::ClassificationRule{"x", {"X"}, {}, {feature, "", ""}, ""}};
        const auto written = ifc::writeIfc({}, with);
        ASSERT_FALSE(written.ok()) << feature;
        EXPECT_NE(written.error().describe().find("a feature of another element"),
                  std::string::npos);
    }
    // The classes the road model writes as elements are a rule's to name.
    const auto course = ifc::parseClassificationRules(
        "rule,words,class,predefined_type\nbase,BASE,IfcCourse,PAVEMENT\n");
    EXPECT_TRUE(course.ok()) << (course.ok() ? "" : course.error().describe());
}

// ---- the whole file ----------------------------------------------------------------------

// GlobalIds are made from the ids ("road/element/<id>" in the project's
// namespace): the same model exported twice is the same file, and an element
// keeps its GlobalId when others are added before it.
TEST(IfcExportRoad, GlobalIdsComeFromTheIdsAndAreStableAcrossExports)
{
    const auto first = exported(roadInput());
    const auto second = exported(roadInput());
    EXPECT_EQ(first.text, second.text);

    const Instances all = instancesIn(first.text);
    EXPECT_EQ(all.at(byTag(all, kWearing)).args[0],
              "'" + ifc::guidFor("Test", "road/element/" + kWearing) + "'");
    EXPECT_EQ(all.at(ofType(all, "IFCROAD").front()).args[0],
              "'" + ifc::guidFor("Test", "road/facility/Main") + "'");

    ifc::ExportInput grown = roadInput();
    grown.road.elements.insert(grown.road.elements.begin(),
                               element("object/sign-0", "sign", {"IfcSign", "PICTORAL", {}}, "",
                                       "", box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)));
    const Instances after = instancesIn(exported(grown).text);
    EXPECT_EQ(after.at(byTag(after, kWearing)).args[0], all.at(byTag(all, kWearing)).args[0]);
}

// The model's problems are the export's warnings, and its reply counts the
// road model - only when one was given, so an export without one answers as
// it always has.
TEST(IfcExportRoad, TheModelsProblemsAreWarningsAndTheReplyCountsTheRoadModel)
{
    const auto out = exported(roadInput());
    EXPECT_TRUE(warned(out, "the road model: corridor Main: the profile ends at 100"));
    const std::string reply = ifc::formatExportReply(out, "road.ifc", false);
    EXPECT_NE(reply.find("\nroad elements=8 skipped=0 roads=1 parts=2\n"), std::string::npos)
        << reply;
    EXPECT_NE(reply.find("\nobject from=\"road Main\" count=2 class=\"IfcCourse\" "
                         "predefined=\"PAVEMENT\" object_type=\"\" system=\"\" "
                         "why=\"the road model's pavement\""),
              std::string::npos)
        << reply;
    EXPECT_NE(reply.find("\nobject from=\"road model\" count=1 class=\"IfcSign\""),
              std::string::npos);

    const auto without = exported(ifc::ExportInput{});
    EXPECT_EQ(ifc::formatExportReply(without, "x.ifc", false).find("road elements="),
              std::string::npos);
}

// The local origin is the south-west corner of everything written, the road
// model included: rounded down to 100 m, 1000, 1900 for the cut at y 1997.
TEST(IfcExportRoad, TheLocalOriginTakesTheRoadModelIntoAccount)
{
    ifc::ExportOptions georeferenced = options();
    georeferenced.georeference.name = "EPSG:7856";
    const auto out = exported(roadInput(), georeferenced);
    const Instances all = instancesIn(out.text);
    const auto conversions = ofType(all, "IFCMAPCONVERSION");
    ASSERT_EQ(conversions.size(), 1u);
    EXPECT_EQ(all.at(conversions.front()).args[2], "1000.");
    EXPECT_EQ(all.at(conversions.front()).args[3], "1900.");
}

// The file IfcOpenShell judges (tools/check_ifc.py; the
// ifc.the_road_model_export_is_valid_to_ifcopenshell test, registered where
// it is installed): the model with its ground, georeferenced, written whole.
// KATANA_IFC_ROAD_FILE names where; without it, a temporary file is written
// and removed.
TEST(IfcExportRoad, TheRoadModelIsWrittenForTheValidator)
{
    const char* named = std::getenv("KATANA_IFC_ROAD_FILE");
    const std::filesystem::path path =
        named != nullptr ? std::filesystem::path(named)
                         : std::filesystem::temp_directory_path() / "katana_road_model.ifc";
    const auto surface = ground();
    ifc::ExportOptions georeferenced = options();
    georeferenced.georeference.name = "EPSG:7856";
    const auto written = ifc::writeIfcFile(roadInput(&surface), path, georeferenced);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(std::filesystem::exists(path));
    EXPECT_EQ(written->roadElements, 8u);
    EXPECT_EQ(written->classes.count("IfcBuildingElementProxy"), 0u);
    if (named == nullptr) {
        std::error_code ignored;
        std::filesystem::remove(path, ignored);
    }
}
