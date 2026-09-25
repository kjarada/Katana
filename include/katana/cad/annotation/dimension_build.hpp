#pragma once

// Making dimensions of every kind from the points a person or an agent gives
// (docs/annotation.md, "Dimensions"): the arithmetic DIM LINEAR, DIM
// ANGULAR, DIM BASELINE and the rest share with the interactive tool, so
// that a dimension typed and a dimension drawn come out alike.
//
// Each point may be ANCHORED to another entity's point (entity/anchor.hpp);
// the dimension then carries the reference and follows that entity through
// every edit (associative.hpp).

#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::annotation {

// A point, and the entity point it came from when it came from one.
struct AnchoredPoint {
    katana::geometry::Point2 point{};
    katana::entity::AnchorRef ref{};
};

// An anchored point from `ref` resolved against the model; NotFound when the
// entity is gone, InvalidArgument when it offers no such point.
[[nodiscard]] katana::core::Result<AnchoredPoint> anchoredPoint(const katana::entity::Model& model,
                                                                const katana::entity::AnchorRef& ref);

// The point an entity is referred to by when no point is named: a point's or
// a text's position, a line's or a polyline's start, an arc's or a circle's
// centre.
[[nodiscard]] katana::entity::AnchorPoint defaultAnchor(const katana::entity::Geometry& geometry);

// Aligned: |b - a|, the dimension line through `at`.
[[nodiscard]] katana::core::Result<katana::entity::DimensionGeometry>
alignedDimension(const AnchoredPoint& a, const AnchoredPoint& b, const katana::geometry::Point2& at);

// Linear along `angle` (radians); with no angle, horizontal or vertical as
// `at` is dragged: a line through `at` above or below the two points gives a
// horizontal dimension, one beside them a vertical one - how every CAD
// program's linear dimension reads the cursor.
[[nodiscard]] katana::core::Result<katana::entity::DimensionGeometry>
linearDimension(const AnchoredPoint& a, const AnchoredPoint& b, const katana::geometry::Point2& at,
                std::optional<double> angle = std::nullopt);

// Angular at `vertex` between the rays through `first` and `second`,
// measuring the side `at` is on (the arc through it); with no `at`, the
// counter-clockwise angle from first to second on the shorter ray's radius.
[[nodiscard]] katana::core::Result<katana::entity::DimensionGeometry>
angularDimension(const AnchoredPoint& vertex, const AnchoredPoint& first,
                 const AnchoredPoint& second,
                 const std::optional<katana::geometry::Point2>& at = std::nullopt);

// Angular between two line entities: the vertex is where they meet
// (extended), each ray runs to the line's end on the side `at` is, and the
// dimension follows both lines - the vertex is worked out again from them
// whenever they move. InvalidArgument for parallel lines.
[[nodiscard]] katana::core::Result<katana::entity::DimensionGeometry>
angularBetweenLines(const katana::entity::Model& model, katana::entity::EntityId first,
                    katana::entity::EntityId second, const katana::geometry::Point2& at);

// Radius or diameter of a circle or an arc entity, through `at` (its
// direction from the centre; the text sits beyond the curve by as much as
// `at` is outside it). Follows the curve's centre and radius.
[[nodiscard]] katana::core::Result<katana::entity::DimensionGeometry>
radialDimension(const katana::entity::Model& model, katana::entity::EntityId curve,
                const katana::geometry::Point2& at, bool diameter);

// Ordinate of `feature` from `datum`, its leader ending at `at`. With no
// axis given, a leader more vertical than horizontal gives the X ordinate
// (the value written up or down the sheet is an easting) and otherwise the
// Y, as AutoCAD's DIMORDINATE decides.
[[nodiscard]] katana::core::Result<katana::entity::DimensionGeometry>
ordinateDimension(const AnchoredPoint& datum, const AnchoredPoint& feature,
                  const katana::geometry::Point2& at, std::optional<bool> xAxis = std::nullopt);

// Baseline chain: from `base`'s first point to each of `points`, each
// dimension line `spacing` further out than the one before (base first).
// Continued chain: end to end, from `base`'s second point, every dimension
// line on base's. Both for Aligned and Linear bases.
[[nodiscard]] katana::core::Result<std::vector<katana::entity::DimensionGeometry>>
baselineDimensions(const katana::entity::DimensionGeometry& base,
                   const std::vector<AnchoredPoint>& points, double spacing);
[[nodiscard]] katana::core::Result<std::vector<katana::entity::DimensionGeometry>>
continuedDimensions(const katana::entity::DimensionGeometry& base,
                    const std::vector<AnchoredPoint>& points);

} // namespace katana::cad::annotation
