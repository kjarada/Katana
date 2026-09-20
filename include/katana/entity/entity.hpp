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
