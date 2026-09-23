#include <gtest/gtest.h>

#include <iterator>
#include <variant>

#include <cmath>
#include <limits>
#include <vector>

#include "katana/entity/entity_database.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/serialization.hpp"
#include "katana/entity/tables.hpp"
#include "support/property.hpp"

using namespace katana::entity;
using katana::core::ErrorCode;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::Mat3;
using katana::math::nearlyEqual;

namespace {

Entity lineEntity(double x0, double y0, double x1, double y1)
{
    Entity entity;
    entity.geometry = Segment2{Point2(x0, y0), Point2(x1, y1)};
    return entity;
}

std::vector<Geometry> oneOfEachGeometry()
{
    return {PointGeometry{Point2(1.5, -2.25)},
            Segment2{Point2(0, 0), Point2(10, 5)},
            Arc2{Point2(1, 2), 3.5, 0.25, -1.75},
            Polyline2{{Point2(0, 0), Point2(4, 0), Point2(4, 3)}, true},
            Circle2{Point2(-1, -1), 2.0},
            TextGeometry{Point2(5, 5), "BM \"A\" 102.45", 2.5, 0.5},
            DimensionGeometry{Point2(0, 0), Point2(10, 0), 3.0, "10.00 m"}};
}

} // namespace

// ---- Entity / Geometry -------------------------------------------------------

TEST(Entity, TypeFollowsGeometryAlternative)
{
    const auto geometries = oneOfEachGeometry();
    const EntityType expected[] = {EntityType::Point, EntityType::Line, EntityType::Arc,
                                   EntityType::Polyline, EntityType::Circle, EntityType::Text,
                                   EntityType::Dimension};
    // The loop below walks `geometries` and indexes `expected`, so growing one
    // without the other was an out-of-bounds read - which a normal build may
    // well survive, failing only under the sanitizer job. Both are also pinned
    // to the variant, so a kind added without extending this corpus fails here
    // rather than leaving a test named "one of each" quietly covering six of
    // seven.
    ASSERT_EQ(geometries.size(), std::variant_size_v<Geometry>)
        << "oneOfEachGeometry() is missing a geometry kind";
    ASSERT_EQ(std::size(expected), geometries.size());
    for (std::size_t i = 0; i < geometries.size(); ++i) {
        EXPECT_EQ(typeOf(geometries[i]), expected[i]);
        const auto parsed = entityTypeFromString(toString(expected[i]));
        ASSERT_TRUE(parsed.ok());
        EXPECT_EQ(*parsed, expected[i]);
    }
    EXPECT_FALSE(entityTypeFromString("Spline").ok());
}

TEST(EntityGeometry, ValidateRejectsDegenerateAndNonFinite)
{
    for (const Geometry& geometry : oneOfEachGeometry()) {
        EXPECT_TRUE(validate(geometry).ok());
    }
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    EXPECT_FALSE(validate(PointGeometry{Point2(nan, 0)}).ok());
    EXPECT_FALSE(validate(Segment2{Point2(0, 0), Point2(inf, 0)}).ok());
    EXPECT_FALSE(validate(Segment2{Point2(1, 1), Point2(1, 1)}).ok());        // zero length
    EXPECT_FALSE(validate(Circle2{Point2(0, 0), 0.0}).ok());                  // zero radius
    EXPECT_FALSE(validate(Circle2{Point2(0, 0), -1.0}).ok());                 // negative radius
    EXPECT_FALSE(validate(Arc2{Point2(0, 0), 1.0, 0.0, 0.0}).ok());           // zero sweep
    EXPECT_FALSE(validate(Arc2{Point2(0, 0), 1.0, 0.0, 7.0}).ok());           // > full turn
    EXPECT_FALSE(validate(Polyline2{{Point2(0, 0)}, false}).ok());            // one vertex
    EXPECT_FALSE(validate(Polyline2{{Point2(0, 0), Point2(0, 0)}, false}).ok()); // duplicates
    EXPECT_FALSE(validate(TextGeometry{Point2(0, 0), "", 2.5, 0.0}).ok());    // empty text
    EXPECT_FALSE(validate(TextGeometry{Point2(0, 0), "A", 0.0, 0.0}).ok());   // zero height
    EXPECT_FALSE(validate(DimensionGeometry{Point2(1, 1), Point2(1, 1), 1.0, ""}).ok());
    EXPECT_EQ(validate(Circle2{Point2(0, 0), 0.0}).error().code, ErrorCode::InvalidGeometry);
}

TEST(EntityGeometry, BoundingBoxAndPickDistance)
{
    const Geometry circle = Circle2{Point2(0, 0), 2.0};
    EXPECT_EQ(boundingBox(circle).min, Point2(-2, -2));
    EXPECT_DOUBLE_EQ(distanceTo(circle, Point2(5, 0)), 3.0);
    EXPECT_DOUBLE_EQ(distanceTo(circle, Point2(0, 0)), 2.0); // to the curve, not the disc

    const Geometry dimension = DimensionGeometry{Point2(0, 0), Point2(10, 0), 3.0, ""};
    EXPECT_DOUBLE_EQ(distanceTo(dimension, Point2(5, 3)), 0.0); // on the dimension line
    EXPECT_TRUE(boundingBox(dimension).contains(Point2(0, 0)));
    EXPECT_TRUE(boundingBox(dimension).contains(Point2(10, 3)));

    const Geometry text = TextGeometry{Point2(0, 0), "ABCD", 10.0, kHalfPi}; // reads upwards
    const auto box = boundingBox(text);
    EXPECT_NEAR(box.max.y, 24.0, 1e-9);  // 4 glyphs * 0.6 * 10 along +y
    EXPECT_NEAR(box.min.x, -10.0, 1e-9); // height extends to the left of the baseline
}

TEST(EntityGeometry, SimilarityTransformsEveryGeometryKind)
{
    const Mat3 move = Mat3::translation(Vec2(100, 200));
    for (const Geometry& geometry : oneOfEachGeometry()) {
        const auto moved = transformed(geometry, move);
        ASSERT_TRUE(moved.ok()) << moved.error().describe();
        const auto before = boundingBox(geometry);
        const auto after = boundingBox(*moved);
        EXPECT_TRUE(nearlyEqual(after.min, before.min + Vec2(100, 200), 1e-12));
        EXPECT_TRUE(nearlyEqual(after.max, before.max + Vec2(100, 200), 1e-12));
    }
}

TEST(EntityGeometry, RotationScaleAndMirrorOfAnArc)
{
    const Arc2 arc{Point2(0, 0), 2.0, 0.0, kHalfPi}; // (2,0) -> (0,2), counter-clockwise

    const auto rotated = transformed(arc, Mat3::rotation(kHalfPi));
    ASSERT_TRUE(rotated.ok());
    const Arc2& r = std::get<Arc2>(*rotated);
    EXPECT_TRUE(nearlyEqual(r.startPoint(), Point2(0, 2)));
    EXPECT_TRUE(nearlyEqual(r.endPoint(), Point2(-2, 0)));
    EXPECT_DOUBLE_EQ(r.sweep, kHalfPi);

    const auto scaled = transformed(arc, Mat3::scaling(3.0, 3.0));
    ASSERT_TRUE(scaled.ok());
    EXPECT_DOUBLE_EQ(std::get<Arc2>(*scaled).radius, 6.0);

    // Mirror about the y axis: the arc must still join the mirrored endpoints
    // through the mirrored midpoint, which requires reversing the sweep.
    const auto mirrored = transformed(arc, Mat3::reflection(Point2(0, 0), Vec2(0, 1)));
    ASSERT_TRUE(mirrored.ok());
    const Arc2& m = std::get<Arc2>(*mirrored);
    EXPECT_TRUE(nearlyEqual(m.startPoint(), Point2(-2, 0)));
    EXPECT_TRUE(nearlyEqual(m.endPoint(), Point2(0, 2)));
    EXPECT_TRUE(nearlyEqual(m.midpoint(), Point2(-std::sqrt(2.0), std::sqrt(2.0))));
    EXPECT_DOUBLE_EQ(m.sweep, -kHalfPi);
}

TEST(EntityGeometry, MirrorFlipsDimensionSide)
{
    const DimensionGeometry dimension{Point2(0, 0), Point2(10, 0), 3.0, ""}; // line at y = +3
    const auto mirrored = transformed(dimension, Mat3::reflection(Point2(0, 0), Vec2(1, 0)));
    ASSERT_TRUE(mirrored.ok());
    EXPECT_DOUBLE_EQ(std::get<DimensionGeometry>(*mirrored).offset, -3.0);
    EXPECT_DOUBLE_EQ(distanceTo(*mirrored, Point2(5, -3)), 0.0); // now at y = -3
}

TEST(EntityGeometry, RejectsNonSimilarityTransforms)
{
    const Geometry circle = Circle2{Point2(0, 0), 2.0};
    const auto stretched = transformed(circle, Mat3::scaling(2.0, 1.0));
    ASSERT_FALSE(stretched.ok());
    EXPECT_EQ(stretched.error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(transformed(circle, Mat3(1, 0.5, 0, 0, 1, 0, 0, 0, 1)).ok()); // shear
    EXPECT_FALSE(transformed(circle, Mat3::scaling(0.0, 0.0)).ok());            // collapse
    EXPECT_FALSE(transformed(circle, Mat3::scaling(1e-9, 1e-9)).ok()); // below tolerance
}

TEST(EntityGeometry, TransformRoundTripProperty)
{
    katana::test::Random random;
    for (int i = 0; i < 200; ++i) {
        const Point2 pivot(random.real(-100, 100), random.real(-100, 100));
        const double angle = random.real(-kPi, kPi);
        const Mat3 forward = Mat3::rotationAbout(pivot, angle);
        const Mat3 backward = Mat3::rotationAbout(pivot, -angle);
        for (const Geometry& geometry : oneOfEachGeometry()) {
            const auto there = transformed(geometry, forward);
            ASSERT_TRUE(there.ok());
            const auto back = transformed(*there, backward);
            ASSERT_TRUE(back.ok());
            const auto expected = boundingBox(geometry);
            const auto actual = boundingBox(*back);
            EXPECT_TRUE(nearlyEqual(actual.min, expected.min, 1e-9));
            EXPECT_TRUE(nearlyEqual(actual.max, expected.max, 1e-9));
        }
    }
}

// ---- Color -------------------------------------------------------------------

TEST(EntityColor, HexRoundTripAndErrors)
{
    const auto opaque = Color::fromHex("#1a2B3c");
    ASSERT_TRUE(opaque.ok());
    EXPECT_EQ(*opaque, (Color{0x1A, 0x2B, 0x3C, 0xFF}));
    EXPECT_EQ(opaque->toHex(), "#1A2B3C");

    const auto translucent = Color::fromHex("#FF000080");
    ASSERT_TRUE(translucent.ok());
    EXPECT_EQ(translucent->a, 0x80);
    EXPECT_EQ(translucent->toHex(), "#FF000080");

    for (const char* bad : {"", "#", "123456", "#12345", "#GGGGGG", "#1234567", "#123456789"}) {
        EXPECT_FALSE(Color::fromHex(bad).ok()) << bad;
    }
}

// ---- EntityDatabase ----------------------------------------------------------

TEST(EntityDatabase, AssignsMonotonicIdsThatAreNeverReused)
{
    EntityDatabase database;
    const auto first = database.add(lineEntity(0, 0, 1, 1));
    const auto second = database.add(lineEntity(0, 0, 2, 2));
    ASSERT_TRUE(first.ok() && second.ok());
    EXPECT_EQ(*first, 1u);
    EXPECT_EQ(*second, 2u);

    ASSERT_TRUE(database.remove(*second).ok());
    const auto third = database.add(lineEntity(0, 0, 3, 3));
    ASSERT_TRUE(third.ok());
    EXPECT_EQ(*third, 3u); // 2 is retired for good

    database.clear();
    EXPECT_TRUE(database.empty());
    const auto afterClear = database.add(lineEntity(0, 0, 4, 4));
    ASSERT_TRUE(afterClear.ok());
    EXPECT_EQ(*afterClear, 4u);
}

TEST(EntityDatabase, RejectsInvalidEntities)
{
    EntityDatabase database;
    const auto degenerate = database.add(lineEntity(1, 1, 1, 1));
    ASSERT_FALSE(degenerate.ok());
    EXPECT_EQ(degenerate.error().code, ErrorCode::InvalidGeometry);

    Entity noLayer = lineEntity(0, 0, 1, 1);
    noLayer.layer.clear();
    EXPECT_FALSE(database.add(noLayer).ok());
    EXPECT_TRUE(database.empty());
    EXPECT_EQ(database.nextId(), 1u); // failed adds do not burn ids
}

TEST(EntityDatabase, InsertRestoresARemovedEntityUnderItsOriginalId)
{
    EntityDatabase database;
    const EntityId id = *database.add(lineEntity(0, 0, 5, 5));
    auto removed = database.remove(id);
    ASSERT_TRUE(removed.ok());
    EXPECT_FALSE(database.contains(id));

    ASSERT_TRUE(database.insert(*removed).ok());
    ASSERT_NE(database.find(id), nullptr);
    EXPECT_EQ(*database.find(id), *removed);

    EXPECT_EQ(database.insert(*removed).error().code, ErrorCode::AlreadyExists);
    EXPECT_FALSE(database.insert(lineEntity(0, 0, 1, 1)).ok()); // id 0 is invalid

    Entity loaded = lineEntity(0, 0, 1, 1);
    loaded.id = 50;
    ASSERT_TRUE(database.insert(loaded).ok());
    EXPECT_EQ(database.nextId(), 51u); // counter jumps past loaded ids
}

TEST(EntityDatabase, ReplaceAndRemoveReportMissingEntities)
{
    EntityDatabase database;
    Entity ghost = lineEntity(0, 0, 1, 1);
    ghost.id = 99;
    EXPECT_EQ(database.replace(ghost).error().code, ErrorCode::NotFound);
    EXPECT_EQ(database.remove(99).error().code, ErrorCode::NotFound);

    const EntityId id = *database.add(lineEntity(0, 0, 1, 1));
    Entity changed = *database.find(id);
    changed.layer = "Survey";
    changed.geometry = Circle2{Point2(0, 0), 5.0};
    ASSERT_TRUE(database.replace(changed).ok());
    EXPECT_EQ(database.find(id)->type(), EntityType::Circle);

    changed.geometry = Circle2{Point2(0, 0), -5.0};
    EXPECT_FALSE(database.replace(changed).ok());
    EXPECT_EQ(database.find(id)->type(), EntityType::Circle); // untouched by the failed replace
    EXPECT_DOUBLE_EQ(std::get<Circle2>(database.find(id)->geometry).radius, 5.0);
}

TEST(EntityDatabase, QueriesAreOrderedAndLayerAware)
{
    EntityDatabase database;
    Entity onSurvey = lineEntity(0, 0, 10, 0);
    onSurvey.layer = "Survey";
    const EntityId a = *database.add(lineEntity(0, 0, 1, 1));
    const EntityId b = *database.add(onSurvey);
    const EntityId c = *database.add(lineEntity(-5, -5, 0, 0));

    EXPECT_EQ(database.ids(), (std::vector<EntityId>{a, b, c}));
    EXPECT_EQ(database.idsOnLayer("Survey"), (std::vector<EntityId>{b}));
    EXPECT_EQ(database.countOnLayer("0"), 2u);
    EXPECT_EQ(database.countOnLayer("missing"), 0u);

    const auto bounds = database.bounds();
    EXPECT_EQ(bounds.min, Point2(-5, -5));
    EXPECT_EQ(bounds.max, Point2(10, 1));
    EXPECT_TRUE(EntityDatabase{}.bounds().empty());

    std::vector<EntityId> visited;
    database.forEach([&](const Entity& entity) { visited.push_back(entity.id); });
    EXPECT_EQ(visited, database.ids());
}

TEST(EntityDatabase, ReportsEveryMutationToTheObserver)
{
    EntityDatabase database;
    std::vector<ChangeEvent> events;
    database.setObserver([&](const ChangeEvent& event) { events.push_back(event); });

    const EntityId id = *database.add(lineEntity(0, 0, 1, 1));
    Entity changed = *database.find(id);
    changed.visible = false;
    ASSERT_TRUE(database.replace(changed).ok());
    ASSERT_TRUE(database.remove(id).ok());
    database.clear();
    EXPECT_FALSE(database.remove(id).ok()); // failures are silent to observers

    ASSERT_EQ(events.size(), 4u);
    EXPECT_EQ(events[0].kind, ChangeKind::EntityAdded);
    EXPECT_EQ(events[1].kind, ChangeKind::EntityModified);
    EXPECT_EQ(events[2].kind, ChangeKind::EntityRemoved);
    EXPECT_EQ(events[3].kind, ChangeKind::Cleared);
    EXPECT_EQ(events[0].id, id);
}

// ---- tables ------------------------------------------------------------------

TEST(LayerDatabase, DefaultLayerAlwaysExistsAndCannotBeRemoved)
{
    LayerDatabase layers;
    ASSERT_NE(layers.find("0"), nullptr);
    EXPECT_EQ(layers.remove("0").error().code, ErrorCode::InvalidArgument);

    ASSERT_TRUE(layers.add(Layer{"Survey", Color{255, 0, 0, 255}, true, false, "dashed", 0.5}).ok());
    EXPECT_EQ(layers.add(Layer{"Survey"}).error().code, ErrorCode::AlreadyExists);
    EXPECT_FALSE(layers.add(Layer{""}).ok());
    EXPECT_EQ(layers.names(), (std::vector<std::string>{"0", "Survey"}));

    Layer hidden = *layers.find("Survey");
    hidden.visible = false;
    ASSERT_TRUE(layers.update(hidden).ok());
    EXPECT_FALSE(layers.find("Survey")->visible);
    EXPECT_EQ(layers.update(Layer{"Missing"}).error().code, ErrorCode::NotFound);

    const auto removed = layers.remove("Survey");
    ASSERT_TRUE(removed.ok());
    EXPECT_EQ(removed->linetype, "dashed"); // returned intact so undo can restore it
    EXPECT_EQ(layers.size(), 1u);

    ASSERT_TRUE(layers.add(Layer{"Temp"}).ok());
    layers.reset();
    EXPECT_EQ(layers.names(), (std::vector<std::string>{"0"}));
}

TEST(StyleDatabase, AddUpdateRemove)
{
    StyleDatabase styles;
    ASSERT_TRUE(styles.add(Style{"Boundary", Color{0, 255, 0, 255}, 0.7, "continuous"}).ok());
    EXPECT_FALSE(styles.add(Style{"Boundary"}).ok());
    EXPECT_FALSE(styles.add(Style{"Bad", std::nullopt, -1.0, "continuous"}).ok());
    Style thinner = *styles.find("Boundary");
    thinner.lineWeight = 0.35;
    ASSERT_TRUE(styles.update(thinner).ok());
    EXPECT_DOUBLE_EQ(styles.find("Boundary")->lineWeight, 0.35);
    ASSERT_TRUE(styles.remove("Boundary").ok());
    EXPECT_EQ(styles.remove("Boundary").error().code, ErrorCode::NotFound);
}

TEST(PropertyDatabase, DefinedPropertiesAreTyped)
{
    PropertyDatabase properties;
    ASSERT_TRUE(properties
                    .define(PropertyDefinition{"elevation", PropertyType::Real, "Ground level (m)",
                                               PropertyValue{0.0}})
                    .ok());
    EXPECT_FALSE(properties.define(PropertyDefinition{"elevation", PropertyType::Real, "", {}}).ok());
    EXPECT_FALSE(properties
                     .define(PropertyDefinition{"code", PropertyType::Text, "",
                                                PropertyValue{std::int64_t{1}}})
                     .ok()); // default of the wrong type

    EXPECT_TRUE(properties.validate("elevation", PropertyValue{102.45}).ok());
    EXPECT_FALSE(properties.validate("elevation", PropertyValue{std::string("high")}).ok());
    EXPECT_FALSE(properties.validate("elevation", PropertyValue{std::nan("")}).ok());
    EXPECT_TRUE(properties.validate("free-form", PropertyValue{std::string("anything")}).ok());

    ASSERT_TRUE(properties.undefine("elevation").ok());
    EXPECT_TRUE(properties.validate("elevation", PropertyValue{std::string("now free")}).ok());
}

// ---- serialisation -----------------------------------------------------------

TEST(EntitySerialization, EveryGeometryKindRoundTripsExactly)
{
    for (const Geometry& geometry : oneOfEachGeometry()) {
        const auto json = geometryToJson(geometry);
        ASSERT_TRUE(json.ok()) << json.error().describe();
        const auto parsed = geometryFromJson(*json);
        ASSERT_TRUE(parsed.ok()) << *json << " -> " << parsed.error().describe();
        EXPECT_EQ(*parsed, geometry) << *json;
    }
}

TEST(EntitySerialization, DoublesSurviveBitForBit)
{
    // Values with no short decimal form, at survey coordinate magnitudes.
    katana::test::Random random;
    for (int i = 0; i < 500; ++i) {
        const Segment2 segment{Point2(random.real(4e5, 6e5), random.real(4e6, 6e6)),
                               Point2(random.real(-1e-6, 1e-6), 1.0 / 3.0)};
        const auto written = geometryToJson(segment);
        ASSERT_TRUE(written.ok()) << written.error().describe();
        const auto parsed = geometryFromJson(*written);
        ASSERT_TRUE(parsed.ok());
        EXPECT_EQ(std::get<Segment2>(*parsed), segment); // exact operator==
    }
}

TEST(EntitySerialization, FullEntityRoundTripKeepsPropertyTypes)
{
    Entity entity;
    entity.id = 42;
    entity.geometry = Circle2{Point2(3, 4), 5.0};
    entity.layer = "Control";
    entity.style = "Monument";
    entity.color = Color{10, 20, 30, 255};
    entity.visible = false;
    entity.properties = {{"code", std::string("IP")},
                         {"elevation", 102.5},
                         {"whole", 7.0}, // a real that happens to be integral
                         {"order", std::int64_t{2}},
                         {"verified", true}};
    entity.metadata = {{"source", std::string("field-book-7.csv")}};

    const auto written = entityToJson(entity);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    const auto parsed = entityFromJson(*written);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(*parsed, entity);
    EXPECT_TRUE(std::holds_alternative<double>(parsed->properties.at("whole")));
    EXPECT_TRUE(std::holds_alternative<std::int64_t>(parsed->properties.at("order")));
}

TEST(EntitySerialization, MalformedOrInvalidInputIsRejected)
{
    EXPECT_EQ(geometryFromJson("not json").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(geometryFromJson("{}").error().code, ErrorCode::ParseFailure); // no type
    EXPECT_EQ(geometryFromJson(R"({"type":"Spline"})").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(geometryFromJson(R"({"type":"Circle","center":[0,0]})").error().code,
              ErrorCode::ParseFailure); // missing radius
    EXPECT_EQ(geometryFromJson(R"({"type":"Circle","center":[0,0,0],"radius":1})").error().code,
              ErrorCode::ParseFailure); // point with three numbers
    EXPECT_EQ(geometryFromJson(R"({"type":"Circle","center":[0,0],"radius":"big"})").error().code,
              ErrorCode::ParseFailure); // wrong value type
    EXPECT_EQ(geometryFromJson(R"({"type":"Circle","center":[0,0],"radius":-1})").error().code,
              ErrorCode::InvalidGeometry); // well formed but invalid
    EXPECT_FALSE(propertiesFromJson(R"({"nested":{"a":1}})").ok());
    EXPECT_FALSE(propertiesFromJson("[1,2]").ok());
    EXPECT_FALSE(entityFromJson(R"({"id":1,"color":"red","geometry":{"type":"Point","position":[0,0]}})").ok());
}

// ---- text encoding ---------------------------------------------------------------------------

// Regression. The JSON writer used by project storage throws on text that is
// not valid UTF-8, and it is reached from a save path that returns Status and
// promises not to throw - so `TEXT 0,0 2 "cafe"` typed on a CP1252 console used
// to abort the process (0xC0000409), losing the whole drawing and leaving a
// stale rollback journal behind. Bad bytes are now refused on the way in.
TEST(EntityUtf8, WellFormedSequencesAreAccepted)
{
    EXPECT_TRUE(isValidUtf8(""));
    EXPECT_TRUE(isValidUtf8("plain ASCII"));
    EXPECT_TRUE(isValidUtf8("caf\xc3\xa9"));             // U+00E9, two bytes
    EXPECT_TRUE(isValidUtf8("\xe2\x88\x86"));            // U+2206, three bytes
    EXPECT_TRUE(isValidUtf8("\xf0\x9f\x93\x90"));        // U+1F4D0, four bytes
    EXPECT_TRUE(isValidUtf8(std::string("a\0b", 3)));    // an embedded NUL is valid UTF-8
}

TEST(EntityUtf8, MalformedSequencesAreRejected)
{
    EXPECT_FALSE(isValidUtf8("caf\xe9"));         // CP1252 e-acute: a lone lead byte
    EXPECT_FALSE(isValidUtf8("\x80"));            // continuation byte with no lead
    EXPECT_FALSE(isValidUtf8("\xc3"));            // truncated two-byte sequence
    EXPECT_FALSE(isValidUtf8("\xe2\x88"));        // truncated three-byte sequence
    EXPECT_FALSE(isValidUtf8("\xc3\x28"));        // lead followed by a non-continuation
    EXPECT_FALSE(isValidUtf8("\xff\xfe"));        // never valid lead bytes
    // Overlong encodings: the standard requires the shortest form, and accepting
    // these is a classic way to smuggle characters past a filter.
    EXPECT_FALSE(isValidUtf8("\xc0\xaf"));        // overlong '/'
    EXPECT_FALSE(isValidUtf8("\xe0\x80\xaf"));    // overlong again
    EXPECT_FALSE(isValidUtf8("\xf0\x80\x80\xaf"));
    // Surrogate halves are not characters and must not appear in UTF-8.
    EXPECT_FALSE(isValidUtf8("\xed\xa0\x80"));    // U+D800
    EXPECT_FALSE(isValidUtf8("\xed\xbf\xbf"));    // U+DFFF
    // Beyond U+10FFFF.
    EXPECT_FALSE(isValidUtf8("\xf4\x90\x80\x80"));
    EXPECT_FALSE(isValidUtf8("\xf5\x80\x80\x80"));
}

TEST(EntityUtf8, InvalidTextIsRefusedByGeometryValidation)
{
    TextGeometry text;
    text.position = Point2(0.0, 0.0);
    text.height = 2.0;
    text.text = "caf\xe9"; // CP1252, not UTF-8

    const auto status = validate(Geometry{text});
    ASSERT_FALSE(status.ok()) << "invalid UTF-8 reached the model";
    EXPECT_EQ(status.error().code, ErrorCode::InvalidGeometry);

    text.text = "caf\xc3\xa9"; // the same word, properly encoded
    EXPECT_TRUE(validate(Geometry{text}).ok());
}

TEST(EntityUtf8, InvalidPropertyStringsAreRefused)
{
    PropertyDatabase properties;
    EXPECT_FALSE(properties.validate("note", PropertyValue{std::string("caf\xe9")}).ok());
    EXPECT_TRUE(properties.validate("note", PropertyValue{std::string("caf\xc3\xa9")}).ok());
    // The NAME is written to JSON too.
    EXPECT_FALSE(properties.validate("na\xe9me", PropertyValue{std::string("ok")}).ok());
}

TEST(EntityUtf8, DescriptionsAreRefusedAsNamesAreWhenTheyAreNotUtf8)
{
    // Audit MOD-12: a description is saved as JSON like the name beside it,
    // and was the one string of these records nothing checked.
    const std::string latin1 = "caf\xe9"; // CP1252, not UTF-8
    HatchPattern hatch;
    hatch.name = "grass";
    hatch.solid = true;
    hatch.description = latin1;
    EXPECT_EQ(validate(hatch).error().code, ErrorCode::InvalidArgument);
    hatch.description = "caf\xc3\xa9";
    EXPECT_TRUE(validate(hatch).ok());

    PropertyDatabase properties;
    PropertyDefinition note;
    note.name = "note";
    note.description = latin1;
    EXPECT_EQ(properties.define(note).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(properties.find("note"), nullptr) << "nothing defined";
    note.description = "caf\xc3\xa9";
    EXPECT_TRUE(properties.define(note).ok());

    // An alignment with no PIs is refused anyway; the description is looked
    // at first, and says so.
    Alignment road;
    road.name = "road";
    road.description = latin1;
    const auto refused = validate(road);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().message, "alignment description is not valid UTF-8");
    road.description = "caf\xc3\xa9";
    ASSERT_FALSE(validate(road).ok());
    EXPECT_NE(validate(road).error().message, "alignment description is not valid UTF-8");
}

// The writers report failure rather than throwing, whatever they are handed.
// Nothing that survives validation can reach this, but a save path must not be
// able to abort the process even if a guard is one day bypassed.
TEST(EntitySerialization, WritersReportFailureInsteadOfThrowing)
{
    TextGeometry text;
    text.position = Point2(0.0, 0.0);
    text.height = 2.0;
    text.text = "caf\xe9";

    const auto written = geometryToJson(Geometry{text});
    ASSERT_FALSE(written.ok()) << "dump() should have refused this";
    EXPECT_EQ(written.error().code, ErrorCode::Internal);

    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(1.0, 2.0)};
    entity.properties.emplace("note", std::string("caf\xe9"));
    EXPECT_FALSE(entityToJson(entity).ok());
}

// ---- heights (the elevation / elevations properties) -----------------------------

TEST(EntityHeights, OneHeightSharedByEveryVertexIsWrittenOnceAsElevation)
{
    PropertyMap properties;
    setHeights(properties, {32.5, 32.5, 32.5});
    ASSERT_EQ(properties.size(), 1u);
    EXPECT_EQ(std::get<double>(properties.at(std::string(kElevationProperty))), 32.5);
    // Read back for any vertex count: a single height is every vertex's.
    const auto heights = heightsOf(properties, 3);
    ASSERT_EQ(heights.size(), 3u);
    for (const auto& z : heights) {
        ASSERT_TRUE(z.has_value());
        EXPECT_EQ(*z, 32.5);
    }
}

TEST(EntityHeights, AStringSurveyedAtSomeVerticesKeepsItsGapsAsNull)
{
    // 0.1 is not exact in binary; the written text must still read back as the
    // same double, which is what formatExactReal promises.
    PropertyMap properties;
    setHeights(properties, {10.25, std::nullopt, 0.1});
    EXPECT_EQ(std::get<std::string>(properties.at(std::string(kElevationsProperty))),
              "10.25 null 0.1");
    EXPECT_FALSE(properties.contains(std::string(kElevationProperty)));

    const auto heights = heightsOf(properties, 3);
    ASSERT_EQ(heights.size(), 3u);
    EXPECT_EQ(heights[0], std::optional<double>(10.25));
    EXPECT_FALSE(heights[1].has_value()) << "a gap is no height, not zero";
    EXPECT_EQ(heights[2], std::optional<double>(0.1));
}

TEST(EntityHeights, NoHeightAnywhereWritesNothingAndClearsAStaleOne)
{
    PropertyMap properties;
    properties.emplace(std::string(kElevationProperty), 5.0);
    properties.emplace(std::string(kElevationsProperty), std::string("1 2"));
    properties.emplace("note", std::string("kept"));
    setHeights(properties, {std::nullopt, std::nullopt});
    EXPECT_FALSE(properties.contains(std::string(kElevationProperty)));
    EXPECT_FALSE(properties.contains(std::string(kElevationsProperty)));
    EXPECT_TRUE(properties.contains("note")) << "only the two height properties are touched";

    const auto heights = heightsOf(properties, 2);
    ASSERT_EQ(heights.size(), 2u);
    EXPECT_FALSE(heights[0].has_value());
    EXPECT_FALSE(heights[1].has_value());
}

TEST(EntityHeights, RewritingReplacesTheOtherFormRatherThanLeavingItToWin)
{
    // A list written first and a uniform height written after: the reader
    // prefers a list of the right length, so a stale one left behind would
    // silently override the new height.
    PropertyMap properties;
    setHeights(properties, {1.0, 2.0});
    setHeights(properties, {7.0, 7.0});
    EXPECT_FALSE(properties.contains(std::string(kElevationsProperty)));
    EXPECT_EQ(heightsOf(properties, 2)[1], std::optional<double>(7.0));
}

TEST(EntityHeights, AListOfTheWrongLengthIsNotStretchedToFit)
{
    // Three heights for a string that now has four vertices: which vertex
    // lost or gained one is unknowable, so the list is passed over and the
    // single elevation, if any, stands in.
    PropertyMap properties;
    properties.emplace(std::string(kElevationsProperty), std::string("1 2 3"));
    for (const auto& z : heightsOf(properties, 4)) {
        EXPECT_FALSE(z.has_value());
    }
    properties.emplace(std::string(kElevationProperty), std::int64_t{12});
    for (const auto& z : heightsOf(properties, 4)) {
        EXPECT_EQ(z, std::optional<double>(12.0)) << "an integer elevation is a height too";
    }
}

TEST(EntityHeights, NonFiniteHeightsAreNoHeight)
{
    PropertyMap properties;
    setHeights(properties, {std::numeric_limits<double>::quiet_NaN(),
                            std::numeric_limits<double>::infinity()});
    EXPECT_TRUE(properties.empty());

    setHeights(properties, {4.0, std::numeric_limits<double>::quiet_NaN()});
    EXPECT_EQ(std::get<std::string>(properties.at(std::string(kElevationsProperty))), "4 null");

    PropertyMap written;
    written.emplace(std::string(kElevationsProperty), std::string("4 nan inf 5"));
    const auto heights = heightsOf(written, 4);
    EXPECT_EQ(heights[0], std::optional<double>(4.0));
    EXPECT_FALSE(heights[1].has_value());
    EXPECT_FALSE(heights[2].has_value());
    EXPECT_EQ(heights[3], std::optional<double>(5.0));
}

TEST(EntityHeights, AnEmptyVertexListWritesAndReadsNothing)
{
    PropertyMap properties;
    setHeights(properties, {});
    EXPECT_TRUE(properties.empty());
    EXPECT_TRUE(heightsOf(properties, 0).empty());
}
