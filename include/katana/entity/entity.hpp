#pragma once

// Domain-independent entity model (PLAN.MD Phase 05).
//
// An Entity is a plain value: identity + geometry + presentation attributes +
// open-ended properties. It knows nothing about rendering, storage, Qt or
// surveying. The renderer consumes entities; it never owns them (Rule 3).

#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

using EntityId = std::uint64_t;
inline constexpr EntityId kInvalidEntityId = 0;

// Order matches the alternatives of Geometry.
enum class EntityType { Point, Line, Arc, Polyline, Circle, Text, Dimension };

[[nodiscard]] std::string_view toString(EntityType type);
[[nodiscard]] katana::core::Result<EntityType> entityTypeFromString(std::string_view name);

// True when `text` is well-formed UTF-8, rejecting overlong encodings and
// surrogate halves as the standard requires.
//
// Every string that reaches the model is checked, because the JSON writer used
// by project storage THROWS on invalid UTF-8 - and it is reached from a path
// that returns Status and promises not to throw. Text typed on a CP1252 console
// or read from an ANSI file is the ordinary way to produce such bytes, so this
// is a routine input to reject, not a hypothetical one.
[[nodiscard]] bool isValidUtf8(std::string_view text);

struct PointGeometry {
    katana::geometry::Point2 position;
    friend constexpr bool operator==(const PointGeometry&, const PointGeometry&) = default;
};

struct TextGeometry {
    katana::geometry::Point2 position; // left end of the baseline
    std::string text;
    double height = 2.5;   // model units
    double rotation = 0.0; // radians, counter-clockwise
    friend bool operator==(const TextGeometry&, const TextGeometry&) = default;
};

// Aligned linear dimension measuring |end - start|. The dimension line runs
// parallel to start->end, displaced by `offset` to its left (negative: right).
struct DimensionGeometry {
    katana::geometry::Point2 start;
    katana::geometry::Point2 end;
    double offset = 0.0;
    std::string textOverride; // empty: show the measured distance

    [[nodiscard]] double measurement() const { return start.distanceTo(end); }
    friend bool operator==(const DimensionGeometry&, const DimensionGeometry&) = default;
};

using Geometry =
    std::variant<PointGeometry, katana::geometry::Segment2, katana::geometry::Arc2,
                 katana::geometry::Polyline2, katana::geometry::Circle2, TextGeometry,
                 DimensionGeometry>;

[[nodiscard]] constexpr EntityType typeOf(const Geometry& geometry)
{
    return static_cast<EntityType>(geometry.index());
}

// ADDING A GEOMETRY KIND? READ THIS.
//
// 1. APPEND ONLY. Never insert an alternative in the middle and never reorder.
//    The variant INDEX is the on-disk kind byte (geometry_blob.cpp writes
//    `geometry.index()`), so reordering silently reinterprets every project
//    ever saved. Most shifts are caught by the payload-length checks, but two
//    kinds of equal payload size swap with no complaint at all and the drawing
//    reloads as the wrong shapes. GeometryBlobWireFormat pins the mapping.
// 2. Add the EntityType enumerator at the SAME ordinal - typeOf() casts the
//    index straight to it, so the two orders are one thing, not two.
// 3. docs/model.md lists every place that must then change. The exhaustive
//    visitors (validate, boundingBox, distanceTo, transformed, and the closed
//    visitor structs) will not compile until they are complete, which is
//    deliberate; the places that opted out of that discipline are listed there
//    because the compiler cannot help with them.
//
// This assert exists so that step 3 is not something you have to remember.
static_assert(std::variant_size_v<Geometry> == 7,
              "A geometry kind was added or removed. See docs/model.md for every place that "
              "must change - several of them fail SILENTLY, not at compile time. Update this "
              "count once you have been through the list.");

struct Color {
    std::uint8_t r = 255;
    std::uint8_t g = 255;
    std::uint8_t b = 255;
    std::uint8_t a = 255;

    // "#RRGGBB" or "#RRGGBBAA".
    [[nodiscard]] static katana::core::Result<Color> fromHex(std::string_view text);
    [[nodiscard]] std::string toHex() const; // alpha omitted when opaque

    friend constexpr bool operator==(const Color&, const Color&) = default;
};

using PropertyValue = std::variant<bool, std::int64_t, double, std::string>;
// Ordered so that iteration, serialisation and diffs are deterministic.
using PropertyMap = std::map<std::string, PropertyValue, std::less<>>;

// A property value as a person reads it: "true", "42", "31.25", the text
// itself. ONE definition, because the property panel, the command line and
// the attribute manager must not disagree about what a value says. A real
// is written with enough digits to read back as the same double, since a
// level shown as 31.2 that is actually 31.249 is a lie in survey work.
[[nodiscard]] std::string toString(const PropertyValue& value);
// "text", "integer", "real" or "boolean": what the attribute manager and
// `PROP SET ... <type>` name the four kinds.
[[nodiscard]] std::string_view typeName(const PropertyValue& value);

struct Entity {
    EntityId id = kInvalidEntityId;
    Geometry geometry{};
    std::string layer = "0";
    std::string style{};          // empty: ByLayer
    std::optional<Color> color{}; // empty: ByLayer
    bool visible = true;
    PropertyMap properties{}; // user / application attributes
    PropertyMap metadata{};   // provenance: source file, import time, author...

    [[nodiscard]] EntityType type() const { return typeOf(geometry); }

    friend bool operator==(const Entity&, const Entity&) = default;
};

} // namespace katana::entity
