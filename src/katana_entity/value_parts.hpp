#pragma once

// The values a label (label_values.cpp) and a leader (leader_values.cpp) both
// read off an entity, worked out in ONE place so that a label and a leader
// on the same line cannot print two bearings for it. Internal to
// katana_entity.

#include <cstddef>
#include <optional>
#include <span>
#include <string>

#include "katana/entity/entity.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity::detail {

// A whole-circle bearing: clockwise from grid north (+y), [0, 2 pi).
[[nodiscard]] double bearingOf(const katana::geometry::Vec2& along);

// id, layer, code (the first of `codeProperties` the entity carries, in its
// properties or its metadata), prop.NAME for every property, and point and
// description as text.
void addCommonValues(const Entity& entity, std::span<const std::string> codeProperties,
                     LabelValues& values);

// A straight piece from `from` to `to`, segment `index` (0-based; printed
// 1-based): bearing (when it has a direction), distance, length, dx, dy,
// segment, and dz and grade when both ends have a height.
void addSegmentValues(LabelValues& values, const katana::geometry::Point2& from,
                      const katana::geometry::Point2& to, std::size_t index,
                      std::optional<double> zFrom, std::optional<double> zTo);

// An arc: radius, length, delta, and chord, bearing (of the chord) and
// tangent where they mean something (docs/annotation.md).
void addArcValues(LabelValues& values, const katana::geometry::Arc2& arc);

} // namespace katana::entity::detail
