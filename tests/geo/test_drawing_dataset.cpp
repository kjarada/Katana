// Drawing data to and from GDAL's feature tables
// (include/katana/interop/geo/drawing_dataset.hpp, docs/geoprocessing.md
// "Bindings"): the one conversion, and a result as one undo step.

#include <algorithm>
#include <cmath>
#include <numbers>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"

namespace {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
namespace cmd = katana::commands;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::gis::GeoPoint;
using katana::gis::GeometryKind;

Entity point(Point2 at)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{at};
    return entity;
}

Entity polyline(std::vector<Point2> vertices, bool closed)
{
    Entity entity;
    katana::geometry::Polyline2 line;
    line.vertices = std::move(vertices);
    line.closed = closed;
    entity.geometry = std::move(line);
    return entity;
}

Entity square(double x, double y, double side)
{
    return polyline({{x, y}, {x + side, y}, {x + side, y + side}, {x, y + side}}, true);
}

// The entities made in one step, and their ids in order.
std::vector<EntityId> draw(katana::cad::Document& document, std::vector<Entity> entities)
{
    EXPECT_TRUE(document.execute(cmd::createEntities(std::move(entities))).ok());
    return document.lastCreatedEntities();
}

const gp::FeatureTable* tableNamed(const gp::FeatureSet& set, const std::string& name)
{
    for (const gp::FeatureTable& table : set.tables) {
        if (table.name == name) {
            return &table;
        }
    }
    return nullptr;
}

std::size_t fieldIndex(const gp::FeatureTable& table, const std::string& name)
{
    for (std::size_t i = 0; i < table.fields.size(); ++i) {
        if (table.fields[i].name == name) {
            return i;
        }
    }
    return table.fields.size();
}

double ringArea(const std::vector<GeoPoint>& ring)
{
    double twice = 0.0;
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const GeoPoint& a = ring[i];
        const GeoPoint& b = ring[(i + 1) % ring.size()];
        twice += a.x * b.y - b.x * a.y;
    }
    return std::abs(twice) / 2.0;
}

TEST(DrawingDataset, KatanaIdAndTypedPropertiesBecomeTypedFields)
{
    katana::cad::Document document;
    Entity tree = point({1, 2});
    tree.properties["code"] = std::string("TREE");
    tree.properties["girth"] = 1.5;
    tree.properties["count"] = std::int64_t{3};
    tree.properties["alive"] = true;
    Entity pit = point({3, 4});
    pit.properties["code"] = std::string("PIT");
    pit.properties["count"] = std::int64_t{1};
    const auto ids = draw(document, {tree, pit});

    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    ASSERT_EQ(dataset->set.tables.size(), 1u);
    const gp::FeatureTable& points = dataset->set.tables.front();
    EXPECT_EQ(points.name, "points");
    EXPECT_EQ(points.kind, GeometryKind::Point);
    // katana_id first, the bookkeeping, then the properties sorted by key.
    std::vector<std::string> names;
    for (const gp::FieldDef& field : points.fields) {
        names.push_back(field.name);
    }
    EXPECT_EQ(names, (std::vector<std::string>{"katana_id", "layer", "style", "colour", "type",
                                               "alive", "code", "count", "girth"}));
    EXPECT_EQ(points.fields[0].type, gp::FieldType::Integer64);
    EXPECT_EQ(points.fields[fieldIndex(points, "alive")].type, gp::FieldType::Boolean);
    EXPECT_EQ(points.fields[fieldIndex(points, "code")].type, gp::FieldType::String);
    EXPECT_EQ(points.fields[fieldIndex(points, "count")].type, gp::FieldType::Integer64);
    EXPECT_EQ(points.fields[fieldIndex(points, "girth")].type, gp::FieldType::Real);
    ASSERT_EQ(points.features.size(), 2u);
    EXPECT_EQ(std::get<std::int64_t>(points.features[0].values[0]),
              static_cast<std::int64_t>(ids[0]));
    EXPECT_EQ(std::get<double>(points.features[0].values[fieldIndex(points, "girth")]), 1.5);
    // Absent is not zero: the pit has no girth.
    EXPECT_TRUE(std::holds_alternative<std::monostate>(
        points.features[1].values[fieldIndex(points, "girth")]));
    EXPECT_EQ(dataset->stats.matched, 2u);
    EXPECT_EQ(dataset->stats.used, 2u);
    EXPECT_EQ(dataset->stats.points, 2u);
}

TEST(DrawingDataset, AKeyOfSeveralTypesBecomesTextAndSaysSo)
{
    katana::cad::Document document;
    Entity a = point({0, 0});
    a.properties["tag"] = std::int64_t{7};
    Entity b = point({1, 1});
    b.properties["tag"] = std::string("seven");
    const auto ids = draw(document, {a, b});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable& points = dataset->set.tables.front();
    EXPECT_EQ(points.fields[fieldIndex(points, "tag")].type, gp::FieldType::String);
    EXPECT_EQ(std::get<std::string>(points.features[0].values[fieldIndex(points, "tag")]), "7");
    ASSERT_EQ(dataset->stats.warnings.size(), 1u);
    EXPECT_NE(dataset->stats.warnings.front().find("tag"), std::string::npos);
}

TEST(DrawingDataset, AVertexWithoutAHeightKeepsTheLineTwoDimensional)
{
    katana::cad::Document document;
    Entity partial = polyline({{0, 0}, {10, 0}, {20, 0}}, false);
    katana::entity::setHeights(partial.properties, {1.0, std::nullopt, 3.0});
    Entity whole = polyline({{0, 5}, {10, 5}, {20, 5}}, false);
    katana::entity::setHeights(whole.properties, {1.0, 2.0, 3.0});
    const auto ids = draw(document, {partial, whole});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable* lines = tableNamed(dataset->set, "lines");
    ASSERT_NE(lines, nullptr);
    ASSERT_EQ(lines->features.size(), 2u);
    EXPECT_FALSE(lines->features[0].parts.front().hasZ);
    ASSERT_TRUE(lines->features[1].parts.front().hasZ);
    EXPECT_EQ(lines->features[1].parts.front().parts.front()[1].z, 2.0);
}

TEST(DrawingDataset, HeightlessEntitiesAreCountedWhenHeightsAreRequired)
{
    katana::cad::Document document;
    Entity surveyed = point({0, 0});
    katana::entity::setHeights(surveyed.properties, {12.5});
    const auto ids = draw(document, {surveyed, point({5, 5})});
    igeo::DrawingDatasetOptions options;
    options.requireHeights = true;
    auto dataset = igeo::drawingDataset(document.model(), ids, options);
    ASSERT_TRUE(dataset.ok());
    EXPECT_EQ(dataset->stats.used, 1u);
    EXPECT_EQ(dataset->stats.skipped.at("heightless"), 1u);
    ASSERT_EQ(dataset->set.tables.size(), 1u);
    EXPECT_EQ(dataset->set.tables.front().features.front().parts.front().parts.front()[0].z, 12.5);
}

TEST(DrawingDataset, TextIsSkippedAndCounted)
{
    katana::cad::Document document;
    Entity text;
    katana::entity::TextGeometry geometry;
    geometry.text = "LOT 7";
    text.geometry = geometry;
    const auto ids = draw(document, {text, point({0, 0})});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    EXPECT_EQ(dataset->stats.matched, 2u);
    EXPECT_EQ(dataset->stats.used, 1u);
    EXPECT_EQ(dataset->stats.skipped.at("text"), 1u);
}

TEST(DrawingDataset, AnIdTheDrawingDoesNotHoldIsCountedMissing)
{
    katana::cad::Document document;
    auto dataset = igeo::drawingDataset(document.model(), {42});
    ASSERT_TRUE(dataset.ok());
    EXPECT_TRUE(dataset->set.tables.empty());
    EXPECT_EQ(dataset->stats.skipped.at("missing"), 1u);
}

TEST(DrawingDataset, ATaggedHoleJoinsItsExterior)
{
    // A 100 m square lot with a 20 m square hole tagged as IMPORT tags one:
    // 10000 - 400 = 9600 m2.
    katana::cad::Document document;
    Entity hole = square(40, 40, 20);
    hole.metadata["source.ring"] = std::string("hole");
    const auto ids = draw(document, {square(0, 0, 100), hole});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable* polygons = tableNamed(dataset->set, "polygons");
    ASSERT_NE(polygons, nullptr);
    ASSERT_EQ(polygons->features.size(), 1u);
    const auto& rings = polygons->features.front().parts.front().parts;
    ASSERT_EQ(rings.size(), 2u);
    EXPECT_DOUBLE_EQ(ringArea(rings[0]) - ringArea(rings[1]), 9600.0);
    EXPECT_EQ(dataset->stats.used, 2u);
    EXPECT_EQ(dataset->stats.polygons, 1u);
}

TEST(DrawingDataset, ABuildingInsideALotIsNotAHole)
{
    katana::cad::Document document;
    const auto ids = draw(document, {square(0, 0, 100), square(40, 40, 20)});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable* polygons = tableNamed(dataset->set, "polygons");
    ASSERT_NE(polygons, nullptr);
    ASSERT_EQ(polygons->features.size(), 2u);
    for (const gp::Feature& feature : polygons->features) {
        EXPECT_EQ(feature.parts.front().parts.size(), 1u);
    }
}

TEST(DrawingDataset, AnArcIsChordsWithinTheTolerance)
{
    // A circle of radius 10 at a 1 mm sagitta: every chord's middle is
    // within 1 mm of the circle, so no chord subtends more than
    // 2 acos(1 - 0.001 / 10).
    katana::cad::Document document;
    Entity circle;
    circle.geometry = katana::geometry::Circle2{{0, 0}, 10.0};
    const auto ids = draw(document, {circle});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    const auto& ring = dataset->set.tables.front().features.front().parts.front().parts.front();
    const double largest = 2.0 * std::acos(1.0 - 0.001 / 10.0);
    EXPECT_GE(static_cast<double>(ring.size()), std::ceil(2.0 * std::numbers::pi / largest));
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const GeoPoint& a = ring[i];
        const GeoPoint& b = ring[(i + 1) % ring.size()];
        const double middle = std::hypot((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        EXPECT_LE(10.0 - middle, 0.001 + 1e-12);
    }
}

TEST(DrawingDataset, DrawingToDatasetToEntitiesIsTheIdentity)
{
    katana::cad::Document document;
    Entity marked = point({1, 2});
    marked.properties["code"] = std::string("PEG");
    Entity kerb = polyline({{0, 0}, {10, 0}, {10, 5}}, false);
    kerb.properties["length.design"] = 15.25;
    Entity lot = square(0, 0, 30);
    lot.properties["lot"] = std::int64_t{7};
    Entity segment;
    segment.geometry = katana::geometry::Segment2{{0, 0}, {3, 4}};
    const auto ids = draw(document, {marked, kerb, lot, segment});

    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    igeo::ResultOptions options;
    options.targetLayer = "copy";
    options.operation = "identity";
    auto plan = igeo::resultCommand(document.model(), dataset->set, options);
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->created, 4u);
    // Three tables, so a layer each under the target.
    EXPECT_EQ(plan->layers, (std::vector<std::string>{"copy/points", "copy/lines", "copy/polygons"}));
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    const auto copies = document.lastCreatedEntities();
    ASSERT_EQ(copies.size(), 4u);
    for (const EntityId copyId : copies) {
        const Entity* copy = document.model().entities.find(copyId);
        ASSERT_NE(copy, nullptr);
        const auto source = std::get<std::int64_t>(copy->properties.at("gis.source"));
        const Entity* original = document.model().entities.find(static_cast<EntityId>(source));
        ASSERT_NE(original, nullptr);
        EXPECT_EQ(copy->geometry, original->geometry);
        EXPECT_EQ(std::get<std::string>(copy->properties.at("gis.op")), "identity");
        auto properties = copy->properties;
        properties.erase("gis.source");
        properties.erase("gis.op");
        EXPECT_EQ(properties, original->properties);
    }
}

TEST(DrawingDataset, AllResultsAreOneUndoStep)
{
    katana::cad::Document document;
    const auto ids = draw(document, {point({0, 0}), square(0, 0, 5)});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    igeo::ResultOptions options;
    options.targetLayer = "gis/result";
    auto plan = igeo::resultCommand(document.model(), dataset->set, options);
    ASSERT_TRUE(plan.ok());
    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.model().entities.size(), 4u);
    EXPECT_TRUE(document.model().layers.contains("gis/result/points"));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 2u);
    EXPECT_FALSE(document.model().layers.contains("gis/result/points"));
}

TEST(DrawingDataset, AHoleWrittenBackIsTaggedAndJoinsItsAreaAgain)
{
    gp::FeatureTable polygons;
    polygons.name = "polygons";
    polygons.kind = GeometryKind::Polygon;
    katana::gis::VectorGeometry lot;
    lot.kind = GeometryKind::Polygon;
    lot.parts = {{{0, 0, 0}, {100, 0, 0}, {100, 100, 0}, {0, 100, 0}, {0, 0, 0}},
                 {{40, 40, 0}, {60, 40, 0}, {60, 60, 0}, {40, 60, 0}, {40, 40, 0}}};
    polygons.features.push_back(gp::Feature{{lot}, {}});
    katana::cad::Document document;
    igeo::ResultOptions options;
    options.targetLayer = "gis/lot";
    auto plan = igeo::resultCommand(document.model(), gp::FeatureSet{{polygons}}, options);
    ASSERT_TRUE(plan.ok());
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    const auto rings = document.lastCreatedEntities();
    ASSERT_EQ(rings.size(), 2u);
    const Entity* exterior = document.model().entities.find(rings[0]);
    const Entity* hole = document.model().entities.find(rings[1]);
    EXPECT_EQ(std::get<std::string>(exterior->properties.at("gis.ring")), "exterior");
    EXPECT_EQ(std::get<std::string>(hole->properties.at("gis.ring")), "hole");
    EXPECT_EQ(exterior->properties.at("gis.part"), hole->properties.at("gis.part"));
    // The part names the exterior by its id.
    EXPECT_EQ(std::get<std::int64_t>(exterior->properties.at("gis.part")),
              static_cast<std::int64_t>(rings[0]));
    auto again = igeo::drawingDataset(document.model(), rings);
    ASSERT_TRUE(again.ok());
    ASSERT_EQ(again->set.tables.front().features.size(), 1u);
    const auto& parts = again->set.tables.front().features.front().parts.front().parts;
    ASSERT_EQ(parts.size(), 2u);
    EXPECT_DOUBLE_EQ(ringArea(parts[0]) - ringArea(parts[1]), 9600.0);
}

TEST(DrawingDataset, ADateAResultMadeIsADateWhenReadAgain)
{
    // A result's Date field becomes a text property tagged gis.type.<key>,
    // so the drawing read as features gives the field its type again.
    gp::FeatureTable points;
    points.name = "points";
    points.kind = GeometryKind::Point;
    points.fields = {{"surveyed", gp::FieldType::Date}, {"note", gp::FieldType::String}};
    katana::gis::VectorGeometry at;
    at.kind = GeometryKind::Point;
    at.parts = {{{1, 2, 0}}};
    points.features.push_back(gp::Feature{
        {at}, {gp::FieldValue(std::string("2024-05-01")), gp::FieldValue(std::string("x"))}});
    katana::cad::Document document;
    igeo::ResultOptions options;
    options.targetLayer = "gis/dated";
    auto plan = igeo::resultCommand(document.model(), gp::FeatureSet{{points}}, options);
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    const auto made = document.lastCreatedEntities();
    ASSERT_EQ(made.size(), 1u);
    auto again = igeo::drawingDataset(document.model(), made);
    ASSERT_TRUE(again.ok());
    const gp::FeatureTable& table = again->set.tables.front();
    const auto typeOf = [&](const std::string& name) {
        for (const gp::FieldDef& field : table.fields) {
            if (field.name == name) {
                return field.type;
            }
        }
        ADD_FAILURE() << "no field " << name;
        return gp::FieldType::Boolean;
    };
    EXPECT_EQ(typeOf("surveyed"), gp::FieldType::Date);
    EXPECT_EQ(typeOf("note"), gp::FieldType::String);
}

TEST(DrawingDataset, UpdateGeometryKeepsTheEntitysId)
{
    katana::cad::Document document;
    const auto ids = draw(document, {polyline({{0, 0}, {10, 0}, {20, 5}}, false)});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    for (GeoPoint& vertex : dataset->set.tables.front().features.front().parts.front().parts.front()) {
        vertex.x += 5.0;
    }
    igeo::ResultOptions options;
    options.mode = igeo::ResultMode::UpdateGeometry;
    auto plan = igeo::resultCommand(document.model(), dataset->set, options);
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->updated, 1u);
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    const Entity* moved = document.model().entities.find(ids.front());
    ASSERT_NE(moved, nullptr);
    const auto& line = std::get<katana::geometry::Polyline2>(moved->geometry);
    EXPECT_EQ(line.vertices.front(), Point2(5, 0));
    EXPECT_EQ(document.model().entities.size(), 1u);
}

TEST(DrawingDataset, SetPropertiesWritesTheFieldsOnTheEntitiesTheyCameFrom)
{
    katana::cad::Document document;
    const auto ids = draw(document, {square(0, 0, 10)});
    gp::FeatureTable zones;
    zones.name = "zones";
    zones.kind = GeometryKind::Polygon;
    zones.fields = {{"katana_id", gp::FieldType::Integer64}, {"mean", gp::FieldType::Real}};
    zones.features.push_back(gp::Feature{{}, {std::int64_t(ids.front()), 3.5}});
    igeo::ResultOptions options;
    options.mode = igeo::ResultMode::SetProperties;
    options.propertyPrefix = "zone.";
    auto plan = igeo::resultCommand(document.model(), gp::FeatureSet{{zones}}, options);
    ASSERT_TRUE(plan.ok());
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    EXPECT_EQ(std::get<double>(document.model().entities.find(ids.front())->properties.at("zone.mean")),
              3.5);
}

TEST(DrawingDataset, AResultWithoutKatanaIdCannotChangeEntitiesInPlace)
{
    katana::cad::Document document;
    gp::FeatureTable loose;
    loose.name = "loose";
    loose.kind = GeometryKind::Point;
    loose.features.push_back(gp::Feature{{}, {}});
    igeo::ResultOptions options;
    options.mode = igeo::ResultMode::SetProperties;
    auto plan = igeo::resultCommand(document.model(), gp::FeatureSet{{loose}}, options);
    ASSERT_FALSE(plan.ok());
    EXPECT_EQ(plan.error().code, katana::core::ErrorCode::InvalidArgument);
}

TEST(DrawingDataset, ReplaceDeletesTheSourcesInTheSameStep)
{
    katana::cad::Document document;
    const auto ids = draw(document, {square(0, 0, 10)});
    auto dataset = igeo::drawingDataset(document.model(), ids);
    ASSERT_TRUE(dataset.ok());
    igeo::ResultOptions options;
    options.targetLayer = "gis/replaced";
    options.deleteSources = true;
    auto plan = igeo::resultCommand(document.model(), dataset->set, options);
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(plan->created, 1u);
    EXPECT_EQ(plan->deleted, 1u);
    ASSERT_TRUE(document.execute(std::move(plan->command)).ok());
    EXPECT_FALSE(document.model().entities.contains(ids.front()));
    EXPECT_EQ(document.model().entities.size(), 1u);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.model().entities.contains(ids.front()));
    EXPECT_EQ(document.model().entities.size(), 1u);
}

TEST(DrawingDataset, AResultOfNothingChangesNothing)
{
    katana::cad::Document document;
    igeo::ResultOptions options;
    options.targetLayer = "gis/nothing";
    auto plan = igeo::resultCommand(document.model(), gp::FeatureSet{}, options);
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(plan->command, nullptr);
    EXPECT_EQ(plan->created, 0u);
}

TEST(DrawingDataset, ATargetThatIsNoLayerPathIsRefused)
{
    katana::cad::Document document;
    igeo::ResultOptions options;
    options.targetLayer = "gis//bad";
    auto plan = igeo::resultCommand(document.model(), gp::FeatureSet{}, options);
    ASSERT_FALSE(plan.ok());
    EXPECT_EQ(plan.error().code, katana::core::ErrorCode::InvalidArgument);
}

} // namespace
