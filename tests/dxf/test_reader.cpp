#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <optional>
#include <string>
#include <vector>

#include "katana/dxf/reader.hpp"
#include "katana/entity/entity_geometry.hpp"

namespace dxf = katana::dxf;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::PointGeometry;
using katana::entity::TextGeometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

constexpr double kPi = std::numbers::pi;

dxf::DxfImport load(const char* name, const dxf::ImportOptions& options = {})
{
    auto result = dxf::readDxfFile(std::string(KATANA_DXF_TEST_DATA) + "/" + name, options);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : dxf::DxfImport{};
}

template <typename T> std::vector<const Entity*> ofKind(const dxf::DxfImport& imported)
{
    std::vector<const Entity*> out;
    for (const Entity& entity : imported.entities) {
        if (std::holds_alternative<T>(entity.geometry)) {
            out.push_back(&entity);
        }
    }
    return out;
}

const Entity* textSaying(const dxf::DxfImport& imported, const std::string& text)
{
    for (const Entity& entity : imported.entities) {
        if (const auto* shape = std::get_if<TextGeometry>(&entity.geometry);
            shape != nullptr && shape->text == text) {
            return &entity;
        }
    }
    return nullptr;
}

const katana::entity::Layer* layerNamed(const dxf::DxfImport& imported, const std::string& name)
{
    for (const auto& layer : imported.layers) {
        if (layer.name == name) {
            return &layer;
        }
    }
    return nullptr;
}

const dxf::EntityTally* tallyOf(const dxf::DxfImport& imported, const std::string& kind)
{
    for (const auto& tally : imported.tally) {
        if (tally.kind == kind) {
            return &tally;
        }
    }
    return nullptr;
}

bool anyWarningContains(const dxf::DxfImport& imported, const std::string& text)
{
    return std::any_of(imported.warnings.begin(), imported.warnings.end(),
                       [&](const std::string& warning) {
                           return warning.find(text) != std::string::npos;
                       });
}

Color rgb(int r, int g, int b)
{
    return Color{static_cast<std::uint8_t>(r), static_cast<std::uint8_t>(g),
                 static_cast<std::uint8_t>(b), 255};
}

} // namespace

// ---- R12 ---------------------------------------------------------------------------

TEST(DxfReaderR12, TheFileIsReadWithEveryEntityItDraws)
{
    const auto imported = load("r12_survey.dxf");
    EXPECT_EQ(imported.version, "AC1009");
    EXPECT_EQ(imported.release, "R12");
    // LINE 1, ARC 1, CIRCLE 1, POLYLINE 2, POINT 1, TEXT 2, and the INSERT's
    // circle, line and visible attribute: 11. The SOLID is not imported.
    EXPECT_EQ(imported.entities.size(), 11u);
}

TEST(DxfReaderR12, AnArcStaysAnArcWithItsCentreRadiusAndSweep)
{
    const auto imported = load("r12_survey.dxf");
    const auto arcs = ofKind<Arc2>(imported);
    ASSERT_EQ(arcs.size(), 1u);
    const Arc2& arc = std::get<Arc2>(arcs[0]->geometry);
    EXPECT_EQ(arc.center, Point2(100.0, 200.0));
    EXPECT_EQ(arc.radius, 1000.0);
    EXPECT_EQ(arc.startAngle, 0.0);
    // 30 degrees; its length is 1000 * pi / 6 = 523.599, which GDAL's
    // nine chords made 523.505.
    EXPECT_NEAR(arc.sweep, kPi / 6.0, 1e-15);
    EXPECT_NEAR(arc.length(), 523.5987755982988, 1e-9);
}

TEST(DxfReaderR12, ACircleIsACircleNotAnOpenPolyline)
{
    const auto imported = load("r12_survey.dxf");
    const auto circles = ofKind<Circle2>(imported);
    // The drawing's own circle, and the tree block's circle.
    ASSERT_EQ(circles.size(), 2u);
    const auto own = std::find_if(circles.begin(), circles.end(), [](const Entity* entity) {
        return std::get<Circle2>(entity->geometry).center == Point2(5.0, 5.0);
    });
    ASSERT_NE(own, circles.end());
    EXPECT_EQ(std::get<Circle2>((*own)->geometry).radius, 0.6);
}

TEST(DxfReaderR12, TextKeepsItsStringHeightAndRotation)
{
    const auto imported = load("r12_survey.dxf");
    const Entity* lot = textSaying(imported, "LOT 42");
    ASSERT_NE(lot, nullptr);
    const auto& text = std::get<TextGeometry>(lot->geometry);
    EXPECT_EQ(text.position, Point2(20.0, 30.0));
    EXPECT_EQ(text.height, 2.5);
    EXPECT_NEAR(text.rotation, kPi / 2.0, 1e-15); // 90 degrees
}

TEST(DxfReaderR12, ThePercentCodesOfTextBecomeTheSignsTheyName)
{
    const auto imported = load("r12_survey.dxf");
    const Entity* bearing = textSaying(imported, "45\u00B0 BEARING");
    ASSERT_NE(bearing, nullptr);
    EXPECT_EQ(std::get<TextGeometry>(bearing->geometry).height, 1.8);
}

TEST(DxfReaderR12, ABulgedPolylineIsChordedWithinTheToleranceAndKeepsItsVertices)
{
    const auto imported = load("r12_survey.dxf");
    const Entity* found = nullptr;
    for (const Entity* entity : ofKind<Polyline2>(imported)) {
        if (std::get<Polyline2>(entity->geometry).closed) {
            found = entity;
        }
    }
    ASSERT_NE(found, nullptr);
    const Polyline2& polyline = std::get<Polyline2>(found->geometry);
    // Bulge 1 from (0,0) to (2,0) is a half turn counter-clockwise: centre
    // (1,0), radius 1, bulging below the chord. At 1 mm the step is
    // 2 acos(1 - 0.001) = 0.08945 rad, so pi needs ceil(35.12) = 36 chords,
    // 35 points between the two vertices: 4 + 35 = 39 in all.
    ASSERT_EQ(polyline.vertices.size(), 39u);
    EXPECT_TRUE(polyline.closed);
    EXPECT_EQ(polyline.vertices.front(), Point2(0.0, 0.0));
    EXPECT_EQ(polyline.vertices[36], Point2(2.0, 0.0));
    EXPECT_EQ(polyline.vertices[37], Point2(2.0, 2.0));
    EXPECT_EQ(polyline.vertices[38], Point2(0.0, 2.0));
    for (std::size_t i = 1; i < 36; ++i) {
        EXPECT_NEAR(polyline.vertices[i].distanceTo(Point2(1.0, 0.0)), 1.0, 1e-12);
        EXPECT_LT(polyline.vertices[i].y, 0.0);
    }
    // The square's 4 and the 36 triangles of the fan: 1/2 * 36 * sin(pi/36).
    EXPECT_NEAR(polyline.area(), 4.0 + 18.0 * std::sin(kPi / 36.0), 1e-12);
    EXPECT_GE(imported.arcsChorded, 1u);
    EXPECT_TRUE(anyWarningContains(imported, "chorded to within 0.001"));
}

TEST(DxfReaderR12, AThreeDimensionalPolylineKeepsEveryVertexHeight)
{
    const auto imported = load("r12_survey.dxf");
    const Entity* found = nullptr;
    for (const Entity* entity : ofKind<Polyline2>(imported)) {
        if (!std::get<Polyline2>(entity->geometry).closed) {
            found = entity;
        }
    }
    ASSERT_NE(found, nullptr);
    const Polyline2& polyline = std::get<Polyline2>(found->geometry);
    ASSERT_EQ(polyline.vertices.size(), 3u);
    EXPECT_EQ(polyline.vertices[2], Point2(10.0, 10.0));
    const auto heights = katana::entity::heightsOf(found->properties, 3);
    ASSERT_EQ(heights.size(), 3u);
    EXPECT_EQ(heights[0], 10.0);
    EXPECT_EQ(heights[1], 11.0);
    EXPECT_EQ(heights[2], 12.5);
    EXPECT_EQ(found->layer, "KERB");
}

TEST(DxfReaderR12, APointKeepsItsHeightAsItsElevation)
{
    const auto imported = load("r12_survey.dxf");
    const auto points = ofKind<PointGeometry>(imported);
    ASSERT_EQ(points.size(), 1u);
    EXPECT_EQ(std::get<PointGeometry>(points[0]->geometry).position, Point2(3.0, 4.0));
    EXPECT_EQ(katana::entity::heightsOf(points[0]->properties, 1).front(), 55.5);
}

TEST(DxfReaderR12, TheLayerTableGivesEachLayerItsColourLinetypeAndVisibility)
{
    const auto imported = load("r12_survey.dxf");
    ASSERT_EQ(imported.layers.size(), 3u);
    const auto* kerb = layerNamed(imported, "KERB");
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->color, rgb(255, 0, 0));
    EXPECT_EQ(kerb->linetype, "DASHED");
    EXPECT_TRUE(kerb->visible);
    // Colour -3: green, switched off.
    const auto* hidden = layerNamed(imported, "HIDDEN");
    ASSERT_NE(hidden, nullptr);
    EXPECT_EQ(hidden->color, rgb(0, 255, 0));
    EXPECT_FALSE(hidden->visible);
    ASSERT_EQ(imported.linetypes.size(), 1u);
    EXPECT_EQ(imported.linetypes[0].name, "DASHED");
    ASSERT_EQ(imported.linetypes[0].pattern.size(), 2u);
    EXPECT_EQ(imported.linetypes[0].pattern[0].length, 0.5);
    EXPECT_EQ(imported.linetypes[0].pattern[1].length, -0.25);
}

TEST(DxfReaderR12, ABlockInsertIsExpandedScaledRotatedAndMoved)
{
    const auto imported = load("r12_survey.dxf");
    // The tree block's base is (1,1); the insert puts it at (50,50), twice
    // the size, turned 90 degrees: p -> (50,50) + R90 (2 (p - (1,1))).
    // Its circle at (1,1) r 0.5 -> (50,50) r 1. Its line (0,1)-(2,1):
    // (-1,0) -> (-2,0) -> (0,-2) -> (50,48), and (1,0) -> (2,0) -> (0,2) -> (50,52).
    const Entity* circle = nullptr;
    for (const Entity* entity : ofKind<Circle2>(imported)) {
        if (std::get<Circle2>(entity->geometry).center == Point2(50.0, 50.0)) {
            circle = entity;
        }
    }
    ASSERT_NE(circle, nullptr);
    EXPECT_NEAR(std::get<Circle2>(circle->geometry).radius, 1.0, 1e-15);
    const Entity* line = nullptr;
    for (const Entity* entity : ofKind<Segment2>(imported)) {
        if (entity->metadata.contains(dxf::kMetaBlock)) {
            line = entity;
        }
    }
    ASSERT_NE(line, nullptr);
    const Segment2& segment = std::get<Segment2>(line->geometry);
    EXPECT_NEAR(segment.start.x, 50.0, 1e-12);
    EXPECT_NEAR(segment.start.y, 48.0, 1e-12);
    EXPECT_NEAR(segment.end.x, 50.0, 1e-12);
    EXPECT_NEAR(segment.end.y, 52.0, 1e-12);
    EXPECT_EQ(katana::entity::toString(line->metadata.at(std::string(dxf::kMetaBlock))), "TREE");
    const auto* tally = tallyOf(imported, "INSERT");
    ASSERT_NE(tally, nullptr);
    EXPECT_EQ(tally->read, 1u);
    EXPECT_EQ(tally->imported, 3u); // the circle, the line, the visible attribute
}

TEST(DxfReaderR12, EntitiesOnLayerZeroInABlockTakeTheInsertsLayerAndByBlockItsColour)
{
    const auto imported = load("r12_survey.dxf");
    for (const Entity* entity : ofKind<Circle2>(imported)) {
        if (std::get<Circle2>(entity->geometry).center == Point2(50.0, 50.0)) {
            EXPECT_EQ(entity->layer, "KERB"); // on 0 in the block
            EXPECT_EQ(entity->color, rgb(0, 255, 0)); // ByBlock; the insert is 3
        }
    }
    for (const Entity* entity : ofKind<Segment2>(imported)) {
        if (entity->metadata.contains(dxf::kMetaBlock)) {
            EXPECT_EQ(entity->layer, "KERB"); // its own layer
            EXPECT_FALSE(entity->color.has_value()); // ByLayer
        }
    }
}

TEST(DxfReaderR12, AnInsertsAttributesArePropertiesOfWhatItProduces)
{
    const auto imported = load("r12_survey.dxf");
    std::size_t carrying = 0;
    for (const Entity& entity : imported.entities) {
        if (!entity.properties.contains("SPECIES")) {
            continue;
        }
        ++carrying;
        EXPECT_EQ(katana::entity::toString(entity.properties.at("SPECIES")), "Gum");
        EXPECT_EQ(katana::entity::toString(entity.properties.at("HEIGHT")), "12");
    }
    EXPECT_EQ(carrying, 3u);
    // The visible attribute is on the drawing where the file put it; the
    // invisible one is not.
    const Entity* height = textSaying(imported, "12");
    ASSERT_NE(height, nullptr);
    EXPECT_EQ(std::get<TextGeometry>(height->geometry).position, Point2(48.0, 48.0));
    EXPECT_EQ(textSaying(imported, "Gum"), nullptr);
}

TEST(DxfReaderR12, UnsupportedEntitiesAreCountedNotSilentlyDropped)
{
    const auto imported = load("r12_survey.dxf");
    const auto* solid = tallyOf(imported, "SOLID");
    ASSERT_NE(solid, nullptr);
    EXPECT_EQ(solid->read, 1u);
    EXPECT_EQ(solid->imported, 0u);
    EXPECT_TRUE(anyWarningContains(imported, "1 x SOLID entities not imported"));
}

// ---- R2000 -------------------------------------------------------------------------

TEST(DxfReaderR2000, TheFileIsReadWithEveryEntityItDraws)
{
    const auto imported = load("r2000_site.dxf");
    EXPECT_EQ(imported.release, "R2000");
    // LWPOLYLINE 1, MTEXT 3 lines, LINE 1, CIRCLE 1, ARC 1, the dimension's
    // line and text 2: 9. The HATCH, the paper space line and the insert of
    // an undefined block make nothing.
    EXPECT_EQ(imported.entities.size(), 9u);
}

TEST(DxfReaderR2000, AnLwPolylineKeepsItsClosedFlagElevationAndArc)
{
    const auto imported = load("r2000_site.dxf");
    const auto polylines = ofKind<Polyline2>(imported);
    ASSERT_EQ(polylines.size(), 1u);
    const Polyline2& polyline = std::get<Polyline2>(polylines[0]->geometry);
    EXPECT_TRUE(polyline.closed);
    // Bulge tan(pi/8) from (10,0) to (20,10) is a quarter turn: chord
    // 10 sqrt 2, radius (10 sqrt 2 / 4)(1 + b^2)/b = 10, centre (10,10).
    // At 1 mm: step 2 acos(0.9999) = 0.028284, (pi/2)/0.028284 = 55.5 -> 56
    // chords, 55 points between: 4 + 55 = 59.
    ASSERT_EQ(polyline.vertices.size(), 59u);
    for (std::size_t i = 2; i < 57; ++i) {
        EXPECT_NEAR(polyline.vertices[i].distanceTo(Point2(10.0, 10.0)), 10.0, 1e-9);
    }
    // The 10 x 10 square and the fan of 56 triangles of the quarter circle:
    // 1/2 * 100 * 56 * sin(pi/112).
    EXPECT_NEAR(polyline.area(), 100.0 + 2800.0 * std::sin(kPi / 112.0), 1e-9);
    const auto heights = katana::entity::heightsOf(polylines[0]->properties, 59);
    EXPECT_EQ(heights.front(), 7.25);
    EXPECT_EQ(heights.back(), 7.25);
    EXPECT_EQ(polylines[0]->layer, "Boundary");
}

TEST(DxfReaderR2000, EachMTextParagraphBecomesATextWithItsCodesStripped)
{
    const auto imported = load("r2000_site.dxf");
    // Attached top left at (100,200), height 3: the first baseline is one
    // height below, and each next one five thirds of a height (5) lower.
    const Entity* site = textSaying(imported, "SITE");
    const Entity* plan = textSaying(imported, "PLAN No.\u00A03");
    const Entity* half = textSaying(imported, "1/2");
    ASSERT_NE(site, nullptr);
    ASSERT_NE(plan, nullptr);
    ASSERT_NE(half, nullptr);
    EXPECT_EQ(std::get<TextGeometry>(site->geometry).position, Point2(100.0, 197.0));
    EXPECT_EQ(std::get<TextGeometry>(plan->geometry).position, Point2(100.0, 192.0));
    EXPECT_EQ(std::get<TextGeometry>(half->geometry).position, Point2(100.0, 187.0));
    EXPECT_EQ(std::get<TextGeometry>(site->geometry).height, 3.0);
    EXPECT_EQ(std::get<TextGeometry>(site->geometry).rotation, 0.0);
    EXPECT_EQ(site->layer, "Text Layer");
}

TEST(DxfReaderR2000, TheTrueColourWinsOverTheIndexedColour)
{
    const auto imported = load("r2000_site.dxf");
    // Layer Boundary: index 1 (red) and true colour 0xFF8000.
    const auto* boundary = layerNamed(imported, "Boundary");
    ASSERT_NE(boundary, nullptr);
    EXPECT_EQ(boundary->color, rgb(255, 128, 0));
    EXPECT_TRUE(boundary->locked);
    EXPECT_EQ(boundary->lineWeight, 0.5); // 370 = 50 hundredths
    EXPECT_EQ(boundary->linetype, "CENTER2");
    // The line: index 1 and true colour 255 (blue), lineweight 35 kept.
    const auto lines = ofKind<Segment2>(imported);
    const auto own = std::find_if(lines.begin(), lines.end(), [](const Entity* entity) {
        return !entity->metadata.contains(dxf::kMetaBlock);
    });
    ASSERT_NE(own, lines.end());
    EXPECT_EQ((*own)->color, rgb(0, 0, 255));
    EXPECT_EQ(std::get<std::int64_t>((*own)->metadata.at(std::string(dxf::kMetaLineweight))), 35);
}

TEST(DxfReaderR2000, ALinetypePatternThatStartsWithAGapIsTurnedToStartWithADash)
{
    const auto imported = load("r2000_site.dxf");
    ASSERT_EQ(imported.linetypes.size(), 2u);
    // CENTER2 is gap 0.1, dash 0.5, gap 0.1, dash 0.2 in the file: the same
    // cycle from its first dash is dash 0.5, gap 0.1, dash 0.2, gap 0.1.
    const auto& centre = imported.linetypes[0];
    EXPECT_EQ(centre.name, "CENTER2");
    ASSERT_EQ(centre.pattern.size(), 4u);
    EXPECT_EQ(centre.pattern[0].length, 0.5);
    EXPECT_EQ(centre.pattern[1].length, -0.1);
    EXPECT_EQ(centre.pattern[2].length, 0.2);
    EXPECT_EQ(centre.pattern[3].length, -0.1);
    // DOTTY is a dot and a gap, which is already Katana's form.
    const auto& dotty = imported.linetypes[1];
    ASSERT_EQ(dotty.pattern.size(), 2u);
    EXPECT_TRUE(dotty.pattern[0].isDot());
}

TEST(DxfReaderR2000, AnObjectCoordinateSystemFacingDownMirrorsCirclesAndArcs)
{
    const auto imported = load("r2000_site.dxf");
    // Extrusion (0,0,-1): the object X axis is world -X, so (x, y) is (-x, y).
    const auto circles = ofKind<Circle2>(imported);
    ASSERT_EQ(circles.size(), 1u);
    EXPECT_EQ(std::get<Circle2>(circles[0]->geometry).center, Point2(-5.0, 3.0));
    // The arc: object centre (10,0), r 5, 0 to 90 degrees, i.e. from (15,0)
    // to (10,5). In the world, from (-15,0) to (-10,5) clockwise - which is
    // counter-clockwise from 90 degrees to 180 about (-10,0).
    const auto arcs = ofKind<Arc2>(imported);
    ASSERT_EQ(arcs.size(), 1u);
    const Arc2& arc = std::get<Arc2>(arcs[0]->geometry);
    EXPECT_EQ(arc.center, Point2(-10.0, 0.0));
    EXPECT_NEAR(arc.startAngle, kPi / 2.0, 1e-15);
    EXPECT_NEAR(arc.sweep, kPi / 2.0, 1e-15);
    EXPECT_NEAR(arc.endPoint().x, -15.0, 1e-12);
    EXPECT_NEAR(arc.endPoint().y, 0.0, 1e-12);
}

TEST(DxfReaderR2000, ADimensionIsItsPictureOnTheDimensionsLayer)
{
    const auto imported = load("r2000_site.dxf");
    const Entity* label = textSaying(imported, "10.00");
    ASSERT_NE(label, nullptr);
    EXPECT_EQ(label->layer, "Text Layer"); // on 0 in the picture block
    EXPECT_EQ(std::get<TextGeometry>(label->geometry).position, Point2(3.5, -4.5));
    const auto* tally = tallyOf(imported, "DIMENSION");
    ASSERT_NE(tally, nullptr);
    EXPECT_EQ(tally->imported, 2u);
}

TEST(DxfReaderR2000, PaperSpaceHatchesAndMissingBlocksAreReportedNotImported)
{
    const auto imported = load("r2000_site.dxf");
    EXPECT_TRUE(anyWarningContains(imported, "1 x paper space entities not imported"));
    EXPECT_TRUE(anyWarningContains(imported, "1 x HATCH entities not imported"));
    EXPECT_TRUE(anyWarningContains(imported, "1 x INSERT of a block the file does not define"));
    const auto* hatch = tallyOf(imported, "HATCH");
    ASSERT_NE(hatch, nullptr);
    EXPECT_EQ(hatch->imported, 0u);
}

TEST(DxfReaderR2000, TheOriginShiftIsSubtractedFromEveryCoordinate)
{
    dxf::ImportOptions options;
    options.originShift = katana::geometry::Vec2(100.0, 200.0);
    const auto imported = load("r2000_site.dxf", options);
    const Entity* site = textSaying(imported, "SITE");
    ASSERT_NE(site, nullptr);
    EXPECT_EQ(std::get<TextGeometry>(site->geometry).position, Point2(0.0, -3.0));
    EXPECT_EQ(std::get<Circle2>(ofKind<Circle2>(imported).front()->geometry).center,
              Point2(-105.0, -197.0));
}

TEST(DxfReaderR2000, EveryImportedEntityIsValidGeometry)
{
    for (const char* name : {"r12_survey.dxf", "r2000_site.dxf"}) {
        const auto imported = load(name);
        for (const Entity& entity : imported.entities) {
            EXPECT_TRUE(katana::entity::validate(entity.geometry).ok()) << name;
        }
        EXPECT_FALSE(imported.bounds.empty()) << name;
    }
}

TEST(DxfReaderPaths, TheDxfExtensionIsRecognisedInAnyCase)
{
    EXPECT_TRUE(dxf::isDxfPath("site.dxf"));
    EXPECT_TRUE(dxf::isDxfPath("C:/work/SITE.DXF"));
    EXPECT_TRUE(dxf::isDxfPath("plan.Dxf"));
    EXPECT_FALSE(dxf::isDxfPath("site.dwg"));
    EXPECT_FALSE(dxf::isDxfPath("site.dxfx"));
    EXPECT_FALSE(dxf::isDxfPath("dxf"));
}

TEST(DxfReaderLayers, TwoLayerNamesThatBecomeOnePathAreOneLayer)
{
    // "Kerb" with a control character in it cannot be a layer path, and
    // becomes "Kerb_" - which the file also names. One layer, two entities.
    const std::string text = "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n"
                             "  0\nLAYER\n  2\nKerb\x01\n 62\n1\n"
                             "  0\nLAYER\n  2\nKerb_\n 62\n5\n"
                             "  0\nENDTAB\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nPOINT\n  8\nKerb\x01\n 10\n1\n 20\n1\n"
                             "  0\nPOINT\n  8\nKerb_\n 10\n2\n 20\n2\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->layers.size(), 1u);
    EXPECT_EQ(imported->layers[0].name, "Kerb_");
    EXPECT_EQ(imported->layers[0].color, rgb(255, 0, 0)); // the first stands
    ASSERT_EQ(imported->entities.size(), 2u);
    EXPECT_EQ(imported->entities[0].layer, "Kerb_");
    EXPECT_EQ(imported->entities[1].layer, "Kerb_");
}

TEST(DxfReaderLayers, LayerNamesAreMatchedWithoutRegardToLetterCase)
{
    // The table says "Kerb", the entity "KERB": the same layer, as the format
    // compares names.
    const std::string text = "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n"
                             "  0\nLAYER\n  2\nKerb\n 62\n3\n"
                             "  0\nENDTAB\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nPOINT\n  8\nKERB\n 10\n1\n 20\n1\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->layers.size(), 1u);
    EXPECT_EQ(imported->entities.front().layer, "Kerb");
}

// ---- blocks ------------------------------------------------------------------------

TEST(DxfReaderBlocks, ANestedBlockIsPlacedThroughBothInsertsAndInheritsThroughBoth)
{
    // INNER: a line (0,0)-(1,0) on layer 0, colour ByBlock. OUTER: INNER
    // inserted at (10,0) turned 90 degrees, on layer 0, ByBlock. The drawing:
    // OUTER at (100,100), twice the size, on SITE in red.
    // INNER in OUTER: (0,0)-(0,1), moved to (10,0)-(10,1). OUTER placed:
    // (20,0)-(20,2), moved to (120,100)-(120,102). Layer 0 under layer 0
    // under SITE is SITE; ByBlock under ByBlock under red is red.
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nINNER\n 10\n0\n 20\n0\n"
                             "  0\nLINE\n  8\n0\n 62\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n0\n"
                             "  0\nENDBLK\n"
                             "  0\nBLOCK\n  2\nOUTER\n 10\n0\n 20\n0\n"
                             "  0\nINSERT\n  8\n0\n 62\n0\n  2\nINNER\n 10\n10\n 20\n0\n 50\n90\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  8\nSITE\n 62\n1\n  2\nOUTER\n 10\n100\n 20\n100\n"
                             " 41\n2\n 42\n2\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 1u);
    const Entity& line = imported->entities[0];
    const Segment2& segment = std::get<Segment2>(line.geometry);
    EXPECT_NEAR(segment.start.x, 120.0, 1e-12);
    EXPECT_NEAR(segment.start.y, 100.0, 1e-12);
    EXPECT_NEAR(segment.end.x, 120.0, 1e-12);
    EXPECT_NEAR(segment.end.y, 102.0, 1e-12);
    EXPECT_EQ(line.layer, "SITE");
    EXPECT_EQ(line.color, rgb(255, 0, 0));
    EXPECT_EQ(katana::entity::toString(line.metadata.at(std::string(dxf::kMetaBlock))),
              "OUTER/INNER");
}

TEST(DxfReaderBlocks, AnArrayInsertPlacesEveryCopy)
{
    // Two columns 10 apart, three rows 5 apart: points at x 0 and 10, y 0, 5, 10.
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nDOT\n 10\n0\n 20\n0\n"
                             "  0\nPOINT\n 10\n0\n 20\n0\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  2\nDOT\n 10\n0\n 20\n0\n 70\n2\n 71\n3\n 44\n10\n 45\n5\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 6u);
    std::vector<Point2> points;
    for (const Entity& entity : imported->entities) {
        points.push_back(std::get<PointGeometry>(entity.geometry).position);
    }
    for (const Point2& expected : {Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(0.0, 5.0),
                                   Point2(10.0, 5.0), Point2(0.0, 10.0), Point2(10.0, 10.0)}) {
        EXPECT_NE(std::find(points.begin(), points.end(), expected), points.end())
            << expected.x << "," << expected.y;
    }
}

TEST(DxfReaderBlocks, ACircleInABlockScaledUnevenlyIsTheEllipseItDraws)
{
    // A unit circle inserted twice as wide as it is high: the ellipse
    // (x/2)^2 + y^2 = 1, chorded. The chords are worked in the block's own
    // units at the tolerance over the largest scale, 0.0005 on radius 1: a
    // step of 2 acos(0.9995) = 0.06325, so 2 pi needs ceil(99.3) = 100.
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nRING\n 10\n0\n 20\n0\n"
                             "  0\nCIRCLE\n 10\n0\n 20\n0\n 40\n1\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  2\nRING\n 10\n0\n 20\n0\n 41\n2\n 42\n1\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 1u);
    const Polyline2& ellipse = std::get<Polyline2>(imported->entities[0].geometry);
    EXPECT_TRUE(ellipse.closed);
    ASSERT_EQ(ellipse.vertices.size(), 100u);
    for (const Point2& vertex : ellipse.vertices) {
        EXPECT_NEAR((vertex.x / 2.0) * (vertex.x / 2.0) + vertex.y * vertex.y, 1.0, 1e-12);
    }
}

// ---- heights --------------------------------------------------------------------

namespace {

// The heights of an imported entity, one a vertex (two for a line or an arc).
std::vector<std::optional<double>> heightsOf(const Entity& entity, std::size_t count)
{
    return katana::entity::heightsOf(entity.properties, count);
}

bool hasNoHeight(const Entity& entity)
{
    return !entity.properties.contains(std::string(katana::entity::kElevationProperty)) &&
           !entity.properties.contains(std::string(katana::entity::kElevationsProperty));
}

} // namespace

TEST(DxfReaderBlocks, AnEntityDrawnAtTheBlocksZeroIsAtTheLevelOfTheInsert)
{
    // The survey symbol: drawn at Z 0, inserted at the point's level, 45.3.
    // Block Z 0, times the insert's Z scale 1, plus 45.3 is 45.3 - for the
    // point and the circle alike; the point drawn at 0.001 is at 45.301.
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nPT\n 10\n0\n 20\n0\n 30\n0\n"
                             "  0\nPOINT\n 10\n0\n 20\n0\n 30\n0\n"
                             "  0\nCIRCLE\n 10\n0\n 20\n0\n 30\n0\n 40\n0.5\n"
                             "  0\nPOINT\n 10\n1\n 20\n0\n 30\n0.001\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  2\nPT\n 10\n100\n 20\n200\n 30\n45.3\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 3u);
    const Entity& dot = imported->entities[0];
    EXPECT_EQ(std::get<PointGeometry>(dot.geometry).position, Point2(100.0, 200.0));
    EXPECT_EQ(heightsOf(dot, 1)[0], 45.3);
    const Entity& ring = imported->entities[1];
    EXPECT_EQ(std::get<Circle2>(ring.geometry).center, Point2(100.0, 200.0));
    EXPECT_EQ(heightsOf(ring, 1)[0], 45.3);
    const auto raised = heightsOf(imported->entities[2], 1)[0];
    ASSERT_TRUE(raised.has_value());
    EXPECT_NEAR(*raised, 45.301, 1e-12);
}

TEST(DxfReaderBlocks, AZeroLevelThroughNestedInsertsIsTheirLevelsAndAnInsertAtZeroLeavesThePlan)
{
    // INNER: a point and a line at Z 0. OUTER (base Z 2) inserts INNER at
    // Z 5, so they are at 5 in OUTER. The drawing inserts OUTER at Z 100,
    // twice the height: 5 x 2 + (100 - 2 x 2) = 106. INNER inserted straight
    // at Z 0 stays in plan, with no level at all.
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nINNER\n 10\n0\n 20\n0\n 30\n0\n"
                             "  0\nPOINT\n 10\n0\n 20\n0\n 30\n0\n"
                             "  0\nLINE\n 10\n0\n 20\n0\n 30\n0\n 11\n1\n 21\n0\n 31\n0\n"
                             "  0\nENDBLK\n"
                             "  0\nBLOCK\n  2\nOUTER\n 10\n0\n 20\n0\n 30\n2\n"
                             "  0\nINSERT\n  2\nINNER\n 10\n0\n 20\n0\n 30\n5\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  2\nOUTER\n 10\n10\n 20\n10\n 30\n100\n 43\n2\n"
                             "  0\nINSERT\n  2\nINNER\n 10\n50\n 20\n50\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 4u);
    EXPECT_EQ(heightsOf(imported->entities[0], 1)[0], 106.0);
    const auto line = heightsOf(imported->entities[1], 2);
    EXPECT_EQ(line[0], 106.0);
    EXPECT_EQ(line[1], 106.0);
    EXPECT_TRUE(hasNoHeight(imported->entities[2]));
    EXPECT_TRUE(hasNoHeight(imported->entities[3]));
}

TEST(DxfReaderOcs, AnObjectSystemFacingDownPutsItsElevationAboveTheDatumNotBelow)
{
    // Extrusion (0,0,-1): object X is world -X, and object Z is world -Z, so
    // the writing program puts a level of +10 as elevation -10. Every entity
    // here is at height 10, and at the mirror in X of its object position.
    const std::string down = "210\n0\n220\n0\n230\n-1\n";
    const std::string text =
        "  0\nSECTION\n  2\nBLOCKS\n"
        "  0\nBLOCK\n  2\nDOT\n 10\n0\n 20\n0\n 30\n0\n"
        "  0\nPOINT\n 10\n0\n 20\n0\n 30\n0\n"
        "  0\nPOINT\n 10\n1\n 20\n0\n 30\n1\n"
        "  0\nENDBLK\n  0\nENDSEC\n"
        "  0\nSECTION\n  2\nENTITIES\n"
        "  0\nCIRCLE\n  8\nC\n 10\n-100\n 20\n50\n 30\n-10\n 40\n1\n" + down +
        "  0\nARC\n  8\nA\n 10\n-100\n 20\n60\n 30\n-10\n 40\n1\n 50\n0\n 51\n90\n" + down +
        "  0\nLWPOLYLINE\n  8\nL\n 90\n2\n 70\n0\n 38\n-10\n 10\n1\n 20\n1\n 10\n2\n 20\n1\n" +
        down +
        "  0\nPOLYLINE\n  8\nP\n 66\n1\n 10\n0\n 20\n0\n 30\n-10\n 70\n0\n" + down +
        "  0\nVERTEX\n  8\nP\n 10\n3\n 20\n3\n"
        "  0\nVERTEX\n  8\nP\n 10\n4\n 20\n3\n"
        "  0\nSEQEND\n"
        "  0\nTEXT\n  8\nT\n 10\n10\n 20\n5\n 30\n-10\n 40\n1\n  1\nT\n" + down +
        // The insert at object (-100,-50) elevation -7: world (100,-50) at
        // height 7. Block Z runs down with it: the point drawn at block Z 1
        // is at -(-7 + 1) = 6, one below the insert, and at world X 99.
        "  0\nINSERT\n  8\nI\n  2\nDOT\n 10\n-100\n 20\n-50\n 30\n-7\n" + down +
        // The plain system, for comparison: elevation 10 is height 10.
        "  0\nCIRCLE\n  8\nU\n 10\n0\n 20\n0\n 30\n10\n 40\n1\n"
        "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 8u);
    const auto& e = imported->entities;
    EXPECT_EQ(std::get<Circle2>(e[0].geometry).center, Point2(100.0, 50.0));
    EXPECT_EQ(heightsOf(e[0], 1)[0], 10.0);
    EXPECT_EQ(std::get<Arc2>(e[1].geometry).center, Point2(100.0, 60.0));
    EXPECT_EQ(heightsOf(e[1], 2)[0], 10.0);
    EXPECT_EQ(heightsOf(e[1], 2)[1], 10.0);
    const Polyline2& lw = std::get<Polyline2>(e[2].geometry);
    EXPECT_EQ(lw.vertices[0], Point2(-1.0, 1.0));
    EXPECT_EQ(lw.vertices[1], Point2(-2.0, 1.0));
    EXPECT_EQ(heightsOf(e[2], 2)[0], 10.0);
    EXPECT_EQ(heightsOf(e[2], 2)[1], 10.0);
    const Polyline2& two = std::get<Polyline2>(e[3].geometry);
    EXPECT_EQ(two.vertices[0], Point2(-3.0, 3.0));
    EXPECT_EQ(heightsOf(e[3], 2)[1], 10.0);
    EXPECT_EQ(std::get<TextGeometry>(e[4].geometry).position, Point2(-10.0, 5.0));
    EXPECT_EQ(heightsOf(e[4], 1)[0], 10.0);
    EXPECT_EQ(std::get<PointGeometry>(e[5].geometry).position, Point2(100.0, -50.0));
    EXPECT_EQ(heightsOf(e[5], 1)[0], 7.0);
    EXPECT_EQ(std::get<PointGeometry>(e[6].geometry).position, Point2(99.0, -50.0));
    EXPECT_EQ(heightsOf(e[6], 1)[0], 6.0);
    EXPECT_EQ(heightsOf(e[7], 1)[0], 10.0);
}

TEST(DxfReaderMText, AnMTextFacingDownStandsAtItsWorldInsertionPoint)
{
    // MTEXT is not an object system entity: its point (10/20/30) and its
    // direction (11/21/31) are world coordinates, whatever way it faces.
    const std::string down = "210\n0\n220\n0\n230\n-1\n";
    const std::string text =
        "  0\nSECTION\n  2\nENTITIES\n"
        // Bottom left (7), one line: the first baseline is the point itself,
        // (10,5), at height 7 - not -7. With the angle only, 0 is measured
        // in the plane facing down, whose X is world -X: the text runs
        // toward -X, a rotation of half a turn.
        "  0\nMTEXT\n 10\n10\n 20\n5\n 30\n7\n 40\n2\n 71\n7\n  1\nABC\n" + down +
        // With a direction vector, it is the world direction: (0,1), a
        // quarter turn, standing at (10,20).
        "  0\nMTEXT\n 10\n10\n 20\n20\n 40\n2\n 71\n7\n 11\n0\n 21\n1\n 31\n0\n  1\nDEF\n" + down +
        // Top left (1), two lines of height 3: the first baseline one height
        // below the point, the second a pitch of 5/3 x 3 = 5 further. The
        // text's up is still world +Y facing down - only its X is mirrored -
        // so the baselines are at y -3 and -8.
        "  0\nMTEXT\n 10\n0\n 20\n0\n 40\n3\n 71\n1\n  1\nG\\PH\n" + down +
        "  0\nENDSEC\n  0\nEOF\n";
    const auto imported = dxf::readDxf(text);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 4u);
    const Entity* abc = textSaying(*imported, "ABC");
    ASSERT_NE(abc, nullptr);
    const TextGeometry& first = std::get<TextGeometry>(abc->geometry);
    EXPECT_EQ(first.position, Point2(10.0, 5.0));
    EXPECT_EQ(std::cos(first.rotation), -1.0);
    EXPECT_EQ(heightsOf(*abc, 1)[0], 7.0);
    const Entity* def = textSaying(*imported, "DEF");
    ASSERT_NE(def, nullptr);
    const TextGeometry& second = std::get<TextGeometry>(def->geometry);
    EXPECT_EQ(second.position, Point2(10.0, 20.0));
    EXPECT_EQ(second.rotation, kPi / 2.0);
    const Entity* g = textSaying(*imported, "G");
    const Entity* h = textSaying(*imported, "H");
    ASSERT_NE(g, nullptr);
    ASSERT_NE(h, nullptr);
    EXPECT_EQ(std::get<TextGeometry>(g->geometry).position, Point2(0.0, -3.0));
    EXPECT_EQ(std::get<TextGeometry>(h->geometry).position, Point2(0.0, -8.0));
}
