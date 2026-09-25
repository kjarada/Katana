#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <set>
#include <string>
#include <vector>

#include "katana/entity/model.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/import.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace ifc = katana::ifc;
namespace sub = katana::survey::subsurface;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

std::string readText(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    EXPECT_TRUE(file) << path;
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

void add(Model& model, katana::entity::Geometry geometry, std::string layer,
         std::vector<std::optional<double>> heights = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = std::move(layer);
    if (!heights.empty()) {
        katana::entity::setHeights(entity.properties, heights);
    }
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

// MC01: four PIs in MGA zone 56, a spiralled curve at the first (R 80,
// transitions of 20 m) turning right and a simple curve at the second (R 60)
// turning left, stationed from 1000, with a profile of two vertical curves.
katana::entity::Alignment mc01()
{
    katana::entity::Alignment alignment;
    alignment.name = "MC01";
    alignment.horizontal.startStation = 1000.0;
    alignment.horizontal.pis = {{Point2{333900.0, 6249950.0}, 0.0, 0.0, 0.0},
                                {Point2{334000.0, 6250020.0}, 80.0, 20.0, 20.0},
                                {Point2{334100.0, 6249990.0}, 60.0, 0.0, 0.0},
                                {Point2{334200.0, 6250060.0}, 0.0, 0.0, 0.0}};
    alignment.vertical = katana::geometry::VerticalAlignment{
        {{1000.0, 21.5, 0.0}, {1080.0, 23.0, 60.0}, {1180.0, 20.0, 80.0}, {1330.5, 21.0, 0.0}}};
    return alignment;
}

// The drawing the cli scenario makes (docs/ifc.md): the alignment, a kerb
// line with its levels, a fence, a stormwater pit, a surveyed point and a
// lot number.
Model scenario()
{
    Model model;
    EXPECT_TRUE(model.alignments.add(mc01()).ok());
    add(model,
        Polyline2{{{333990.0, 6250000.0}, {334010.0, 6250004.0}, {334030.0, 6250008.0}}, false},
        "Survey/Kerb", {20.10, 20.05, 20.00});
    add(model, Polyline2{{{333990.0, 6250012.0}, {334030.0, 6250015.0}}, false}, "Survey/Fence");
    add(model, katana::entity::PointGeometry{{334015.0, 6249995.0}}, "Stormwater/Pits");
    add(model, katana::entity::PointGeometry{{334005.0, 6250002.0}}, "Survey/Points");
    add(model, katana::entity::TextGeometry{{334000.0, 6250020.0}, "LOT 42"}, "Survey/Points");
    return model;
}

ifc::ExportOptions mga56()
{
    ifc::ExportOptions options;
    options.projectName = "Test";
    options.georeference.name = "EPSG:7856";
    options.georeference.description = "GDA2020 / MGA zone 56";
    return options;
}

ifc::IfcExport exported(const ifc::ExportInput& input, const ifc::ExportOptions& options)
{
    auto result = ifc::writeIfc(input, options);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : ifc::IfcExport{};
}

// Every instance line of a class, in the order written.
std::vector<std::string> instancesOf(const std::string& text, const std::string& entity)
{
    std::vector<std::string> out;
    const std::string needle = "=" + entity + "(";
    for (std::size_t at = text.find(needle); at != std::string::npos;
         at = text.find(needle, at + 1)) {
        const std::size_t start = text.rfind('\n', at) + 1;
        out.push_back(text.substr(start, text.find('\n', at) - start));
    }
    return out;
}

// The top-level arguments of one instance line, as written.
std::vector<std::string> argumentsOf(const std::string& line)
{
    std::vector<std::string> out;
    const std::size_t open = line.find('(');
    const std::size_t close = line.rfind(')');
    std::string current;
    int depth = 0;
    bool quoted = false;
    for (std::size_t i = open + 1; i < close; ++i) {
        const char c = line[i];
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

double real(const std::string& argument)
{
    return std::stod(argument);
}

const Entity* withName(const ifc::IfcImport& imported, const std::string& name)
{
    for (const Entity& entity : imported.entities) {
        const auto found = entity.metadata.find("ifc.name");
        if (found != entity.metadata.end() && std::get<std::string>(found->second) == name) {
            return &entity;
        }
    }
    return nullptr;
}

std::string property(const Entity& entity, const std::string& name)
{
    const auto found = entity.properties.find(name);
    return found == entity.properties.end() ? std::string("<absent>")
                                            : katana::entity::toString(found->second);
}

ifc::UtilityInput sampleSchedule()
{
    auto lines =
        sub::parseUtilityCsv(readText(std::string(KATANA_SAMPLES) + "/utilities/schedule.csv"));
    EXPECT_TRUE(lines.ok()) << (lines.ok() ? "" : lines.error().describe());
    ifc::UtilityInput input;
    if (lines.ok()) {
        input.lines = std::move(*lines);
    }
    input.sourceName = "schedule.csv";
    return input;
}

} // namespace

// ---- the file --------------------------------------------------------------------

TEST(IfcExport, WritesAnIfc4x3Add2FileInTheAlignmentBasedView)
{
    const Model model = scenario();
    const auto out = exported({&model, {}, {}}, mga56());
    EXPECT_TRUE(out.text.starts_with("ISO-10303-21;\nHEADER;\n"));
    EXPECT_NE(out.text.find("FILE_DESCRIPTION(('ViewDefinition [Alignment-basedView]'),'2;1');"),
              std::string::npos);
    EXPECT_NE(out.text.find("FILE_SCHEMA(('IFC4X3_ADD2'));"), std::string::npos);
    EXPECT_TRUE(out.text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
    EXPECT_EQ(instancesOf(out.text, "IFCPROJECT").size(), 1u);
    EXPECT_EQ(instancesOf(out.text, "IFCSITE").size(), 1u);
    EXPECT_EQ(out.bytesWritten, out.text.size());
    // One line per instance, #1 up.
    EXPECT_NE(out.text.find("\n#" + std::to_string(out.instances) + "="), std::string::npos);
    EXPECT_EQ(out.text.find("\n#" + std::to_string(out.instances + 1) + "="), std::string::npos);
}

// The local origin is the south-west corner of the work rounded down to
// 100 m: the first PI is the furthest west (333900) and south (6249950), so
// 333900, 6249900 - and the map conversion says so, unrotated and unscaled.
TEST(IfcExport, IsGeoreferencedByAMapConversionFromALocalOriginNearTheWork)
{
    const Model model = scenario();
    const auto out = exported({&model, {}, {}}, mga56());
    const auto crs = instancesOf(out.text, "IFCPROJECTEDCRS");
    ASSERT_EQ(crs.size(), 1u);
    EXPECT_EQ(argumentsOf(crs[0])[0], "'EPSG:7856'");
    EXPECT_EQ(argumentsOf(crs[0])[1], "'GDA2020 / MGA zone 56'");
    const auto conversion = instancesOf(out.text, "IFCMAPCONVERSION");
    ASSERT_EQ(conversion.size(), 1u);
    const auto args = argumentsOf(conversion[0]);
    EXPECT_EQ(args[2], "333900.");
    EXPECT_EQ(args[3], "6249900.");
    EXPECT_EQ(args[4], "0.");
    EXPECT_EQ(args[5], "1."); // x axis abscissa
    EXPECT_EQ(args[6], "0."); // x axis ordinate
    EXPECT_EQ(args[7], "1."); // scale
    EXPECT_TRUE(out.warnings.empty()) << out.warnings.front();

    // A chosen origin is used as given.
    auto options = mga56();
    options.localOrigin = katana::math::Vec3(333000.0, 6249000.0, 10.0);
    const auto chosen = exported({&model, {}, {}}, options);
    const auto chosenArgs = argumentsOf(instancesOf(chosen.text, "IFCMAPCONVERSION")[0]);
    EXPECT_EQ(chosenArgs[2], "333000.");
    EXPECT_EQ(chosenArgs[4], "10.");
}

TEST(IfcExport, WithoutAnEpsgCodeTheFileIsNotGeoreferencedAndSaysSo)
{
    const Model model = scenario();
    ifc::ExportOptions options;
    const auto none = exported({&model, {}, {}}, options);
    EXPECT_TRUE(instancesOf(none.text, "IFCMAPCONVERSION").empty());
    ASSERT_EQ(none.warnings.size(), 1u);
    EXPECT_NE(none.warnings[0].find("no coordinate system"), std::string::npos);
    // The coordinates are the project's own: the kerb's first vertex is
    // written where it is.
    EXPECT_NE(none.text.find("IFCCARTESIANPOINT((333990.,6250000.,20.1))"), std::string::npos);

    options.georeference.name = "GDA2020 / MGA zone 56 (WKT)";
    const auto named = exported({&model, {}, {}}, options);
    EXPECT_TRUE(instancesOf(named.text, "IFCPROJECTEDCRS").empty());
    ASSERT_EQ(named.warnings.size(), 1u);
    EXPECT_NE(named.warnings[0].find("has no EPSG code"), std::string::npos);

    // A shift nothing in the file records would move the survey.
    options.localOrigin = katana::math::Vec3(333900.0, 6249900.0, 0.0);
    const auto refused = ifc::writeIfc({&model, {}, {}}, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

// Rule 7: the same input is the same file, byte for byte, and the GlobalIds
// are the project's - the same next time, different for another project.
TEST(IfcExport, TheSameInputIsTheSameFileAndGlobalIdsBelongToTheProject)
{
    const Model model = scenario();
    auto input = ifc::ExportInput{&model, {}, sampleSchedule()};
    const auto first = exported(input, mga56());
    const auto second = exported(input, mga56());
    EXPECT_EQ(first.text, second.text);

    auto other = mga56();
    other.guidNamespace = "another project";
    const auto third = exported(input, other);
    EXPECT_NE(first.text, third.text);
    EXPECT_EQ(first.instances, third.instances);
    const auto project = argumentsOf(instancesOf(first.text, "IFCPROJECT")[0])[0];
    EXPECT_EQ(third.text.find(project), std::string::npos);

    // No GlobalId is used twice in a file. A GlobalId is the first argument
    // of what has one: 22 characters of the IFC alphabet, the first 0 to 3.
    const std::string alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    std::set<std::string> ids;
    std::size_t rooted = 0;
    for (std::size_t at = first.text.find("=IFC"); at != std::string::npos;
         at = first.text.find("=IFC", at + 1)) {
        const std::size_t open = first.text.find('(', at);
        if (first.text.compare(open, 2, "('") != 0 || first.text.compare(open + 24, 2, "',") != 0) {
            continue;
        }
        const std::string candidate = first.text.substr(open + 2, 22);
        if (candidate.find_first_not_of(alphabet) == std::string::npos &&
            candidate.front() >= '0' && candidate.front() <= '3') {
            ++rooted;
            EXPECT_TRUE(ids.insert(candidate).second) << candidate;
        }
    }
    EXPECT_GT(rooted, 100u);
}

TEST(IfcExport, WriteIfcFileWritesTheFileWhole)
{
    const Model model = scenario();
    const auto path = std::filesystem::temp_directory_path() / "katana_ifc_export_test.ifc";
    const auto written = ifc::writeIfcFile({&model, {}, {}}, path, mga56());
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(written->text.empty());
    const std::string text = readText(path);
    EXPECT_EQ(text.size(), written->bytesWritten);
    EXPECT_NE(text.find("FILE_NAME('katana_ifc_export_test.ifc',"), std::string::npos);
    EXPECT_FALSE(std::filesystem::exists(path.string() + ".partial"));
    std::filesystem::remove(path);

    const auto nowhere = ifc::writeIfcFile(
        {&model, {}, {}}, std::filesystem::path("/no/such/directory/x.ifc"), mga56());
    ASSERT_FALSE(nowhere.ok());
    EXPECT_EQ(nowhere.error().code, ErrorCode::FileExportFailure);

    EXPECT_TRUE(ifc::isIfcPath("site.IFC"));
    EXPECT_TRUE(ifc::isIfcPath("a/b/site.ifc"));
    EXPECT_FALSE(ifc::isIfcPath("site.ifcxml"));
    EXPECT_FALSE(ifc::isIfcPath("site.dxf"));
}

// ---- alignments ---------------------------------------------------------------------

// The horizontal's design parameters, worked out from the PIs:
//   tangent bearings atan2(70, 100) = 0.6107259644 and atan2(-30, 100) =
//   -0.2914567944, so the deflection at each curve is 0.9021827588;
//   at PI 1, R 80 with 20 m transitions: the circular arc subtends
//   0.9021827588 - (20 + 20) / (2 x 80) = 0.6521827588, 52.1746207 m, and
//   it turns right, so its radius is -80;
//   at PI 2, R 60, no transitions: 60 x 0.9021827588 = 54.1309655 m, left.
TEST(IfcExportAlignment, TheHorizontalIsThePiDefinitionsDesignParametersClosedByAZeroLengthSegment)
{
    const Model model = scenario();
    const auto out = exported({&model, {}, {}}, mga56());
    EXPECT_EQ(out.alignments, 1u);
    EXPECT_EQ(out.classes.at("IfcAlignment"), 1u);
    const auto segments = instancesOf(out.text, "IFCALIGNMENTHORIZONTALSEGMENT");
    ASSERT_EQ(segments.size(), 8u);
    const std::vector<std::string> types{".LINE.", ".CLOTHOID.",    ".CIRCULARARC.", ".CLOTHOID.",
                                         ".LINE.", ".CIRCULARARC.", ".LINE.",        ".LINE."};
    for (std::size_t i = 0; i < segments.size(); ++i) {
        EXPECT_EQ(argumentsOf(segments[i])[8], types[i]) << segments[i];
    }
    const double deflection = std::atan2(70.0, 100.0) + std::atan2(30.0, 100.0);
    const auto arc1 = argumentsOf(segments[2]);
    EXPECT_NEAR(real(arc1[4]), -80.0, 1e-12);
    EXPECT_NEAR(real(arc1[6]), 80.0 * (deflection - 40.0 / 160.0), 1e-6);
    const auto spiral = argumentsOf(segments[1]);
    EXPECT_NEAR(real(spiral[4]), 0.0, 1e-12);   // from the tangent...
    EXPECT_NEAR(real(spiral[5]), -80.0, 1e-12); // ...to the curve
    EXPECT_NEAR(real(spiral[6]), 20.0, 1e-9);
    EXPECT_NEAR(real(argumentsOf(segments[0])[3]), std::atan2(70.0, 100.0), 1e-9);
    const auto arc2 = argumentsOf(segments[5]);
    EXPECT_NEAR(real(arc2[4]), 60.0, 1e-12);
    EXPECT_NEAR(real(arc2[6]), 60.0 * deflection, 1e-6);
    EXPECT_NEAR(real(argumentsOf(segments[4])[3]), -std::atan2(30.0, 100.0), 1e-9);
    // Closed by a segment of length 0 at the end, as IFC 4.3 requires.
    EXPECT_EQ(argumentsOf(segments[7])[6], "0.");
    EXPECT_EQ(argumentsOf(segments[7])[0], "'End'");
}

// The profile's: grades (23 - 21.5) / 80 = 0.01875, (20 - 23) / 100 = -0.03
// and (21 - 20) / 150.5; the first curve (60 m at 1080) starts at 1050, 50 m
// from the start, at 21.5 + 50 x 0.01875 = 22.4375, and its radius is
// L / (g2 - g1) = 60 / -0.04875.
TEST(IfcExportAlignment, TheVerticalIsThePviDefinitionsDesignParameters)
{
    const Model model = scenario();
    const auto out = exported({&model, {}, {}}, mga56());
    const auto segments = instancesOf(out.text, "IFCALIGNMENTVERTICALSEGMENT");
    ASSERT_EQ(segments.size(), 6u);
    const std::vector<std::string> types{".CONSTANTGRADIENT.", ".PARABOLICARC.",
                                         ".CONSTANTGRADIENT.", ".PARABOLICARC.",
                                         ".CONSTANTGRADIENT.", ".CONSTANTGRADIENT."};
    for (std::size_t i = 0; i < segments.size(); ++i) {
        EXPECT_EQ(argumentsOf(segments[i])[8], types[i]) << segments[i];
    }
    const auto first = argumentsOf(segments[0]);
    EXPECT_NEAR(real(first[2]), 0.0, 1e-12);
    EXPECT_NEAR(real(first[3]), 50.0, 1e-9);
    EXPECT_NEAR(real(first[4]), 21.5, 1e-12);
    EXPECT_NEAR(real(first[5]), 0.01875, 1e-12);
    const auto curve = argumentsOf(segments[1]);
    EXPECT_NEAR(real(curve[2]), 50.0, 1e-9);
    EXPECT_NEAR(real(curve[3]), 60.0, 1e-9);
    EXPECT_NEAR(real(curve[4]), 22.4375, 1e-9);
    EXPECT_NEAR(real(curve[6]), -0.03, 1e-12);
    EXPECT_NEAR(real(curve[7]), 60.0 / -0.04875, 1e-6);
    EXPECT_NEAR(real(argumentsOf(segments[4])[5]), 1.0 / 150.5, 1e-12);
    EXPECT_EQ(argumentsOf(segments[5])[3], "0.");
}

TEST(IfcExportAlignment, StationsAreReferentsWithPsetStationingAtTheStartTheKeyPointsAndTheEnd)
{
    const Model model = scenario();
    const auto out = exported({&model, {}, {}}, mga56());
    const auto referents = instancesOf(out.text, "IFCREFERENT");
    ASSERT_FALSE(referents.empty());
    EXPECT_EQ(argumentsOf(referents.front())[2], "'Start 1000.000'");
    EXPECT_EQ(argumentsOf(referents.front())[7], ".STATION.");
    EXPECT_NE(out.text.find("IFCPROPERTYSINGLEVALUE('Station',$,IFCLENGTHMEASURE(1000.),$)"),
              std::string::npos);
    // TS, SC, CS, ST, PC, PT, the two PVCs and PVTs, the start and the end.
    EXPECT_EQ(referents.size(), 12u);
    EXPECT_EQ(out.classes.at("IfcReferent"), 12u);
    EXPECT_EQ(argumentsOf(referents[1])[2], "'PVC 1050.000'");
}

// ---- services -----------------------------------------------------------------------

// samples/utilities/schedule.csv: W1 has six points (W1-X1 among them), E1
// four, T1 and G1 two each - one graded segment between each pair, 5 + 3 +
// 1 + 1 = 10. A segment is drawn in 3D where the centre level is known at
// both ends: W1-2 and W1-3 are centre levels, W1-X1 and W1-4 top levels on
// a 150 mm main (centre 75 mm below), W1-1 and W1-5 have none - so W1-2 to
// W1-4 is three segments - and all four of E1's are top levels on a 400 mm
// size: three more. Six.
TEST(IfcExportUtilities, EveryGradedSegmentIsOneElementAndOnlyThoseWithLevelsAreIn3d)
{
    const auto out = exported({nullptr, {}, sampleSchedule()}, mga56());
    EXPECT_EQ(out.services, 4u);
    EXPECT_EQ(out.serviceSegments, 10u);
    EXPECT_EQ(out.segmentsIn3d, 6u);
    EXPECT_EQ(out.locatedPoints, 14u);
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 4u);
    // W1 and G1 are pipes; E1 is "4 x 100 mm conduits", cable carriers; T1
    // a cable.
    EXPECT_EQ(out.classes.at("IfcPipeSegment"), 6u);
    EXPECT_EQ(out.classes.at("IfcCableCarrierSegment"), 3u);
    EXPECT_EQ(out.classes.at("IfcCableSegment"), 1u);
    EXPECT_EQ(out.classes.at("IfcAnnotation"), 14u);
    EXPECT_FALSE(out.classes.contains("IfcBuildingElementProxy"));
    EXPECT_EQ(instancesOf(out.text, "IFCSWEPTDISKSOLID").size(), 6u);

    const auto systems = instancesOf(out.text, "IFCDISTRIBUTIONSYSTEM");
    std::map<std::string, std::string> systemOf;
    for (const std::string& line : systems) {
        const auto args = argumentsOf(line);
        systemOf[args[2]] = args[6];
    }
    EXPECT_EQ(systemOf["'W1'"], ".WATERSUPPLY.");
    EXPECT_EQ(systemOf["'E1'"], ".ELECTRICAL.");
    EXPECT_EQ(systemOf["'T1'"], ".COMMUNICATION.");
    EXPECT_EQ(systemOf["'G1'"], ".GAS.");
}

// E1-1 to E1-2: tops 19.60 and 19.55 on a 400 mm size, so centres 19.40 and
// 19.35. Read back, the segment's axis is at those heights, and it carries
// its grade.
TEST(IfcExportUtilities, ASegmentCarriesItsGradeAndIsDrawnAtTheCentreOfTheService)
{
    const auto out = exported({nullptr, {}, sampleSchedule()}, mga56());
    const auto back = ifc::readIfc(out.text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    const Entity* segment = withName(*back, "E1 E1-1 to E1-2");
    ASSERT_NE(segment, nullptr);
    EXPECT_EQ(std::get<std::string>(segment->metadata.at("ifc.class")), "IfcCableCarrierSegment");
    EXPECT_EQ(std::get<std::string>(segment->metadata.at("ifc.predefinedType")), "CONDUITSEGMENT");
    const auto heights = katana::entity::heightsOf(segment->properties, 2);
    ASSERT_TRUE(heights[0] && heights[1]);
    EXPECT_NEAR(*heights[0], 19.40, 1e-9);
    EXPECT_NEAR(*heights[1], 19.35, 1e-9);
    EXPECT_EQ(property(*segment, "AS5488_QualityLevel/QualityLevel"), "QL-B");
    EXPECT_EQ(property(*segment, "AS5488_QualityLevel/ClaimSupported"), "true");
    EXPECT_EQ(property(*segment, "Pset_Uncertainty/UncertaintyBasis"), "MEASUREMENT");
    EXPECT_EQ(property(*segment, "Classification/AS 5488.1-2019"), "QL-B");
    EXPECT_EQ(property(*segment, "System"), "E1");

    // G1 is from the records: QL-D, in plan only - no level was invented.
    const Entity* records = withName(*back, "G1 G1-1 to G1-2");
    ASSERT_NE(records, nullptr);
    EXPECT_EQ(property(*records, "AS5488_QualityLevel/QualityLevel"), "QL-D");
    EXPECT_EQ(property(*records, "Pset_Uncertainty/UncertaintyBasis"), "ESTIMATE");
    EXPECT_FALSE(katana::entity::heightsOf(records->properties, 2)[0]);
}

// With the delivery schema, the schedule's own values go in a property set
// named for it - its title's words joined by '_', "Utility Delivery Schema"
// in the test data - in its order, a listed value as the enumeration it is from
// - and a value the schema does not list (E-0001's "In service") as
// written, not corrected. E-0001's two points are 18 m apart, beyond the
// 10 m a detected path keeps QL-B for: it claims QL-B and attains QL-C.
TEST(IfcExportUtilities, TheDeliverySchemasAttributesAreWrittenAsTheScheduleWroteThem)
{
    auto lines = sub::parseUtilityCsv(
        readText(std::string(KATANA_SAMPLES) + "/utilities/schedule_tfnsw.csv"));
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    auto schema = sub::parseDeliverySchema(
        readText(std::string(KATANA_IFC_TEST_DATA) + "/delivery_schema.csv"));
    ASSERT_TRUE(schema.ok()) << schema.error().describe();
    ifc::UtilityInput input;
    input.lines = std::move(*lines);
    input.schema = &*schema;
    const auto out = exported({nullptr, {}, std::move(input)}, mga56());

    EXPECT_NE(out.text.find("IFCPROPERTYENUMERATION('AssetStatus',(IFCLABEL('In Service'),"
                            "IFCLABEL('Disused')),$)"),
              std::string::npos);
    EXPECT_NE(out.text.find("IFCPROPERTYSINGLEVALUE('AssetStatus','Asset Status',"
                            "IFCLABEL('In service'),$)"),
              std::string::npos);
    EXPECT_NE(out.text.find("IFCPROPERTYENUMERATEDVALUE('AssetTypeCode','Asset Type Code',"
                            "(IFCLABEL('E')),"),
              std::string::npos);
    EXPECT_NE(out.text.find("'Utility_Delivery_Schema'"), std::string::npos);
    // E-0001's asset type code E is electricity in AS 5488.2-2019 Table A.4,
    // the standard the codes are from, whichever schema carried them.
    EXPECT_NE(out.text.find("IFCCLASSIFICATION('Standards Australia','2019',$,'AS 5488.2-2019',"),
              std::string::npos);
    EXPECT_NE(out.text.find("IFCCLASSIFICATIONREFERENCE($,'E','electricity',"), std::string::npos);

    const auto back = ifc::readIfc(out.text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    const Entity* conduit = withName(*back, "E-0001 E1 to E2");
    ASSERT_NE(conduit, nullptr);
    EXPECT_EQ(std::get<std::string>(conduit->metadata.at("ifc.class")), "IfcCableCarrierSegment");
    EXPECT_EQ(property(*conduit, "AS5488_QualityLevel/QualityLevel"), "QL-C");
    EXPECT_EQ(property(*conduit, "AS5488_QualityLevel/QualityLevelClaimed"), "QL-B");
    EXPECT_EQ(property(*conduit, "AS5488_QualityLevel/ClaimSupported"), "false");
    EXPECT_EQ(property(*conduit, "Utility_Delivery_Schema/AssetStatus"), "In service");
    EXPECT_EQ(property(*conduit, "Pset_ConstructionOccurence/InstallationDate"), "2011-02-14");
    // Top of encasement 20.40 - 0.70 = 19.70, the centre 50 mm below on a
    // 100 mm size.
    const auto heights = katana::entity::heightsOf(conduit->properties, 2);
    ASSERT_TRUE(heights[0]);
    EXPECT_NEAR(*heights[0], 19.65, 1e-9);
}

// ---- the drawing --------------------------------------------------------------------

TEST(IfcExportDrawing, EntitiesAreTheClassesTheirLayersNameAndNoneIsAProxy)
{
    const Model model = scenario();
    const auto out = exported({&model, {}, {}}, mga56());
    EXPECT_EQ(out.entitiesWritten, 5u);
    EXPECT_EQ(out.entitiesSkipped, 0u);
    EXPECT_EQ(out.classes.at("IfcKerb"), 1u);
    EXPECT_EQ(out.classes.at("IfcRailing"), 1u);
    EXPECT_EQ(out.classes.at("IfcDistributionChamberElement"), 1u);
    EXPECT_EQ(out.classes.at("IfcAnnotation"), 2u);
    EXPECT_FALSE(out.classes.contains("IfcBuildingElementProxy"));
    EXPECT_EQ(out.text.find("IFCBUILDINGELEMENTPROXY"), std::string::npos);
    // The kerb's levels make its Axis 3D; the fence has none, so a 2D
    // FootPrint, not a line at height 0.
    EXPECT_EQ(argumentsOf(instancesOf(out.text, "IFCRAILING")[0])[8], ".FENCE.");
    EXPECT_NE(out.text.find("IFCCARTESIANPOINT((90.,100.,20.1))"), std::string::npos);
    EXPECT_NE(out.text.find("IFCCARTESIANPOINT((90.,112.))"), std::string::npos);
    EXPECT_NE(out.text.find("IFCTEXTLITERALWITHEXTENT('LOT 42',"), std::string::npos);
    EXPECT_EQ(out.text.find("IFCTEXTLITERAL("), std::string::npos); // deprecated in 4.3
    // Each entity keeps its layer, as a presentation layer.
    EXPECT_NE(out.text.find("IFCPRESENTATIONLAYERASSIGNMENT('Survey/Kerb',"), std::string::npos);
}

TEST(IfcExportDrawing, SelectionAndOptionsLimitWhatIsWritten)
{
    const Model model = scenario();
    auto options = mga56();
    options.layers = {"Survey/Kerb"};
    options.exportAlignments = false;
    const auto kerbOnly = exported({&model, {}, {}}, options);
    EXPECT_EQ(kerbOnly.entitiesWritten, 1u);
    EXPECT_EQ(kerbOnly.alignments, 0u);
    EXPECT_TRUE(instancesOf(kerbOnly.text, "IFCALIGNMENT").empty());

    options = mga56();
    options.exportEntities = false;
    const auto alignmentOnly = exported({&model, {}, {}}, options);
    EXPECT_EQ(alignmentOnly.entitiesWritten, 0u);
    EXPECT_EQ(alignmentOnly.alignments, 1u);
}

TEST(IfcExportDrawing, AProjectsRulesMustNameAClassTheExportWritesAndSayWhatUserDefinedIs)
{
    const Model model = scenario();
    auto options = mga56();
    options.rules = {{"x", {"KERB"}, {}, {"IfcKerbStone", {}, {}}, {}}};
    auto refused = ifc::writeIfc({&model, {}, {}}, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);

    options.rules = {{"x", {"KERB"}, {}, {"IfcKerb", "USERDEFINED", {}}, {}}};
    refused = ifc::writeIfc({&model, {}, {}}, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);

    // A proxy only when a project asks for one, by name.
    options.rules = {
        {"anything", {"SURVEY"}, {}, {"IfcBuildingElementProxy", "NOTDEFINED", {}}, {}}};
    const auto asked = exported({&model, {}, {}}, options);
    EXPECT_EQ(asked.classes.at("IfcBuildingElementProxy"), 4u);
}

// A drainage string from a .12da archive: pipes pit to pit, the pits chambers, one system.
// Pipe 1 runs 19.00 to 18.80 (flow_direction 1: upstream at the start), a
// 375 mm pipe, so its axis is 0.1875 above: 19.1875 to 18.9875.
TEST(IfcExportDrawing, AnArchivesDrainageStringIsPipesPitToPitAndItsPitsChambers)
{
    Model model;
    Entity line;
    line.geometry = Polyline2{{{1000.0, 2000.0}, {1030.0, 2000.0}, {1060.0, 2010.0}}, false};
    line.layer = "Drainage";
    line.metadata["12d.element"] = std::string("string drainage");
    line.metadata["12d.name"] = std::string("SW01");
    line.properties["flow_direction"] = 1.0;
    line.properties["pipe.1.diameter"] = 0.375;
    line.properties["pipe.1.us_level"] = 19.0;
    line.properties["pipe.1.ds_level"] = 18.8;
    line.properties["pipe.1.name"] = std::string("P1");
    line.properties["pipe.2.diameter"] = 0.375;
    line.properties["pipe.2.us_level"] = 18.75;
    line.properties["pipe.2.ds_level"] = 18.5;
    ASSERT_TRUE(model.entities.add(line).ok());
    const std::vector<std::pair<Point2, std::string>> pits{{{1000.0, 2000.0}, "KIP"},
                                                           {{1030.0, 2000.0}, "Junction pit"},
                                                           {{1060.0, 2010.0}, "Headwall"}};
    for (const auto& [at, type] : pits) {
        Entity pit;
        pit.geometry = katana::entity::PointGeometry{at};
        pit.layer = "Drainage";
        pit.metadata["12d.element"] = std::string("drainage pit");
        pit.metadata["12d.name"] = std::string("SW01");
        pit.properties["pit.type"] = type;
        pit.properties["pit.diameter"] = 1.05;
        pit.properties[std::string(katana::entity::kElevationProperty)] = 20.5;
        ASSERT_TRUE(model.entities.add(pit).ok());
    }
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcPipeSegment"), 2u);
    EXPECT_EQ(out.classes.at("IfcDistributionChamberElement"), 3u);
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 1u);
    EXPECT_NE(out.text.find(".STORMWATER.)"), std::string::npos);
    EXPECT_NE(out.text.find(".USERDEFINED.)"), std::string::npos); // the headwall
    EXPECT_NE(out.text.find("'HEADWALL'"), std::string::npos);
    EXPECT_NE(out.text.find("IFCCARTESIANPOINT((1000.,2000.,19.1875))"), std::string::npos);
    EXPECT_NE(out.text.find("IFCCARTESIANPOINT((1030.,2000.,18.9875))"), std::string::npos);

    const auto back = ifc::readIfc(out.text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    const Entity* pipe = withName(*back, "P1");
    ASSERT_NE(pipe, nullptr);
    EXPECT_EQ(property(*pipe, "Pset_PipeSegmentTypeCommon/NominalDiameter"), "0.375");
    EXPECT_EQ(property(*pipe, "Pset_PipeSegmentOccurrence/InvertElevation"), "19");
    EXPECT_EQ(property(*pipe, "System"), "SW01");
}

TEST(IfcExportSurfaces, ASurfaceIsTerrainAsATriangulatedFaceSet)
{
    auto surface = katana::terrain::TinSurface::create(
        {{0.0, 0.0, 10.0}, {10.0, 0.0, 11.0}, {0.0, 10.0, 12.0}, {10.0, 10.0, 13.0}},
        {{0, 1, 3}, {0, 3, 2}});
    ASSERT_TRUE(surface.ok()) << surface.error().describe();
    const auto out = exported({nullptr, {{"Ground", &*surface}}, {}}, {});
    EXPECT_EQ(out.surfaces, 1u);
    EXPECT_EQ(out.classes.at("IfcGeographicElement"), 1u);
    EXPECT_NE(out.text.find(".TERRAIN.)"), std::string::npos);
    ASSERT_EQ(instancesOf(out.text, "IFCTRIANGULATEDFACESET").size(), 1u);

    const auto back = ifc::readIfc(out.text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    ASSERT_EQ(back->surfaces.size(), 1u);
    EXPECT_EQ(back->surfaces[0].name, "Ground");
    EXPECT_EQ(back->surfaces[0].surface.vertexCount(), 4u);
    EXPECT_EQ(back->surfaces[0].surface.triangleCount(), 2u);
}

namespace {

Entity drainageString(std::string name, std::vector<Point2> vertices, std::size_t pipes)
{
    Entity line;
    line.geometry = Polyline2{std::move(vertices), false};
    line.layer = "Drainage";
    line.metadata["12d.element"] = std::string("string drainage");
    line.metadata["12d.name"] = std::move(name);
    for (std::size_t i = 1; i <= pipes; ++i) {
        line.properties["pipe." + std::to_string(i) + ".diameter"] = 0.375;
    }
    return line;
}

Entity drainagePit(std::string name, Point2 at)
{
    Entity pit;
    pit.geometry = katana::entity::PointGeometry{at};
    pit.layer = "Drainage";
    pit.metadata["12d.element"] = std::string("drainage pit");
    pit.metadata["12d.name"] = std::move(name);
    pit.properties["pit.type"] = std::string("Junction pit");
    return pit;
}

} // namespace

// A .12da archive names a string's pits by its header, and two strings may share a name:
// each pit is written once, with the string it stands on.
TEST(IfcExportDrawing, TwoDrainageStringsOfOneNameWriteEachPitOnce)
{
    Model model;
    ASSERT_TRUE(model.entities.add(drainageString("SW", {{0.0, 0.0}, {30.0, 0.0}}, 1)).ok());
    ASSERT_TRUE(model.entities.add(drainageString("SW", {{0.0, 50.0}, {30.0, 50.0}}, 1)).ok());
    for (const Point2 at :
         {Point2{0.0, 0.0}, Point2{30.0, 0.0}, Point2{0.0, 50.0}, Point2{30.0, 50.0}}) {
        ASSERT_TRUE(model.entities.add(drainagePit("SW", at)).ok());
    }
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcDistributionChamberElement"), 4u);
    EXPECT_EQ(out.classes.at("IfcPipeSegment"), 2u);
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 2u);
    EXPECT_EQ(out.entitiesWritten, 6u);
}

// A drainage string with nothing to draw is not written, and is accounted
// for as not written - not claimed as "one pipe along its line".
TEST(IfcExportDrawing, ADrainageStringWithNothingToDrawIsSkippedAndSaysSo)
{
    Model model;
    ASSERT_TRUE(
        model.entities.add(drainageString("SW", {{0.0, 0.0}, {0.000001, 0.0}, {0.000002, 0.0}}, 0))
            .ok());
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_FALSE(out.classes.contains("IfcPipeSegment"));
    EXPECT_FALSE(out.classes.contains("IfcDistributionSystem")); // no system of nothing
    EXPECT_EQ(out.entitiesWritten, 0u);
    EXPECT_EQ(out.entitiesSkipped, 1u);
    ASSERT_EQ(out.tally.size(), 1u);
    EXPECT_EQ(out.tally[0].entity, "");
    EXPECT_EQ(out.tally[0].why, "nothing to draw");
    EXPECT_TRUE(std::any_of(out.warnings.begin(), out.warnings.end(), [](const std::string& w) {
        return w.find("no extent to write; it is not written") != std::string::npos;
    }));
}

namespace {

// What UTILITY DRAW puts on a run and a point: the drawing's record of the
// grading (docs/subsurface_utilities.md, "What the grading found is on the
// entities"), written here by hand from that table.
Entity drawnRun(std::string layer, std::string line, std::string type, std::string level,
                std::vector<Point2> vertices, std::string configuration)
{
    Entity run;
    run.geometry = Polyline2{std::move(vertices), false};
    run.layer = std::move(layer);
    run.properties["utility.line"] = line;
    run.properties["utility.type"] = std::move(type);
    run.properties["utility.quality_level"] = std::move(level);
    run.properties["utility.from"] = line + "-1";
    run.properties["utility.to"] = line + "-2";
    run.properties["utility.length"] = 20.0;
    if (!configuration.empty()) {
        run.properties["utility.configuration"] = std::move(configuration);
    }
    return run;
}

Entity drawnPoint(std::string layer, std::string line, std::string type, std::string vertex,
                  std::string level, Point2 at)
{
    Entity point;
    point.geometry = katana::entity::PointGeometry{at};
    point.layer = std::move(layer);
    point.properties["utility.line"] = std::move(line);
    point.properties["utility.type"] = std::move(type);
    point.properties["utility.vertex"] = std::move(vertex);
    point.properties["utility.quality_level"] = std::move(level);
    point.properties["utility.method"] = std::string("EML");
    return point;
}

} // namespace

// A services plan drawn from a schedule is laid out a layer per type and
// quality level; it goes out as the services it is. Worked out from the
// schedule export's own rules (classifyUtilityRun): an electricity run whose
// configuration says conduits is an IfcCableCarrierSegment CONDUITSEGMENT, as
// the schedule's E1 is (IfcExportUtilities above), not the cable its layer's
// words would make it; water is an IfcPipeSegment RIGIDSEGMENT. One system a
// service - E1 over its QL-B and QL-C layers is one - and W1 drawn under two
// prefixes is two services. Each run and point is classified by its level in
// AS 5488.1-2019: four levels used, four references, one standard.
TEST(IfcExportDrawing, ADrawnServicesPlanGoesOutByServiceAsTheScheduleWouldClassIt)
{
    Model model;
    const std::string conduits = "4 x 100 mm conduits";
    for (Entity entity :
         {drawnRun("utilities/electricity/QL-B", "E1", "electricity", "QL-B",
                   {{0.0, 0.0}, {9.0, 0.0}}, conduits),
          drawnRun("utilities/electricity/QL-C", "E1", "electricity", "QL-C",
                   {{9.0, 0.0}, {27.0, 0.0}}, conduits),
          drawnPoint("utilities/electricity/points", "E1", "electricity", "E1-1", "QL-B",
                     {0.0, 0.0}),
          drawnPoint("utilities/electricity/points", "E1", "electricity", "E1-2", "QL-C",
                     {27.0, 0.0}),
          drawnRun("utilities/water/QL-A", "W1", "water", "QL-A", {{0.0, 5.0}, {20.0, 5.0}}, ""),
          drawnPoint("utilities/water/points", "W1", "water", "W1-1", "QL-A", {0.0, 5.0}),
          drawnPoint("utilities/water/points", "W1", "water", "W1-2", "QL-A", {20.0, 5.0}),
          drawnRun("site b/water/QL-D", "W1", "water", "QL-D", {{0.0, 50.0}, {20.0, 50.0}}, ""),
          drawnPoint("site b/water/points", "W1", "water", "W1-1", "QL-D", {0.0, 50.0})}) {
        ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
    }
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcCableCarrierSegment"), 2u);
    EXPECT_FALSE(out.classes.contains("IfcCableSegment"));
    EXPECT_EQ(out.classes.at("IfcPipeSegment"), 2u);
    EXPECT_EQ(out.classes.at("IfcAnnotation"), 5u);
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 3u);
    EXPECT_EQ(out.entitiesWritten, 9u);

    std::multiset<std::string> systems;
    for (const std::string& line : instancesOf(out.text, "IFCDISTRIBUTIONSYSTEM")) {
        const auto arguments = argumentsOf(line);
        systems.insert(arguments[2] + " " + arguments[6]);
    }
    EXPECT_EQ(systems, (std::multiset<std::string>{"'E1' .ELECTRICAL.", "'W1' .WATERSUPPLY.",
                                                   "'W1' .WATERSUPPLY."}));
    for (const std::string& line : instancesOf(out.text, "IFCCABLECARRIERSEGMENT")) {
        EXPECT_EQ(argumentsOf(line).back(), ".CONDUITSEGMENT.");
    }

    EXPECT_EQ(instancesOf(out.text, "IFCCLASSIFICATION").size(), 1u);
    std::set<std::string> levels;
    for (const std::string& line : instancesOf(out.text, "IFCCLASSIFICATIONREFERENCE")) {
        levels.insert(argumentsOf(line)[1]);
    }
    EXPECT_EQ(levels, (std::set<std::string>{"'QL-A'", "'QL-B'", "'QL-C'", "'QL-D'"}));
    EXPECT_EQ(instancesOf(out.text, "IFCRELASSOCIATESCLASSIFICATION").size(), 4u);
    // The grade in the sets the schedule's export writes: one a run, one a point.
    std::size_t grades = 0;
    std::size_t located = 0;
    for (const std::string& line : instancesOf(out.text, "IFCPROPERTYSET")) {
        grades += line.find("'AS5488_QualityLevel'") != std::string::npos ? 1 : 0;
        located += line.find("'AS5488_LocatedPoint'") != std::string::npos ? 1 : 0;
    }
    EXPECT_EQ(grades, 4u);
    EXPECT_EQ(located, 5u);

    // The preview says so, by service - named apart from the schedule's
    // "service E1", and by where it was drawn, so W1's two are two rows.
    const auto e1 = std::find_if(out.tally.begin(), out.tally.end(), [](const ifc::ClassTally& t) {
        return t.source == "drawn service E1 in utilities/electricity" &&
               t.entity == "IfcCableCarrierSegment";
    });
    ASSERT_NE(e1, out.tally.end());
    EXPECT_EQ(e1->predefinedType, "CONDUITSEGMENT");
    EXPECT_EQ(e1->system, "ELECTRICAL");
    EXPECT_EQ(e1->count, 2u);
    EXPECT_EQ(e1->why, "a drawn run: configuration \"4 x 100 mm conduits\"");
}

// What the review of the drawn plan's export found (8f101e1). Each case is a
// plan as UTILITY DRAW leaves it, then edited or drawn again as a person
// would; the class each run should be is classifyUtilityRun's for its own
// attributes, as the schedule's export gives it.

namespace {

std::size_t countOf(const std::string& text, const std::string& needle)
{
    std::size_t count = 0;
    for (std::size_t at = text.find(needle); at != std::string::npos;
         at = text.find(needle, at + 1)) {
        ++count;
    }
    return count;
}

bool warned(const ifc::IfcExport& out, const std::string& words)
{
    return std::any_of(out.warnings.begin(), out.warnings.end(), [&](const std::string& warning) {
        return warning.find(words) != std::string::npos;
    });
}

} // namespace

// Two schedules drawn under one prefix, each with an "E1": conduits in one,
// a direct-buried cable in the other. Each run is its own service's class,
// each service its own system with its own description, and the clash of
// ids is said.
TEST(IfcExportDrawing, TwoServicesDrawnWithOneLineIdStayTwoServices)
{
    Model model;
    Entity conduit = drawnRun("utilities/electricity/QL-B", "E1", "electricity", "QL-B",
                              {{0.0, 0.0}, {9.0, 0.0}}, "4 x 100 mm conduits");
    conduit.properties["utility.description"] = std::string("11 kV");
    Entity buried = drawnRun("utilities/electricity/QL-C", "E1", "electricity", "QL-C",
                             {{0.0, 40.0}, {9.0, 40.0}}, "direct buried");
    buried.properties["utility.description"] = std::string("LV street supply");
    ASSERT_TRUE(model.entities.add(conduit).ok());
    ASSERT_TRUE(model.entities.add(buried).ok());
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcCableCarrierSegment"), 1u);
    EXPECT_EQ(out.classes.at("IfcCableSegment"), 1u);
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 2u);
    std::set<std::string> described;
    for (const std::string& line : instancesOf(out.text, "IFCDISTRIBUTIONSYSTEM")) {
        described.insert(argumentsOf(line)[3]);
    }
    EXPECT_EQ(described, (std::set<std::string>{"'11 kV'", "'LV street supply'"}));
    EXPECT_TRUE(warned(out, "line E1 under utilities/electricity was drawn from services with "
                            "different attributes"));
}

// EXPLODE makes a run's segments lines that keep its layer and properties:
// they stay in their service, as the class it gives, not a second system
// classed by the layer's words.
TEST(IfcExportDrawing, AnExplodedRunStaysInItsService)
{
    Model model;
    Entity whole = drawnRun("utilities/electricity/QL-B", "E1", "electricity", "QL-B",
                            {{0.0, 0.0}, {9.0, 0.0}}, "4 x 100 mm conduits");
    Entity piece = whole;
    piece.geometry = katana::geometry::Segment2{{9.0, 0.0}, {27.0, 0.0}};
    piece.layer = "utilities/electricity/QL-C";
    piece.properties["utility.quality_level"] = std::string("QL-C");
    ASSERT_TRUE(model.entities.add(whole).ok());
    ASSERT_TRUE(model.entities.add(piece).ok());
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcCableCarrierSegment"), 2u);
    EXPECT_FALSE(out.classes.contains("IfcCableSegment"));
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 1u);
}

// A located point moved to another layer - an ordinary edit - still belongs
// to the service it stands on, and one at the service's level is written in
// 3D there, as the schedule's export places its points.
TEST(IfcExportDrawing, APointMovedToAnotherLayerStaysWithTheRunItStandsOn)
{
    Model model;
    ASSERT_TRUE(model.entities
                    .add(drawnRun("utilities/water/QL-A", "W1", "water", "QL-A",
                                  {{0.0, 5.0}, {20.0, 5.0}}, ""))
                    .ok());
    Entity moved = drawnPoint("survey/checked", "W1", "water", "W1-2", "QL-A", {20.0, 5.0});
    moved.properties["utility.service_level"] = 19.3;
    ASSERT_TRUE(model.entities.add(moved).ok());
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcDistributionSystem"), 1u);
    const auto back = ifc::readIfc(out.text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    const Entity* point = withName(*back, "W1-2");
    ASSERT_NE(point, nullptr);
    const auto heights = katana::entity::heightsOf(point->properties, 1);
    ASSERT_TRUE(heights[0]);
    EXPECT_NEAR(*heights[0], 19.3, 1e-9);
    // The derived level is ServiceLevel: the schedule export's Level is a
    // recorded one, which the drawing does not keep.
    EXPECT_EQ(property(*point, "AS5488_LocatedPoint/ServiceLevel"), "19.3");
}

// A project's own rule still names the class of what it drew; the defaults
// give way to the service's class, a project's rules do not.
TEST(IfcExportDrawing, AProjectsRuleStillClassesADrawnRun)
{
    Model model;
    ASSERT_TRUE(
        model.entities
            .add(drawnRun("utilities/gas/QL-D", "G1", "gas", "QL-D", {{0.0, 0.0}, {40.0, 0.0}}, ""))
            .ok());
    const auto rules = ifc::parseClassificationRules(
        "rule,words,kinds,class,predefined_type,object_type,system\n"
        "gas flexible,GAS,Polyline,IfcPipeSegment,FLEXIBLESEGMENT,,GAS\n");
    ASSERT_TRUE(rules.ok()) << rules.error().describe();
    ifc::ExportOptions options;
    options.rules = *rules;
    const auto& defaults = ifc::defaultClassificationRules();
    options.rules.insert(options.rules.end(), defaults.begin(), defaults.end());
    const auto out = exported({&model, {}, {}}, options);
    const auto pipes = instancesOf(out.text, "IFCPIPESEGMENT");
    ASSERT_EQ(pipes.size(), 1u);
    EXPECT_EQ(argumentsOf(pipes[0]).back(), ".FLEXIBLESEGMENT.");
    // Without the project's rule, the service's own class.
    const auto plain = exported({&model, {}, {}}, {});
    ASSERT_EQ(instancesOf(plain.text, "IFCPIPESEGMENT").size(), 1u);
    EXPECT_EQ(argumentsOf(instancesOf(plain.text, "IFCPIPESEGMENT")[0]).back(), ".RIGIDSEGMENT.");
}

// A level AS 5488 does not have, or none, is not written as the standard's:
// it is said, and the run goes out unclassified.
TEST(IfcExportDrawing, ALevelTheStandardDoesNotHaveIsSaidAndNotClassified)
{
    Model model;
    ASSERT_TRUE(model.entities
                    .add(drawnRun("utilities/water/QL-B", "W1", "water", "QL-E",
                                  {{0.0, 0.0}, {20.0, 0.0}}, ""))
                    .ok());
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_EQ(out.classes.at("IfcPipeSegment"), 1u);
    EXPECT_TRUE(instancesOf(out.text, "IFCCLASSIFICATIONREFERENCE").empty());
    // Not AS 5488's QualityLevel; the entity's own record keeps what it
    // says (Katana_Attributes), as every entity's does.
    EXPECT_EQ(countOf(out.text, "'QualityLevel'"), 0u);
    EXPECT_EQ(countOf(out.text, "'QL-E'"), 1u);
    EXPECT_NE(out.text.find("IFCPROPERTYSINGLEVALUE('utility.quality_level',$,IFCLABEL('QL-E')"),
              std::string::npos);
    EXPECT_TRUE(warned(out, "has quality level \"QL-E\", which is not one of AS 5488.1-2019's"));
}

// The plan drawn from a schedule and the schedule itself in one file is the
// service twice: written, as asked, and said.
TEST(IfcExportDrawing, AServiceBothDrawnAndGivenAsTheScheduleIsSaid)
{
    Model model;
    ASSERT_TRUE(model.entities
                    .add(drawnRun("utilities/water/QL-B", "W1", "water", "QL-B",
                                  {{334000.0, 6250000.0}, {334010.0, 6250000.0}}, ""))
                    .ok());
    const auto out = exported({&model, {}, sampleSchedule()}, mga56());
    EXPECT_TRUE(warned(out, "drawn service W1 in utilities/water is also in the schedule given "
                            "with UTILITIES"));
}

// A name longer than IFC's 255 characters is cut there, between characters,
// and the cut is said.
TEST(IfcExport, ANameLongerThanIfcAllowsIsCutAndSaid)
{
    Model model;
    Entity kerb;
    kerb.geometry = Polyline2{{Point2{0.0, 0.0}, Point2{10.0, 0.0}}, false};
    kerb.layer = "Survey/Kerb";
    // 300 characters, the 255th a two-byte one: the cut must not split it.
    std::string name(254, 'k');
    name += "\xC3\xA9"; // é
    name += std::string(45, 'k');
    kerb.properties["point"] = name;
    ASSERT_TRUE(model.entities.add(kerb).ok());
    const auto out = exported({&model, {}, {}}, {});
    EXPECT_NE(out.text.find("'" + std::string(254, 'k') + "\\X2\\00E9\\X0\\'"), std::string::npos);
    EXPECT_TRUE(warned(out, "its name is longer than the 255 characters IFC allows"));
}
