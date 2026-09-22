#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <sstream>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/entity/layer_path.hpp"

namespace a12 = katana::archive12d;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

constexpr double kPi = std::numbers::pi;

// These fixtures are hand-written, and a hand-written 12da inherits the
// format's CURRENT BREAKLINE TYPE, whose default is `point` (commands,
// 1.4.4). A fixture that means a line has to say so, exactly as every string
// in a real archive does - 12d writes the flag on all 25,659 strings of a
// production file and leaves nothing to the default. Rather than repeat it in
// every fixture, the helper states it once at file level, which is the
// format's own way of saying it; a fixture that wants POINTS says
// `breakline point` inside the string and overrides this.
constexpr const char* kBreaklineLine = "breakline line\n";

a12::DomainImport import(const std::string& text, const a12::ImportOptions& options = {})
{
    auto archive = a12::readArchive(kBreaklineLine + text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    auto domain = a12::toDomain(archive.ok() ? *archive : a12::Archive{}, options);
    EXPECT_TRUE(domain.ok()) << (domain.ok() ? "" : domain.error().describe());
    return domain.ok() ? std::move(*domain) : a12::DomainImport{};
}

bool anyWarningContains(const a12::DomainImport& domain, const std::string& needle)
{
    return std::any_of(domain.warnings.begin(), domain.warnings.end(),
                       [&](const std::string& w) { return w.find(needle) != std::string::npos; });
}

std::string allWarnings(const a12::DomainImport& domain)
{
    std::ostringstream out;
    for (const std::string& warning : domain.warnings) {
        out << "\n  " << warning;
    }
    return out.str();
}

double degreesOf(const Point2& from, const Point2& to)
{
    const double value = std::atan2(to.y - from.y, to.x - from.x) * 180.0 / kPi;
    return value < 0.0 ? value + 360.0 : value;
}

} // namespace

// ---- layers ------------------------------------------------------------------

TEST(LayerPath, ATreeNameIsAlreadyALayerPath)
{
    // Manual 1.1: "Stage 1/Water/Drainage" is an object-tree model name.
    EXPECT_EQ(a12::layerPathForModel("Stage 1/Water/Drainage"), "Stage 1/Water/Drainage");
    EXPECT_EQ(a12::layerPathForModel("Kerb", "survey/2024"), "survey/2024/Kerb");
}

TEST(LayerPath, WhatALayerPathCannotHoldIsReplacedNotRefused)
{
    EXPECT_EQ(a12::layerPathForModel(""), "12d");
    EXPECT_EQ(a12::layerPathForModel("   "), "12d");
    EXPECT_EQ(a12::layerPathForModel("/a//b/"), "a/b");
    EXPECT_EQ(a12::layerPathForModel("  padded  / name "), "padded/name");
    EXPECT_EQ(a12::layerPathForModel("a\\b"), "a_b");
    EXPECT_EQ(a12::layerPathForModel("../etc"), "_/etc");
    EXPECT_EQ(a12::layerPathForModel(std::string("tab\there")), "tab_here");
}

TEST(LayerPath, EveryResultIsAValidLayerPathHoweverHostileTheName)
{
    std::string deep;
    for (int i = 0; i < 40; ++i) {
        deep += "level" + std::to_string(i) + "/";
    }
    const std::string longName(2000, 'x');
    // Two-byte characters, so that a cut at the length limit can land inside one.
    std::string accented;
    for (int i = 0; i < 400; ++i) {
        accented += "\xC3\xA9";
    }
    std::size_t deeperThanTheLimit = 0;
    for (const std::string& name : {deep, longName, accented, std::string("."), std::string("a/./b"),
                                    std::string("\x01\x02"), std::string("x/") + longName}) {
        const std::string path = a12::layerPathForModel(name, "imported");
        EXPECT_TRUE(katana::entity::validateLayerPath(path).ok()) << path;
        EXPECT_TRUE(katana::entity::isValidUtf8(path));
        deeperThanTheLimit += name == deep ? 1 : 0;
    }
    // The generator does reach the case it exists for: 40 levels against a
    // limit of kMaximumLayerDepth.
    EXPECT_EQ(deeperThanTheLimit, 1u);
    EXPECT_GT(std::size_t{40}, katana::entity::kMaximumLayerDepth);
    EXPECT_EQ(katana::entity::layerDepth(a12::layerPathForModel(deep)),
              katana::entity::kMaximumLayerDepth);
}

TEST(DomainImport, EachModelBecomesALayerInFirstUseOrderWithItsFirstColour)
{
    const auto domain = import("model \"Stage 1/Water\" string super { colour blue data_2d { 0 0 1 1 } }\n"
                               "string super { colour red data_2d { 0 0 1 1 } }\n"
                               "model \"Kerb\" string super { colour \"pen 025\" data_2d { 0 0 1 1 } }");
    ASSERT_EQ(domain.layersNeeded.size(), 2u);
    EXPECT_EQ(domain.layersNeeded[0].name, "Stage 1/Water");
    EXPECT_EQ(domain.layersNeeded[0].color, (katana::entity::Color{0, 0, 255, 255}));
    EXPECT_EQ(domain.layersNeeded[1].name, "Kerb");
    ASSERT_EQ(domain.entities.size(), 3u);
    EXPECT_EQ(domain.entities[1].layer, "Stage 1/Water");
    EXPECT_EQ(domain.entities[2].layer, "Kerb");
}

TEST(DomainImport, ALayerPrefixPutsEveryModelUnderOneParent)
{
    a12::ImportOptions options;
    options.layerPrefix = "survey";
    const auto domain = import("model \"Kerb\" string super { data_2d { 0 0 1 1 } }", options);
    EXPECT_EQ(domain.entities.at(0).layer, "survey/Kerb");
}

// ---- colours -------------------------------------------------------------------

TEST(StandardColour, NamesAreMatchedWithoutRegardToCaseOrSpelling)
{
    EXPECT_EQ(a12::standardColour("red"), (katana::entity::Color{255, 0, 0, 255}));
    EXPECT_EQ(a12::standardColour("RED"), a12::standardColour("red"));
    EXPECT_EQ(a12::standardColour("Dark Green"), a12::standardColour("dark_green"));
    EXPECT_EQ(a12::standardColour("gray"), a12::standardColour("grey"));
    EXPECT_EQ(a12::standardColour("light gray"), a12::standardColour("light grey"));
    EXPECT_FALSE(a12::standardColour("pen 025").has_value());
    EXPECT_FALSE(a12::standardColour("").has_value());
}

TEST(StandardColour, EveryNameIsItsOwnNearestColour)
{
    for (const char* name : {"red", "green", "blue", "yellow", "cyan", "magenta", "white", "black",
                             "grey", "orange", "brown", "purple", "dark grey", "light blue"}) {
        const auto colour = a12::standardColour(name);
        ASSERT_TRUE(colour.has_value()) << name;
        EXPECT_EQ(a12::nearestStandardColour(*colour), name);
    }
    EXPECT_EQ(a12::nearestStandardColour({250, 5, 5, 255}), "red");
}

TEST(DomainImport, AStandardColourColoursTheEntityAndAnyOtherIsLeftToTheLayerButKept)
{
    const auto domain = import("string super { colour yellow data_2d { 0 0 1 1 } }\n"
                               "string super { colour \"vis concrete\" data_2d { 0 0 1 1 } }");
    ASSERT_EQ(domain.entities.size(), 2u);
    EXPECT_EQ(domain.entities[0].color, (katana::entity::Color{255, 255, 0, 255}));
    EXPECT_FALSE(domain.entities[1].color.has_value());
    EXPECT_EQ(std::get<std::string>(domain.entities[1].metadata.at("12d.colour")), "vis concrete");
}

// ---- geometry and heights ---------------------------------------------------------

TEST(DomainImport, OneVertexIsAPointTwoALineMoreAPolyline)
{
    const auto domain = import("string super { data_3d { 1 2 3 } }\n"
                               "string super { data_3d { 1 2 3  4 6 3 } }\n"
                               "string super { closed true data_3d { 0 0 1  4 0 1  4 3 1 } }");
    ASSERT_EQ(domain.entities.size(), 3u);
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(domain.entities[0].geometry).position,
              Point2(1.0, 2.0));
    const auto& line = std::get<katana::geometry::Segment2>(domain.entities[1].geometry);
    EXPECT_EQ(line.end, Point2(4.0, 6.0));
    const auto& polyline = std::get<Polyline2>(domain.entities[2].geometry);
    EXPECT_TRUE(polyline.closed);
    EXPECT_EQ(polyline.vertices.size(), 3u);
    // A 3-4-5 triangle: the bounds of everything imported.
    EXPECT_EQ(domain.bounds.min, Point2(0.0, 0.0));
    EXPECT_EQ(domain.bounds.max, Point2(4.0, 6.0));
}

TEST(DomainImport, VerticesAllInOnePlaceAreAPointNotAFailedImport)
{
    // A zero-length polyline is invalid, and entities are created in ONE
    // atomic command: left alone, this string would cost the whole file.
    const auto domain = import("string super { name \"dup\" data_3d { 5 5 1  5 5 1  5 5 1 } }\n"
                               "string super { data_3d { 0 0 0  1 1 0 } }");
    ASSERT_EQ(domain.entities.size(), 2u);
    EXPECT_TRUE(std::holds_alternative<katana::entity::PointGeometry>(domain.entities[0].geometry));
}

TEST(DomainImport, AStringWithNoVerticesIsSkippedAndSaidToBe)
{
    const auto domain = import("string super { name \"empty\" }\nstring super { data_2d { 0 0 1 1 } }");
    EXPECT_EQ(domain.entities.size(), 1u);
    EXPECT_TRUE(anyWarningContains(domain, "\"empty\"")) << allWarnings(domain);
    ASSERT_EQ(domain.tally.size(), 1u);
    EXPECT_EQ(domain.tally[0].read, 2u);
    EXPECT_EQ(domain.tally[0].imported, 1u);
}

TEST(DomainImport, OneHeightIsElevationAndDifferingHeightsAreListedWithTheirNulls)
{
    const auto domain = import("string super { data_3d { 0 0 7.5  1 1 7.5 } }\n"
                               "string super { data_3d { 0 0 1.5  1 1 null  2 2 2.25 } }\n"
                               "string super { z 31.25 data_2d { 0 0  1 1 } }\n"
                               "string super { data_2d { 0 0  1 1 } }");
    ASSERT_EQ(domain.entities.size(), 4u);
    EXPECT_EQ(std::get<double>(domain.entities[0].properties.at("elevation")), 7.5);
    EXPECT_FALSE(domain.entities[0].properties.contains("elevations"));
    EXPECT_EQ(std::get<std::string>(domain.entities[1].properties.at("elevations")),
              "1.5 null 2.25");
    EXPECT_FALSE(domain.entities[1].properties.contains("elevation"));
    EXPECT_EQ(std::get<double>(domain.entities[2].properties.at("elevation")), 31.25);
    EXPECT_TRUE(domain.entities[3].properties.empty()) << "no height is not a height of zero";
}

TEST(DomainImport, AnOriginShiftMovesPlanCoordinatesAndLeavesHeightsAlone)
{
    a12::ImportOptions options;
    options.originShift = katana::geometry::Vec2(502000.0, 6960000.0);
    const auto domain = import("string super { data_3d { 502010 6960020 32.5 } }", options);
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(domain.entities.at(0).geometry).position,
              Point2(10.0, 20.0));
    EXPECT_EQ(std::get<double>(domain.entities[0].properties.at("elevation")), 32.5);
}

// ---- attributes and metadata ------------------------------------------------------------

TEST(DomainImport, AttributesBecomeTypedPropertiesAndGroupsFlattenIntoPaths)
{
    const auto domain = import(R"(string super { attributes {
  text "Owner" "Telstra"  integer "Ways" 4  real "Size" 0.125
  group { name "Asset" attributes { text "Type" "Conduit" group { name "Dim" attributes { real "Cover" 0.9 } } } }
  text "Owner" "second of the same name"
} data_2d { 0 0 1 1 } })");
    const auto& p = domain.entities.at(0).properties;
    EXPECT_EQ(std::get<std::string>(p.at("Owner")), "Telstra");
    EXPECT_EQ(std::get<std::int64_t>(p.at("Ways")), 4);
    EXPECT_EQ(std::get<double>(p.at("Size")), 0.125);
    EXPECT_EQ(std::get<std::string>(p.at("Asset/Type")), "Conduit");
    EXPECT_EQ(std::get<double>(p.at("Asset/Dim/Cover")), 0.9);
    EXPECT_EQ(std::get<std::string>(p.at("Owner (2)")), "second of the same name");
}

TEST(DomainImport, WhatOnlyTwelveDKnowsAboutAStringTravelsInItsMetadata)
{
    a12::ImportOptions options;
    options.sourceName = "job.12da";
    const auto domain = import("model \"M\" string super { name \"Kerb BL-047\" style \"Kerb\" colour cyan"
                               " chainage 12.5 breakline line point_data { 101 \"A 7\" } data_3d { 0 0 1  1 1 1 }"
                               " pipe_value { diameter 0.3 } justify invert }",
                               options);
    const auto& m = domain.entities.at(0).metadata;
    EXPECT_EQ(std::get<std::string>(m.at("12d.element")), "string super");
    EXPECT_EQ(std::get<std::string>(m.at("12d.name")), "Kerb BL-047");
    EXPECT_FALSE(m.contains("12d.style")) << "the style is the entity's, not metadata";
    EXPECT_EQ(domain.entities[0].style, "Kerb");
    ASSERT_EQ(domain.stylesNeeded.size(), 1u);
    EXPECT_EQ(domain.stylesNeeded[0].name, "Kerb");
    EXPECT_EQ(domain.stylesNeeded[0].description, "12d linestyle");
    // The 12d LINESTYLE NAME, which is what the line should be drawn with.
    // It used to be left as "continuous", so the name reached nowhere a
    // renderer would look and a loaded 12d linestyle library could never be
    // matched against it (PLAN.MD 20.3).
    EXPECT_EQ(domain.stylesNeeded[0].linetype, "Kerb");
    EXPECT_EQ(std::get<std::string>(m.at("12d.colour")), "cyan");
    EXPECT_EQ(std::get<double>(m.at("12d.chainage")), 12.5);
    EXPECT_EQ(std::get<std::string>(m.at("12d.breakline")), "line");
    EXPECT_EQ(std::get<std::string>(m.at("12d.point_ids")), "101,A 7");
    EXPECT_EQ(std::get<double>(m.at("12d.diameter")), 0.3);
    EXPECT_EQ(std::get<std::string>(m.at("12d.justify")), "invert");
    EXPECT_EQ(std::get<std::string>(m.at("source")), "job.12da");
}

// ---- arcs: which way a radius turns ------------------------------------------------------------

namespace {

Polyline2 arcFromOriginToTwoZero(const std::string& radius, const std::string& major)
{
    const auto domain = import("string super { data_2d { 0 0  2 0 } radius_data { " + radius +
                               " } major_data { " + major + " } }");
    EXPECT_EQ(domain.entities.size(), 1u);
    const auto* polyline = std::get_if<Polyline2>(&domain.entities.at(0).geometry);
    EXPECT_NE(polyline, nullptr) << "an arc chords into more than two vertices";
    return polyline != nullptr ? *polyline : Polyline2{};
}

} // namespace

TEST(DomainImportArcs, APositiveRadiusBulgesAboveTheLineAndANegativeOneBelow)
{
    // Manual 1.5.8.2.2.2: "+ve radius puts the arc's bulge above the line
    // connecting the vertices". From (0,0) to (2,0) with radius 1 the arc is
    // the unit semicircle about (1,0): above for +1, below for -1.
    for (const double sign : {1.0, -1.0}) {
        const Polyline2 arc = arcFromOriginToTwoZero(sign > 0 ? "1" : "-1", "0");
        ASSERT_GT(arc.vertices.size(), 4u);
        EXPECT_EQ(arc.vertices.front(), Point2(0.0, 0.0));
        EXPECT_EQ(arc.vertices.back(), Point2(2.0, 0.0));
        for (std::size_t i = 1; i + 1 < arc.vertices.size(); ++i) {
            EXPECT_NEAR(arc.vertices[i].distanceTo(Point2(1.0, 0.0)), 1.0, 1e-12);
            EXPECT_GT(arc.vertices[i].y * sign, 0.0);
            // and it runs from the first vertex to the second, not back again
            EXPECT_GT(arc.vertices[i].x, arc.vertices[i - 1].x);
        }
    }
}

TEST(DomainImportArcs, MajorTakesTheLargerOfTheTwoArcsOnTheSameSide)
{
    // Radius sqrt 2 over a chord of 2: the centre is 1 from the chord, so the
    // minor arc rises to sqrt 2 - 1 above it and the major one to sqrt 2 + 1.
    const double root2 = std::sqrt(2.0);
    const auto height = [](const Polyline2& arc) {
        double top = 0.0;
        for (const Point2& vertex : arc.vertices) {
            top = std::max(top, vertex.y);
        }
        return top;
    };
    // The default chord tolerance is 1 mm, so the highest VERTEX is within
    // that of the top of the arc.
    EXPECT_NEAR(height(arcFromOriginToTwoZero("1.4142135623730951", "0")), root2 - 1.0, 1e-3);
    EXPECT_NEAR(height(arcFromOriginToTwoZero("1.4142135623730951", "1")), root2 + 1.0, 1e-3);
}

TEST(DomainImportArcs, ChordsStayWithinTheStatedToleranceOfTheArc)
{
    a12::ImportOptions options;
    options.curveTolerance = 0.01;
    const auto domain = import("string super { data_2d { 0 0  200 0 } radius_data { 100 } major_data { 0 } }",
                               options);
    const auto& arc = std::get<Polyline2>(domain.entities.at(0).geometry);
    for (std::size_t i = 0; i + 1 < arc.vertices.size(); ++i) {
        // The middle of a chord is where it is furthest from the arc.
        const Point2 mid = arc.vertices[i] + (arc.vertices[i + 1] - arc.vertices[i]) * 0.5;
        const double sagitta = 100.0 - mid.distanceTo(Point2(100.0, 0.0));
        EXPECT_GE(sagitta, 0.0);
        EXPECT_LE(sagitta, 0.01 + 1e-12);
    }
    EXPECT_TRUE(anyWarningContains(domain, "1 arcs and 0 transitions were chorded"))
        << allWarnings(domain);
}

TEST(DomainImportArcs, HeightsAreInterpolatedForTheVerticesChordingAdds)
{
    const auto domain = import("string super { data_3d { 0 0 10  2 0 20 } radius_data { 1 } major_data { 0 } }");
    const auto& arc = std::get<Polyline2>(domain.entities.at(0).geometry);
    std::istringstream list(std::get<std::string>(domain.entities[0].properties.at("elevations")));
    std::vector<double> heights;
    for (double z = 0.0; list >> z;) {
        heights.push_back(z);
    }
    ASSERT_EQ(heights.size(), arc.vertices.size());
    EXPECT_EQ(heights.front(), 10.0);
    EXPECT_EQ(heights.back(), 20.0);
    for (std::size_t i = 1; i < heights.size(); ++i) {
        EXPECT_GT(heights[i], heights[i - 1]);
    }
    // Equal chords of a semicircle: height rises linearly with vertex number.
    const double step = 10.0 / static_cast<double>(heights.size() - 1);
    EXPECT_NEAR(heights[1], 10.0 + step, 1e-9);
}

TEST(DomainImportArcs, ARadiusThatCannotSpanItsChordIsDrawnStraightAndCounted)
{
    const auto domain = import("string super { data_2d { 0 0  10 0 } radius_data { 2 } major_data { 0 } }");
    EXPECT_TRUE(std::holds_alternative<katana::geometry::Segment2>(domain.entities.at(0).geometry));
    EXPECT_TRUE(anyWarningContains(domain, "1 curved segments could not be reproduced"))
        << allWarnings(domain);
}

// ---- transitions: three conventions settled against 12d Model's own output --------------------
//
// The numbers below are copied from `test Super Alignment.12da`, written by 12d
// Model 15.0C1t: two consecutive vertices of a horizontal_data block and the
// `spiral` block between them. They are the external authority these tests
// rest on - 12d computed both ends, so a reading of the parameters is right
// exactly when the curve it produces runs from one to the other.

TEST(DomainImportTransitions, ALeadingCubicParabolaLandsOnTheVertexTwelveDRecorded)
{
    a12::ImportOptions options;
    options.curveTolerance = 1e-6;
    const auto domain = import(R"(string super { data_2d { 1342.85432099 1789.30813759  1419.06779208 1813.40212949 }
 geometry_data { spiral { type "cubic parabola" leading 1 l1 0 r1 0 a1 20.20620846 l2 80 r2 294.56299992 a2 12.2642972 } } })",
                               options);
    const auto& curve = std::get<Polyline2>(domain.entities.at(0).geometry);
    ASSERT_GT(curve.vertices.size(), 10u);
    // It leaves along a1 and arrives along a2 - which a clothoid of the same
    // length and radius does not: that ends at 12.4258 degrees, 0.16 out.
    // A chord differs from the tangent by half the angle it subtends: at a
    // sagitta of 1e-6 on R = 294 a chord is 0.1 m long and that is 0.01 degrees.
    EXPECT_NEAR(degreesOf(curve.vertices[0], curve.vertices[1]), 20.20620846, 0.02);
    const std::size_t n = curve.vertices.size();
    EXPECT_NEAR(degreesOf(curve.vertices[n - 2], curve.vertices[n - 1]), 12.2642972, 0.02);
    // Positive radius: it turns to the right, so the heading falls.
    EXPECT_FALSE(anyWarningContains(domain, "moved by")) << allWarnings(domain);
    EXPECT_FALSE(anyWarningContains(domain, "could not be reproduced")) << allWarnings(domain);
    EXPECT_NEAR(curve.length(), 80.0, 1e-3) << "l2 - l1 is the length ALONG the curve";
}

TEST(DomainImportTransitions, ATrailingTransitionIsDescribedBackwardsFromItsStraightEnd)
{
    // leading 0, yet l1 = 0 and r1 = 0: the manual says a full trailing
    // transition has l2 = 0 and r2 = 0. a1 = 200.2 is the tangent at the
    // SECOND vertex pointing back; the string itself runs at 20.2 there.
    a12::ImportOptions options;
    options.curveTolerance = 1e-6;
    const auto domain = import(R"(string super { data_2d { 1202.63472783 1741.58956859  1278.83135325 1765.74439519 }
 geometry_data { spiral { type "cubic parabola" leading 0 l1 0 r1 0 a1 200.20620846 l2 80 r2 299.38949171 a2 192.39768555 } } })",
                               options);
    const auto* curve = std::get_if<Polyline2>(&domain.entities.at(0).geometry);
    ASSERT_NE(curve, nullptr) << "read the manual's way it misses by 160 m and is drawn straight"
                              << allWarnings(domain);
    const std::size_t n = curve->vertices.size();
    EXPECT_NEAR(degreesOf(curve->vertices[0], curve->vertices[1]), 192.39768555 - 180.0, 0.02);
    EXPECT_NEAR(degreesOf(curve->vertices[n - 2], curve->vertices[n - 1]), 200.20620846 - 180.0,
                0.02);
    EXPECT_FALSE(anyWarningContains(domain, "moved by")) << allWarnings(domain);
    EXPECT_FALSE(anyWarningContains(domain, "could not be reproduced")) << allWarnings(domain);
}

TEST(DomainImportTransitions, ATransitionTypeWithNoDefinitionIsApproximatedAndTheErrorStated)
{
    // The same leading transition, relabelled as a type the manual names and
    // does not define. Drawn as a clothoid it ends 0.097 m from the recorded
    // vertex (integrated independently, in Python, while establishing the
    // conventions above); it is moved to meet it and the movement reported.
    const auto domain = import(R"(string super { data_2d { 1342.85432099 1789.30813759  1419.06779208 1813.40212949 }
 geometry_data { spiral { type "bloss" leading 1 l1 0 r1 0 a1 20.20620846 l2 80 r2 294.56299992 a2 12.2642972 } } })");
    const auto& curve = std::get<Polyline2>(domain.entities.at(0).geometry);
    EXPECT_EQ(curve.vertices.back(), Point2(1419.06779208, 1813.40212949));
    EXPECT_TRUE(anyWarningContains(domain, "\"bloss\" are drawn as clothoids and moved by up to 0.097"))
        << allWarnings(domain);
}

TEST(DomainImportTransitions, ATransitionThatMissesItsEndByMetresIsNotDrawnAsIfItFitted)
{
    // a1 points the wrong way entirely.
    const auto domain = import(R"(string super { data_2d { 0 0  80 5 }
 geometry_data { spiral { type clothoid leading 1 l1 0 r1 0 a1 180 l2 80 r2 300 a2 170 } } })");
    EXPECT_TRUE(std::holds_alternative<katana::geometry::Segment2>(domain.entities.at(0).geometry));
    EXPECT_TRUE(anyWarningContains(domain, "1 curved segments could not be reproduced"))
        << allWarnings(domain);

    // A length that cannot be a transition between these two vertices: shorter
    // than the chord, or ten times longer than it (a spiral turning less than
    // a half turn is under pi/2 chords long). Found by fuzzing: a length of
    // 1e300 passed the closure check because the allowed miss grew with it.
    for (const char* length : {"5", "1e300", "200"}) {
        const auto absurd = import("string super { data_2d { 0 0  10 10 } geometry_data { spiral {"
                                   " type clothoid leading 1 l1 0 r1 0 a1 45 l2 " + std::string(length) +
                                   " r2 1e-300 a2 45 } } }");
        EXPECT_TRUE(std::holds_alternative<katana::geometry::Segment2>(absurd.entities.at(0).geometry))
            << length;
        EXPECT_TRUE(anyWarningContains(absurd, "could not be reproduced")) << length << allWarnings(absurd);
    }
}

// ---- the other strings ------------------------------------------------------------------------------

TEST(DomainImport, AnArcStringsRadiusSaysWhichWayRoundItGoes)
{
    // Centre (0,0), from (10,0) to (0,10). Counter-clockwise that is a quarter
    // turn; clockwise it is the other three quarters.
    const auto domain = import("string arc { radius -10 xcentre 0 ycentre 0 zcentre 1 xstart 10 ystart 0 zstart 1"
                               " xend 0 yend 10 zend 1 }\n"
                               "string arc { radius 10 xcentre 0 ycentre 0 zcentre 1 xstart 10 ystart 0 zstart 1"
                               " xend 0 yend 10 zend 3 }");
    ASSERT_EQ(domain.entities.size(), 2u);
    const auto& quarter = std::get<katana::geometry::Arc2>(domain.entities[0].geometry);
    EXPECT_NEAR(quarter.sweep, kPi / 2.0, 1e-12);
    EXPECT_EQ(quarter.radius, 10.0);
    EXPECT_EQ(std::get<double>(domain.entities[0].properties.at("elevation")), 1.0);
    const auto& rest = std::get<katana::geometry::Arc2>(domain.entities[1].geometry);
    EXPECT_NEAR(rest.sweep, -1.5 * kPi, 1e-12);
    EXPECT_EQ(std::get<std::string>(domain.entities[1].properties.at("elevations")), "1 3");
}

TEST(DomainImport, CirclesFeaturesAndTextArriveAsThemselves)
{
    const auto domain = import("string circle { radius 8 xcentre 650 ycentre 200 zcentre 32 }\n"
                               "string feature { radius 3 xcentre 700 ycentre 250 zcentre 31 }\n"
                               "string text { worldsize 2.7 angle 90 x 5 y 6 z null text \"SS112061\" }\n"
                               "string text { papersize 3 x 1 y 1 text \"paper\" }\n"
                               "string text { x 1 y 1 text \"\" }");
    ASSERT_EQ(domain.entities.size(), 4u) << "empty text is not an entity";
    EXPECT_EQ(std::get<katana::geometry::Circle2>(domain.entities[0].geometry).radius, 8.0);
    EXPECT_EQ(std::get<double>(domain.entities[1].properties.at("elevation")), 31.0);
    const auto& text = std::get<katana::entity::TextGeometry>(domain.entities[2].geometry);
    EXPECT_EQ(text.text, "SS112061");
    EXPECT_EQ(text.height, 2.7);
    EXPECT_NEAR(text.rotation, kPi / 2.0, 1e-15);
    // Millimetres on paper are not model units; the model's default stands in.
    EXPECT_EQ(std::get<katana::entity::TextGeometry>(domain.entities[3].geometry).height,
              katana::entity::TextGeometry{}.height);
    EXPECT_TRUE(anyWarningContains(domain, "text is empty")) << allWarnings(domain);
}

TEST(DomainImport, TheTextOnTheVerticesOfAStringBecomesTextEntities)
{
    const auto domain = import("string 4d { worldsize 1.5 data { 0 0 1 \"GUM-01\"  5 5 2 \"\"  9 9 3 \"GUM-03\" } }");
    ASSERT_EQ(domain.entities.size(), 3u);
    EXPECT_TRUE(std::holds_alternative<Polyline2>(domain.entities[0].geometry));
    const auto& last = std::get<katana::entity::TextGeometry>(domain.entities[2].geometry);
    EXPECT_EQ(last.text, "GUM-03");
    EXPECT_EQ(last.position, Point2(9.0, 9.0));
    EXPECT_EQ(last.height, 1.5);
    EXPECT_EQ(std::get<double>(domain.entities[2].properties.at("elevation")), 3.0);
}

TEST(DomainImport, APlotFrameIsItsSheetOnTheGround)
{
    // A3, 420 x 297 mm, at 1:500: 0.420 * 500 = 210 m by 0.297 * 500 = 148.5 m.
    const auto domain = import("string plot_frame { name \"Sheet 01\" width 420 height 297 scale 500"
                               " rotation 0 xorigin 1000 yorigin 2000 }\n"
                               "string plot_frame { width 100 height 50 scale 1000 rotation 90 xorigin 0 yorigin 0 }");
    ASSERT_EQ(domain.entities.size(), 2u);
    const auto& sheet = std::get<Polyline2>(domain.entities[0].geometry);
    EXPECT_TRUE(sheet.closed);
    ASSERT_EQ(sheet.vertices.size(), 4u);
    EXPECT_EQ(sheet.vertices[0], Point2(1000.0, 2000.0));
    EXPECT_EQ(sheet.vertices[2], Point2(1210.0, 2148.5));
    EXPECT_EQ(std::get<std::int64_t>(domain.entities[0].properties.at("plot_frame.scale")), 500);
    // Turned a quarter counter-clockwise, its long side runs up the page.
    const auto& turned = std::get<Polyline2>(domain.entities[1].geometry);
    EXPECT_NEAR(turned.vertices[1].x, 0.0, 1e-9);
    EXPECT_NEAR(turned.vertices[1].y, 100.0, 1e-9);
    EXPECT_NEAR(turned.vertices[2].x, -50.0, 1e-9);
}

TEST(DomainImport, ADrainageStringIsALineCarryingItsPipesAndAPointForEachPit)
{
    const auto domain = import(R"(model "Drainage" string drainage { name "SW Line A" outfall 28.5 flow_direction 1
  data { 0 0 29 0 0   50 20 29.5 0 0 }
  pit { name "A1" type "Grated" diameter 1.05 x 0 y 0 z 31 }
  pit { name "A2" x 50 y 20 z 31.5 }
  pipe { name "A1-A2" diameter 0.375 us_level 29.5 }
  house_connection { name "HC1" x 25 y 10 z 29.2 }
})");
    ASSERT_EQ(domain.entities.size(), 4u);
    const Entity& line = domain.entities[0];
    EXPECT_TRUE(std::holds_alternative<katana::geometry::Segment2>(line.geometry));
    EXPECT_EQ(std::get<double>(line.properties.at("outfall")), 28.5);
    EXPECT_EQ(std::get<std::string>(line.properties.at("pipe.1.name")), "A1-A2");
    EXPECT_EQ(std::get<double>(line.properties.at("pipe.1.diameter")), 0.375);
    const Entity& pit = domain.entities[1];
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(pit.geometry).position, Point2(0.0, 0.0));
    EXPECT_EQ(std::get<std::string>(pit.properties.at("pit.name")), "A1");
    EXPECT_EQ(std::get<double>(pit.properties.at("pit.diameter")), 1.05);
    EXPECT_EQ(std::get<double>(pit.properties.at("elevation")), 31.0);
    EXPECT_FALSE(pit.properties.contains("pit.x")) << "where it is, is its geometry";
    EXPECT_EQ(std::get<std::string>(domain.entities[3].properties.at("house_connection.name")), "HC1");
    EXPECT_EQ(std::get<std::string>(domain.entities[3].metadata.at("12d.element")),
              "drainage house_connection");
}

// ---- surfaces --------------------------------------------------------------------------------------

TEST(DomainImportTin, OnlyVisibleNonConstructionTrianglesMakeTheSurface)
{
    const auto domain = import(R"(full_tin { name "GROUND" colour green
  points { -100 -100 0  -100 100 0  100 100 0  100 -100 0   0 0 5  10 0 6  0 10 7  10 10 8 }
  triangles { 5 7 6   6 7 8   1 5 6 }
  neighbours { 0 2 3   1 0 0   0 1 0 }
  nulling { 2 1 2 }
})");
    ASSERT_EQ(domain.surfaces.size(), 1u);
    const a12::ImportedSurface& ground = domain.surfaces[0];
    EXPECT_EQ(ground.name, "GROUND");
    EXPECT_EQ(ground.trianglesInFile, 3u);
    EXPECT_EQ(ground.trianglesNulled, 2u) << "one nulled, one touching a construction point";
    EXPECT_EQ(ground.surface.triangleCount(), 1u);
    EXPECT_EQ(ground.surface.vertexCount(), 3u) << "the construction points are not carried along";
    // The plane through (0,0,5), (10,0,6), (0,10,7) is z = 5 + 0.1x + 0.2y.
    const auto z = ground.surface.elevationAt(2.0, 2.0);
    ASSERT_TRUE(z.has_value());
    EXPECT_NEAR(*z, 5.6, 1e-12);
    EXPECT_FALSE(ground.surface.elevationAt(8.0, 8.0).has_value()) << "inside the nulled triangle";
    EXPECT_TRUE(domain.entities.empty());
    EXPECT_EQ(domain.bounds.max, Point2(10.0, 10.0));
}

TEST(DomainImportTin, TrianglesAreAcceptedWhicheverWayRoundTheWriterListedThem)
{
    // The manual wants clockwise. (0,0) (0,10) (10,0) is; (10,0) (10,10)
    // (0,10) is not - its doubled signed area is 0*10 - 10*(-10) = +100.
    const auto domain = import("tin { name \"T\" points { 0 0 1  10 0 1  0 10 1  10 10 1 }"
                               " triangles { 1 3 2   2 4 3 } }");
    ASSERT_EQ(domain.surfaces.size(), 1u) << allWarnings(domain);
    EXPECT_EQ(domain.surfaces[0].surface.triangleCount(), 2u);
    EXPECT_NEAR(domain.surfaces[0].surface.planArea(), 100.0, 1e-9);
}

TEST(DomainImportTin, ATriangleWithNoAreaInPlanHasNoSurfaceToGiveEither)
{
    // (0,0) (10,0) (20,0) are collinear: no ground under the triangle, and no
    // side for it to have been listed from. Left in, its edge 2-3 runs the
    // same way round as the neighbour's below the line, and one such pair
    // refuses the whole surface - which is how four surfaces of
    // plot_PW_example_data.12da were lost.
    const auto domain = import("tin { name \"T\" points { 0 0 1  10 0 1  20 0 1  15 -5 2 }"
                               " triangles { 1 2 3   3 2 4 } }");
    ASSERT_EQ(domain.surfaces.size(), 1u) << allWarnings(domain);
    EXPECT_EQ(domain.surfaces[0].surface.triangleCount(), 1u);
    EXPECT_EQ(domain.surfaces[0].trianglesNulled, 1u);
}

TEST(DomainImportTin, ATriangleWithANullHeightHasNoSurfaceToGive)
{
    const auto domain = import("tin { name \"T\" points { 0 0 1  10 0 1  0 10 1  10 10 null }"
                               " triangles { 1 3 2   2 3 4 } }");
    ASSERT_EQ(domain.surfaces.size(), 1u);
    EXPECT_EQ(domain.surfaces[0].surface.triangleCount(), 1u);
    EXPECT_EQ(domain.surfaces[0].trianglesNulled, 1u);
}

TEST(DomainImportTin, ATinWithNothingVisibleIsSkippedNotImportedEmpty)
{
    const auto domain = import("full_tin { name \"T\" points { 0 0 0 0 9 0 9 9 0 9 0 0  1 1 1 2 1 1 1 2 1 }"
                               " triangles { 5 7 6 } neighbours { 0 0 0 } nulling { 1 } }");
    EXPECT_TRUE(domain.surfaces.empty());
    EXPECT_TRUE(anyWarningContains(domain, "no visible triangles")) << allWarnings(domain);
    EXPECT_EQ(domain.tally.at(0).imported, 0u);
}

TEST(DomainImport, SuperTinsCloudsAndTrimeshesAreEachAccountedFor)
{
    const auto domain = import(R"(super_tin { name "COMBINED" tins { "A" "B" } }
string las_cloud_data { name "Scan" data { format v10_p0 points_v10_p0 { p { x 1 y 2 z 3 i 900 cl 2 } } } }
string las_cloud_data { name "Ref" ref_data { file_name "site.las" } }
primitive_3d { name M trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 } faces { 1 2 3 } } })");
    ASSERT_EQ(domain.superTins.size(), 1u);
    EXPECT_EQ(domain.superTins[0].tins.size(), 2u);
    ASSERT_EQ(domain.clouds.size(), 2u);
    ASSERT_EQ(domain.clouds[0].points.size(), 1u);
    EXPECT_EQ(domain.clouds[0].points[0].z, 3.0);
    EXPECT_EQ(domain.clouds[0].points[0].intensity, 900);
    EXPECT_EQ(domain.clouds[0].points[0].classification, 2);
    EXPECT_EQ(domain.clouds[1].referenceFile, "site.las");
    // The trimesh becomes a mesh in the session (PLAN.MD 20.2 slice 4).
    ASSERT_EQ(domain.meshes.size(), 1u);
    EXPECT_EQ(domain.meshes[0].name, "M");
    EXPECT_EQ(domain.meshes[0].mesh.triangleCount(), 1u);
    // The tally says the same thing in numbers.
    const auto mesh = std::find_if(domain.tally.begin(), domain.tally.end(),
                                   [](const auto& t) { return t.keyword == "primitive_3d"; });
    ASSERT_NE(mesh, domain.tally.end());
    EXPECT_EQ(mesh->read, 1u);
    EXPECT_EQ(mesh->imported, 1u);
}

TEST(DomainImport, WhatTheReaderDidNotReadIsSaidInTheImportsWarnings)
{
    const auto domain = import("string super { data_2d { 0 0 1 1 } drawables { } }");
    EXPECT_TRUE(anyWarningContains(domain, "not read: string super/drawables (1)"))
        << allWarnings(domain);
}

TEST(DomainImport, BadOptionsAreRefused)
{
    a12::ImportOptions options;
    options.curveTolerance = 0.0;
    EXPECT_FALSE(a12::toDomain(a12::Archive{}, options).ok());
    options = {};
    options.alignmentTolerance = -1.0;
    EXPECT_FALSE(a12::toDomain(a12::Archive{}, options).ok());
    options = {};
    options.layerPrefix = "bad//prefix";
    EXPECT_FALSE(a12::toDomain(a12::Archive{}, options).ok());
    options = {};
    options.originShift = katana::geometry::Vec2(std::nan(""), 0.0);
    EXPECT_FALSE(a12::toDomain(a12::Archive{}, options).ok());
    EXPECT_TRUE(a12::toDomain(a12::Archive{}, {}).ok()) << "an empty archive is an empty import";
}
