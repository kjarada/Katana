// The drawing of a graded utility schedule (utility_drawing.hpp), built with
// no document: what layers, linetypes and entities UTILITY DRAW would add.
//
// The sample is samples/utilities/schedule.csv, graded by hand in
// docs/subsurface_utilities.md and by the report the cli.utility_* tests pin:
// W1's segments grade QL-B, QL-B, QL-B, QL-A, QL-C - three detected, the
// trench between the potholes, then 12.5 m interpolated past the detected
// spacing - so W1 is three polylines, and a polyline ends exactly where the
// level changes.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <fstream>
#include <iterator>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/plot.hpp"
#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"

using namespace katana::cad::utilities;
namespace sub = katana::survey::subsurface;
using katana::entity::Entity;
using katana::entity::EntityType;

namespace {

std::string sampleText(const std::string& name)
{
    std::ifstream file(std::string(KATANA_UTILITY_SAMPLES) + "/" + name, std::ios::binary);
    EXPECT_TRUE(file.good()) << name;
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

std::vector<sub::UtilityLine> parsed(std::string_view text)
{
    auto lines = sub::parseUtilityCsv(text);
    EXPECT_TRUE(lines.ok()) << lines.error().describe();
    return lines.ok() ? std::move(lines).value() : std::vector<sub::UtilityLine>{};
}

std::vector<sub::UtilityLine> sample()
{
    return parsed(sampleText("schedule.csv"));
}

UtilityDrawing drawn(const std::vector<sub::UtilityLine>& lines,
                     const UtilityDrawOptions& options = {})
{
    auto drawing = drawUtilities(lines, options);
    EXPECT_TRUE(drawing.ok()) << drawing.error().describe();
    return drawing.ok() ? std::move(drawing).value() : UtilityDrawing{};
}

std::string text(const Entity& entity, std::string_view key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? std::string("(absent)")
                                            : katana::entity::toString(found->second);
}

double real(const Entity& entity, std::string_view key)
{
    const auto found = entity.properties.find(key);
    if (found == entity.properties.end() || !std::holds_alternative<double>(found->second)) {
        ADD_FAILURE() << key << " is not a real";
        return NAN;
    }
    return std::get<double>(found->second);
}

bool has(const Entity& entity, std::string_view key)
{
    return entity.properties.contains(key);
}

// The entities of one service of one kind, in the order they were built.
std::vector<Entity> of(const UtilityDrawing& drawing, const std::string& line, EntityType type)
{
    std::vector<Entity> out;
    for (const Entity& entity : drawing.entities) {
        if (entity.type() == type && text(entity, keys::kLine) == line) {
            out.push_back(entity);
        }
    }
    return out;
}

const Entity& point(const UtilityDrawing& drawing, const std::string& vertex)
{
    for (const Entity& entity : drawing.entities) {
        if (entity.type() == EntityType::Point && text(entity, keys::kVertex) == vertex) {
            return entity;
        }
    }
    ADD_FAILURE() << "no point " << vertex;
    static const Entity none;
    return none;
}

std::vector<std::string> layerNames(const UtilityDrawing& drawing)
{
    std::vector<std::string> names;
    for (const katana::entity::Layer& layer : drawing.layers) {
        names.push_back(layer.name);
    }
    return names;
}

const katana::entity::Layer& layerOf(const UtilityDrawing& drawing, const std::string& name)
{
    for (const katana::entity::Layer& layer : drawing.layers) {
        if (layer.name == name) {
            return layer;
        }
    }
    ADD_FAILURE() << "no layer " << name;
    static const katana::entity::Layer none;
    return none;
}

std::size_t vertexCount(const Entity& polyline)
{
    return std::get<katana::geometry::Polyline2>(polyline.geometry).vertices.size();
}

// The WCAG contrast of two colours: 1 (none) to 21 (black on white). 3 is
// what a line or a symbol needs to be seen.
double luminance(katana::entity::Color colour)
{
    const auto linear = [](std::uint8_t channel) {
        const double c = channel / 255.0;
        return c <= 0.03928 ? c / 12.92 : std::pow((c + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b);
}

double contrast(katana::entity::Color a, katana::entity::Color b)
{
    const double la = luminance(a);
    const double lb = luminance(b);
    return (std::max(la, lb) + 0.05) / (std::min(la, lb) + 0.05);
}

const std::vector<sub::UtilityType>& everyType()
{
    static const std::vector<sub::UtilityType> types{
        sub::UtilityType::Unknown,       sub::UtilityType::Electricity,
        sub::UtilityType::Telecommunications, sub::UtilityType::Gas,
        sub::UtilityType::Water,         sub::UtilityType::RecycledWater,
        sub::UtilityType::FireService,   sub::UtilityType::Sewer,
        sub::UtilityType::Stormwater,    sub::UtilityType::Fuel,
        sub::UtilityType::IntelligentTransport, sub::UtilityType::Other};
    return types;
}

// A service located across a pothole whose two records share one place: the
// trench between them is QL-A and has no length.
constexpr std::string_view kSharedPlace =
    "line,point,easting,northing,method,level,level_ref,surface,h_unc,v_unc,path,type\n"
    "S1,S1-1,100.0,200.0,EML,,,,0.10,,,sewer\n"
    "S1,S1-2,105.0,200.0,pothole,19.0,top,20.0,0.02,0.02,exposed,\n"
    "S1,S1-3,105.0,200.0,pothole,19.0,top,20.0,0.02,0.02,,\n"
    "S1,S1-4,110.0,200.0,EML,,,,0.10,,,\n";

// The same pothole with its two records a rounding error apart, as a
// reprojected export leaves them: 10 nm, a length, but under the model's
// tolerance for one.
constexpr std::string_view kNearlyOnePlace =
    "line,point,easting,northing,method,level,level_ref,surface,h_unc,v_unc,path,type\n"
    "S1,S1-1,100.0,200.0,EML,,,,0.10,,,sewer\n"
    "S1,S1-2,105.0,200.0,pothole,19.0,top,20.0,0.02,0.02,exposed,\n"
    "S1,S1-3,105.00000001,200.0,pothole,19.0,top,20.0,0.02,0.02,,\n"
    "S1,S1-4,110.0,200.0,EML,,,,0.10,,,\n";

} // namespace

TEST(UtilityDrawing, TheSampleIsDrawnAsRunsAndPoints)
{
    const UtilityDrawing drawing = drawn(sample());
    EXPECT_EQ(drawing.lines.size(), 4u);
    EXPECT_EQ(drawing.vertices, 14u);
    EXPECT_EQ(drawing.segments, 10u);
    // W1 three runs, E1 two, T1 and G1 one each; a point per vertex.
    EXPECT_EQ(drawing.entities.size(), 7u + 14u);
    EXPECT_EQ(drawing.drawnLayers, 11u);
    EXPECT_DOUBLE_EQ(drawing.bounds.min.x, 334000.0);
    EXPECT_DOUBLE_EQ(drawing.bounds.min.y, 6250000.0);
    EXPECT_DOUBLE_EQ(drawing.bounds.max.x, 334040.0);
    EXPECT_DOUBLE_EQ(drawing.bounds.max.y, 6250007.2);
    EXPECT_EQ(formatUtilityDrawing(drawing),
              "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 "
              "bounds=334000.000,6250000.000,334040.000,6250007.200\n"
              "line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ql_d=0.000\n"
              "line id=E1 type=electricity length=27.001 ql_a=0.000 ql_b=9.001 ql_c=18.001 "
              "ql_d=0.000\n"
              "line id=T1 type=telecommunications length=33.000 ql_a=0.000 ql_b=0.000 "
              "ql_c=33.000 ql_d=0.000\n"
              "line id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 ql_c=0.000 ql_d=40.000");
}

TEST(UtilityDrawing, APolylineEndsExactlyWhereTheGradedLevelChanges)
{
    const UtilityDrawing drawing = drawn(sample());
    const std::vector<Entity> runs = of(drawing, "W1", EntityType::Polyline);
    ASSERT_EQ(runs.size(), 3u);
    EXPECT_EQ(text(runs[0], keys::kQualityLevel), "QL-B");
    EXPECT_EQ(text(runs[0], keys::kFrom), "W1-1");
    EXPECT_EQ(text(runs[0], keys::kTo), "W1-X1");
    EXPECT_EQ(vertexCount(runs[0]), 4u);
    EXPECT_EQ(runs[0].layer, "utilities/water/QL-B");
    EXPECT_EQ(text(runs[1], keys::kQualityLevel), "QL-A");
    EXPECT_EQ(text(runs[1], keys::kFrom), "W1-X1");
    EXPECT_EQ(text(runs[1], keys::kTo), "W1-4");
    EXPECT_EQ(vertexCount(runs[1]), 2u);
    EXPECT_EQ(runs[1].layer, "utilities/water/QL-A");
    EXPECT_EQ(text(runs[2], keys::kQualityLevel), "QL-C");
    EXPECT_EQ(text(runs[2], keys::kFrom), "W1-4");
    EXPECT_EQ(text(runs[2], keys::kTo), "W1-5");
    EXPECT_EQ(runs[2].layer, "utilities/water/QL-C");
    // The runs meet: each starts where the one before it ended.
    const auto& first = std::get<katana::geometry::Polyline2>(runs[0].geometry);
    const auto& second = std::get<katana::geometry::Polyline2>(runs[1].geometry);
    EXPECT_EQ(first.vertices.back(), second.vertices.front());
    EXPECT_DOUBLE_EQ(second.vertices.front().x, 334016.080);
    EXPECT_DOUBLE_EQ(second.vertices.front().y, 6250000.310);
    // E1's weak radar pick makes its last two segments one QL-C run.
    const std::vector<Entity> duct = of(drawing, "E1", EntityType::Polyline);
    ASSERT_EQ(duct.size(), 2u);
    EXPECT_EQ(text(duct[1], keys::kQualityLevel), "QL-C");
    EXPECT_EQ(vertexCount(duct[1]), 3u);
}

TEST(UtilityDrawing, TheLayersAreByTypeAndLevelParentsFirst)
{
    const UtilityDrawing drawing = drawn(sample());
    const std::vector<std::string> expected{
        "utilities",
        "utilities/electricity",
        "utilities/electricity/QL-B",
        "utilities/electricity/QL-C",
        "utilities/electricity/points",
        "utilities/gas",
        "utilities/gas/QL-D",
        "utilities/gas/points",
        "utilities/telecommunications",
        "utilities/telecommunications/QL-C",
        "utilities/telecommunications/points",
        "utilities/water",
        "utilities/water/QL-A",
        "utilities/water/QL-B",
        "utilities/water/QL-C",
        "utilities/water/points"};
    EXPECT_EQ(layerNames(drawing), expected);
    // Colour by type, linetype by level; the group of a type in its colour.
    const auto water = utilityTypeColour(sub::UtilityType::Water);
    EXPECT_EQ(layerOf(drawing, "utilities/water").color, water);
    EXPECT_EQ(layerOf(drawing, "utilities/water/QL-A").color, water);
    EXPECT_EQ(layerOf(drawing, "utilities/water/QL-A").linetype, "continuous");
    EXPECT_EQ(layerOf(drawing, "utilities/water/QL-B").linetype, "utility-ql-b");
    EXPECT_EQ(layerOf(drawing, "utilities/water/QL-C").linetype, "utility-ql-c");
    EXPECT_EQ(layerOf(drawing, "utilities/gas/QL-D").linetype, "utility-ql-d");
    EXPECT_EQ(layerOf(drawing, "utilities/gas/QL-D").color,
              utilityTypeColour(sub::UtilityType::Gas));
    EXPECT_EQ(layerOf(drawing, "utilities/water/points").linetype, "continuous");
    EXPECT_EQ(layerOf(drawing, "utilities").color, katana::entity::Layer{}.color);
    // Only the levels drawn get a linetype, and QL-A needs none of its own.
    ASSERT_EQ(drawing.linetypes.size(), 3u);
    EXPECT_EQ(drawing.linetypes[0].name, "utility-ql-b");
    EXPECT_EQ(drawing.linetypes[1].name, "utility-ql-c");
    EXPECT_EQ(drawing.linetypes[2].name, "utility-ql-d");
    // The entities are ByLayer: the layer is where a person changes them.
    for (const Entity& entity : drawing.entities) {
        EXPECT_FALSE(entity.color.has_value());
        EXPECT_TRUE(entity.style.empty());
    }
}

TEST(UtilityDrawing, EachQualityLevelHasADistinctLinetypeInModelMetres)
{
    EXPECT_EQ(qualityLevelLinetypeName(sub::QualityLevel::A), "continuous");
    EXPECT_TRUE(qualityLevelLinetype(sub::QualityLevel::A).isContinuous());
    std::set<std::vector<double>> patterns;
    for (const sub::QualityLevel level :
         {sub::QualityLevel::B, sub::QualityLevel::C, sub::QualityLevel::D}) {
        const katana::entity::Linetype linetype = qualityLevelLinetype(level);
        EXPECT_EQ(linetype.name, qualityLevelLinetypeName(level));
        EXPECT_TRUE(katana::entity::validate(linetype).ok()) << linetype.name;
        EXPECT_FALSE(linetype.isContinuous());
        // A period a person can read at 1:200 to 1:500: between 1 and 5 mm
        // of paper at 1:500 (0.5 to 2.5 m), and so 2.5 to 12.5 mm at 1:200.
        EXPECT_GE(linetype.patternLength(), 0.5) << linetype.name;
        EXPECT_LE(linetype.patternLength(), 2.5) << linetype.name;
        std::vector<double> lengths;
        for (const auto& element : linetype.pattern) {
            lengths.push_back(element.length);
        }
        patterns.insert(lengths);
    }
    EXPECT_EQ(patterns.size(), 3u);
    // Dashed, then with a dot, then only dots, as the evidence weakens.
    const auto dots = [](sub::QualityLevel level) {
        const auto pattern = qualityLevelLinetype(level).pattern;
        return std::ranges::count_if(pattern, [](const auto& e) { return e.isDot(); });
    };
    EXPECT_EQ(dots(sub::QualityLevel::B), 0);
    EXPECT_EQ(dots(sub::QualityLevel::C), 1);
    EXPECT_EQ(qualityLevelLinetype(sub::QualityLevel::D).pattern.front().isDot(), true);
}

TEST(UtilityDrawing, EveryTypeColourReadsOnTheScreenAndOnPaper)
{
    // The plan view's ground (viewport_widget.cpp) and a colour plot on white.
    const katana::entity::Color screen{0x1E, 0x23, 0x29, 255};
    const katana::entity::Color paper{255, 255, 255, 255};
    const katana::cad::PlotSettings plot;
    std::set<std::string> words;
    std::set<std::string> colours;
    for (const sub::UtilityType type : everyType()) {
        const std::string word(utilityTypeWord(type));
        words.insert(word);
        EXPECT_EQ(word.find(' '), std::string::npos) << word;
        const katana::entity::Color colour = utilityTypeColour(type);
        colours.insert(colour.toHex());
        EXPECT_GE(contrast(colour, screen), 3.0) << word << " " << colour.toHex();
        EXPECT_GE(contrast(katana::cad::paperColour(colour, plot), paper), 3.0)
            << word << " " << colour.toHex();
    }
    EXPECT_EQ(words.size(), everyType().size());
    EXPECT_EQ(colours.size(), everyType().size());
    EXPECT_EQ(utilityTypeWord(sub::UtilityType::RecycledWater), "recycled-water");
    EXPECT_EQ(utilityTypeWord(sub::UtilityType::FireService), "fire-service");
    EXPECT_EQ(utilityTypeWord(sub::UtilityType::IntelligentTransport), "its");
    EXPECT_EQ(qualityLevelLayerName("utilities", sub::UtilityType::RecycledWater,
                                    sub::QualityLevel::B),
              "utilities/recycled-water/QL-B");
    EXPECT_EQ(pointsLayerName("utilities", sub::UtilityType::Unknown), "utilities/unknown/points");
}

TEST(UtilityDrawing, TheRunsCarryTheServiceAndWhyTheyAreLimited)
{
    const UtilityDrawing drawing = drawn(sample());
    const std::vector<Entity> runs = of(drawing, "W1", EntityType::Polyline);
    ASSERT_EQ(runs.size(), 3u);
    for (const Entity& run : runs) {
        EXPECT_EQ(text(run, keys::kType), "water");
        EXPECT_EQ(text(run, keys::kOwner), "WaterCo");
        EXPECT_EQ(text(run, keys::kMaterial), "DICL");
        EXPECT_DOUBLE_EQ(real(run, keys::kDiameter), 0.150);
        EXPECT_EQ(text(run, keys::kStatus), "in service");
        EXPECT_EQ(text(run, keys::kDescription), "trunk main");
    }
    EXPECT_EQ(text(runs[0], keys::kLimitedBy), "the QL-B vertex W1-3");
    EXPECT_FALSE(has(runs[1], keys::kLimitedBy));
    EXPECT_NE(text(runs[2], keys::kLimitedBy).find("longer than the maximum detected spacing"),
              std::string::npos);
    EXPECT_NEAR(real(runs[0], keys::kLength), 16.102, 0.0005);
    EXPECT_NEAR(real(runs[1], keys::kLength), 1.420, 0.0005);
    EXPECT_NEAR(real(runs[2], keys::kLength), 12.502, 0.0005);
    // Not recorded is not there: T1 has no material or size.
    const std::vector<Entity> telco = of(drawing, "T1", EntityType::Polyline);
    ASSERT_EQ(telco.size(), 1u);
    EXPECT_FALSE(has(telco[0], keys::kMaterial));
    EXPECT_FALSE(has(telco[0], keys::kDiameter));
    EXPECT_EQ(text(telco[0], keys::kConfiguration), "pit to pit");
    // Nor a status: a schedule with none gives no utility.status, where
    // "unknown" would read as if someone had recorded it.
    const std::vector<Entity> unrecorded = of(drawn(parsed(kSharedPlace)), "S1",
                                              EntityType::Polyline);
    ASSERT_FALSE(unrecorded.empty());
    for (const Entity& run : unrecorded) {
        EXPECT_FALSE(has(run, keys::kStatus)) << text(run, keys::kStatus);
        EXPECT_EQ(text(run, keys::kType), "sewer");
    }
}

TEST(UtilityDrawing, ThePointsCarryWhatTheGradingFoundAtEachVertex)
{
    UtilityDrawOptions options;
    options.minimumCover = 0.85;
    const UtilityDrawing drawing = drawn(sample(), options);
    ASSERT_EQ(of(drawing, "E1", EntityType::Point).size(), 4u);

    // The weak radar pick: claimed QL-B, graded QL-C, its level not qualified.
    const Entity& pick = point(drawing, "E1-3");
    EXPECT_EQ(pick.layer, "utilities/electricity/points");
    EXPECT_EQ(text(pick, keys::kLine), "E1");
    EXPECT_EQ(text(pick, keys::kMethod), "ground penetrating radar");
    EXPECT_EQ(text(pick, keys::kQualityLevel), "QL-C");
    EXPECT_EQ(text(pick, keys::kClaimed), "QL-B");
    EXPECT_TRUE(has(pick, keys::kOverClaim));
    EXPECT_DOUBLE_EQ(real(pick, keys::kServiceLevel), 19.40);
    EXPECT_EQ(text(pick, keys::kLevelReference), "top");
    EXPECT_EQ(text(pick, keys::kLevelQualified), "false");
    EXPECT_DOUBLE_EQ(real(pick, keys::kSurfaceLevel), 20.30);
    EXPECT_NEAR(real(pick, keys::kCover), 0.900, 1e-9);
    EXPECT_TRUE(has(pick, keys::kCoverNote));
    EXPECT_EQ(text(pick, keys::kCoverBelowMinimum), "false");
    const auto& at = std::get<katana::entity::PointGeometry>(pick.geometry).position;
    EXPECT_DOUBLE_EQ(at.x, 334018.0);
    EXPECT_DOUBLE_EQ(at.y, 6250003.1);

    // 0.800 of cover against 0.85 asked for.
    EXPECT_EQ(text(point(drawing, "E1-1"), keys::kCoverBelowMinimum), "true");
    EXPECT_EQ(text(point(drawing, "W1-2"), keys::kCoverBelowMinimum), "false");
    // A pothole that checks a detection says which.
    EXPECT_EQ(text(point(drawing, "W1-X1"), keys::kVerifies), "W1-3");
    EXPECT_EQ(text(point(drawing, "W1-X1"), keys::kQualityLevel), "QL-A");
    EXPECT_EQ(text(point(drawing, "W1-X1"), keys::kOverClaim), "(absent)");
    // No level: no service level, no cover, nothing below any minimum - but
    // the reason the cover is missing.
    const Entity& plain = point(drawing, "W1-1");
    EXPECT_FALSE(has(plain, keys::kServiceLevel));
    EXPECT_FALSE(has(plain, keys::kCover));
    EXPECT_FALSE(has(plain, keys::kCoverBelowMinimum));
    EXPECT_FALSE(has(plain, keys::kLevelQualified));
    EXPECT_EQ(text(plain, keys::kCoverNote), "no level or depth of the service");

    // Without a minimum no point says whether it is below one.
    const UtilityDrawing unasked = drawn(sample());
    EXPECT_FALSE(has(point(unasked, "E1-1"), keys::kCoverBelowMinimum));
    EXPECT_TRUE(has(point(unasked, "E1-1"), keys::kCover));

    // A level given with no reference is on the top, as the grading and the
    // cover take it - the one reading the parsed schedule keeps.
    const UtilityDrawing unreferenced =
        drawn(parsed("line,point,easting,northing,method,level,surface,h_unc,type\n"
                     "S2,S2-1,0,0,pothole,19.0,20.0,0.02,sewer\n"
                     "S2,S2-2,5,0,pothole,19.1,20.1,0.02,\n"));
    EXPECT_EQ(text(point(unreferenced, "S2-1"), keys::kLevelReference), "top");
    EXPECT_NEAR(real(point(unreferenced, "S2-1"), keys::kCover), 1.0, 1e-9);
}

TEST(UtilityDrawing, EachPointCarriesItsWholeScheduleRowSoThePointsAreTheSchedule)
{
    // W1-2: "W1,W1-2,334008.000,6250000.150,EML,19.20,centre,20.25,0.10,0.35,QL-B"
    // on a line whose path column says the pothole's stretch is exposed.
    const UtilityDrawing drawing = drawn(sample());
    const Entity& w12 = point(drawing, "W1-2");
    EXPECT_EQ(text(w12, keys::kOrder), "2");
    EXPECT_DOUBLE_EQ(real(w12, keys::kLevel), 19.20);
    EXPECT_EQ(text(w12, keys::kLevelReference), "centre");
    EXPECT_DOUBLE_EQ(real(w12, keys::kSurfaceLevel), 20.25);
    EXPECT_DOUBLE_EQ(real(w12, keys::kHorizontalUncertainty), 0.10);
    EXPECT_DOUBLE_EQ(real(w12, keys::kVerticalUncertainty), 0.35);
    EXPECT_EQ(text(w12, keys::kClaimed), "QL-B");
    EXPECT_EQ(text(w12, keys::kMethod), "electromagnetic location");
    EXPECT_FALSE(has(w12, keys::kDepth));
    // Its line's attributes, as the runs carry them.
    EXPECT_EQ(text(w12, keys::kType), "water");
    EXPECT_EQ(text(w12, keys::kOwner), "WaterCo");
    EXPECT_EQ(text(w12, keys::kMaterial), "DICL");
    EXPECT_DOUBLE_EQ(real(w12, keys::kDiameter), 0.150);
    EXPECT_EQ(text(w12, keys::kDiameterInside), "false");
    EXPECT_EQ(text(w12, keys::kStatus), "in service");
    EXPECT_EQ(text(w12, keys::kDescription), "trunk main");
    // A path is to the next point: given for one stretch of W1, so on every
    // point of it but the last, the rest detected.
    EXPECT_EQ(text(w12, keys::kPath), "detected");
    EXPECT_EQ(text(point(drawing, "W1-X1"), keys::kPath), "exposed");
    EXPECT_FALSE(has(point(drawing, "W1-5"), keys::kPath));
    EXPECT_EQ(text(point(drawing, "W1-5"), keys::kOrder), "6");
    // A line with no path column value has none: every stretch detected.
    EXPECT_FALSE(has(point(drawing, "E1-1"), keys::kPath));
    EXPECT_EQ(text(point(drawing, "T1-P1"), keys::kPath), "assumed");
    // No level and no depth: no reference, since there is nothing it is of.
    EXPECT_FALSE(has(point(drawing, "T1-P1"), keys::kLevelReference));

    // A depth and no level, as a delivery schema records it: the depth, its
    // reference, and the service level the surface makes of it - and the
    // schema's own fields, the point's and its line's.
    const UtilityDrawing tfnsw = drawn(parsed(sampleText("schedule_tfnsw.csv")));
    const Entity& w1 = point(tfnsw, "W1");
    EXPECT_FALSE(has(w1, keys::kLevel));
    EXPECT_DOUBLE_EQ(real(w1, keys::kDepth), 0.95);
    EXPECT_EQ(text(w1, keys::kLevelReference), "top");
    EXPECT_NEAR(real(w1, keys::kServiceLevel), 20.30 - 0.95, 1e-9);
    EXPECT_DOUBLE_EQ(real(w1, keys::kDiameter), 0.150);
    EXPECT_EQ(text(w1, keys::kDiameterInside), "true");
    EXPECT_EQ(text(w1, std::string(keys::kFieldPrefix) + "DateInfoObtained"), "2026/09/20");
    EXPECT_EQ(text(w1, std::string(keys::kFieldPrefix) + "AssetType"), "Water");
    // Recorded as unknown, the reference is kept although there is no level.
    EXPECT_EQ(text(point(tfnsw, "G1"), keys::kLevelReference), "unknown");
}

TEST(UtilityDrawing, SpacingChangesTheGrading)
{
    UtilityDrawOptions options;
    options.grading.maximumDetectedSpacing = 20.0;
    const UtilityDrawing drawing = drawn(sample(), options);
    // W1-4 -> W1-5, 12.5 m, is traced now: QL-B, and still its own run,
    // since the QL-A trench lies between it and the other QL-B stretch.
    const std::vector<Entity> runs = of(drawing, "W1", EntityType::Polyline);
    ASSERT_EQ(runs.size(), 3u);
    EXPECT_EQ(text(runs[2], keys::kQualityLevel), "QL-B");
    EXPECT_EQ(runs[2].layer, "utilities/water/QL-B");
    EXPECT_NEAR(drawing.lines[0].lengthAt[static_cast<std::size_t>(sub::QualityLevel::C)], 0.0,
                1e-9);
    // W1 has nothing left at QL-C, so it has no QL-C layer; E1's weak pick
    // is a vertex, not a spacing, and keeps its own.
    const std::vector<std::string> names = layerNames(drawing);
    EXPECT_EQ(std::ranges::count(names, std::string("utilities/water/QL-C")), 0);
    EXPECT_EQ(std::ranges::count(names, std::string("utilities/electricity/QL-C")), 1);
    EXPECT_EQ(formatUtilityDrawing(drawing).substr(0, formatUtilityDrawing(drawing).find('\n')),
              "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=10 "
              "bounds=334000.000,6250000.000,334040.000,6250007.200");
}

TEST(UtilityDrawing, ALineThatCannotBeGradedRefusesTheWholeDrawing)
{
    const std::vector<sub::UtilityLine> lines = parsed(
        "line,point,easting,northing,method,h_unc,type\n"
        "W1,W1-1,0,0,EML,0.1,water\n"
        "W1,W1-2,10,0,EML,0.1,\n"
        "W9,W9-1,5,5,EML,0.1,water\n");
    ASSERT_EQ(lines.size(), 2u);
    const auto drawing = drawUtilities(lines);
    ASSERT_FALSE(drawing.ok());
    EXPECT_EQ(drawing.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_NE(drawing.error().message.find("line W9 cannot be graded"), std::string::npos)
        << drawing.error().describe();
    EXPECT_NE(drawing.error().message.find("at least two vertices"), std::string::npos);
    EXPECT_NE(drawing.error().message.find("nothing was drawn"), std::string::npos);

    EXPECT_FALSE(drawUtilities({}).ok());
}

TEST(UtilityDrawing, ALayerPrefixPlacesEveryLayerUnderIt)
{
    UtilityDrawOptions options;
    options.layerPrefix = "Survey/Services";
    const UtilityDrawing drawing = drawn(sample(), options);
    const std::vector<std::string> names = layerNames(drawing);
    ASSERT_GE(names.size(), 3u);
    EXPECT_EQ(names[0], "Survey");
    EXPECT_EQ(names[1], "Survey/Services");
    EXPECT_EQ(names[2], "Survey/Services/electricity");
    for (const Entity& entity : drawing.entities) {
        EXPECT_TRUE(katana::entity::isLayerUnder(entity.layer, "Survey/Services")) << entity.layer;
    }
    options.layerPrefix = "a//b";
    const auto refused = drawUtilities(sample(), options);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("not a layer path"), std::string::npos);

    // A prefix that is a layer path but leaves no room for the two levels
    // drawn under it is refused here, by the prefix - not by the model once
    // the drawing's layers are being made. Fifteen levels are seventeen with
    // the type and the quality level; the model takes sixteen.
    options.layerPrefix = "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o";
    const auto deep = drawUtilities(sample(), options);
    ASSERT_FALSE(deep.ok());
    EXPECT_EQ(deep.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_NE(deep.error().message.find("the layer prefix leaves no room"), std::string::npos)
        << deep.error().describe();
    EXPECT_NE(deep.error().message.find("nested too deeply (17 > 16)"), std::string::npos);
    EXPECT_EQ(deep.error().context, options.layerPrefix);
    // 490 characters take "/water/QL-B", but not "/telecommunications/QL-C",
    // which makes 514 of the model's 512.
    options.layerPrefix = std::string(490, 'p');
    const auto wide = drawUtilities(sample(), options);
    ASSERT_FALSE(wide.ok());
    EXPECT_NE(wide.error().message.find("/telecommunications/QL-C - layer name is too long"),
              std::string::npos)
        << wide.error().describe();
}

TEST(UtilityDrawing, ARunOfNoLengthIsNotDrawnButItsPointsAre)
{
    const UtilityDrawing drawing = drawn(parsed(kSharedPlace));
    EXPECT_EQ(drawing.segments, 3u);
    const std::vector<Entity> runs = of(drawing, "S1", EntityType::Polyline);
    ASSERT_EQ(runs.size(), 2u);
    EXPECT_EQ(text(runs[0], keys::kQualityLevel), "QL-B");
    EXPECT_EQ(text(runs[1], keys::kQualityLevel), "QL-B");
    EXPECT_EQ(of(drawing, "S1", EntityType::Point).size(), 4u);
    EXPECT_EQ(drawing.lines[0].polylines, 2u);
    // No QL-A layer: nothing is on it.
    EXPECT_EQ(std::ranges::count(layerNames(drawing), std::string("utilities/sewer/QL-A")), 0);
}

TEST(UtilityDrawing, ARunARoundingErrorLongIsNotDrawnEither)
{
    // Longer than nothing, shorter than the model's tolerance: were it drawn,
    // the model would refuse it, and with it the whole draw.
    const UtilityDrawing drawing = drawn(parsed(kNearlyOnePlace));
    EXPECT_EQ(drawing.segments, 3u);
    EXPECT_EQ(of(drawing, "S1", EntityType::Polyline).size(), 2u);
    EXPECT_EQ(of(drawing, "S1", EntityType::Point).size(), 4u);
    EXPECT_EQ(std::ranges::count(layerNames(drawing), std::string("utilities/sewer/QL-A")), 0);
    for (const Entity& entity : drawing.entities) {
        const auto valid = katana::entity::validate(entity.geometry);
        EXPECT_TRUE(valid.ok()) << text(entity, keys::kVertex) << ": "
                                << (valid.ok() ? std::string() : valid.error().describe());
    }
}
