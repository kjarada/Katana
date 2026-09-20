#pragma once

// Geometry-generic operations on entities: validation, extents, picking
// distance and similarity transforms.

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/mat3.hpp"

namespace katana::entity {

// Rejects geometry that must never enter the model: non-finite coordinates,
// non-positive radii or text heights, arcs sweeping more than a full turn,
// polylines with fewer than two vertices.
[[nodiscard]] katana::core::Status validate(const Geometry& geometry);

// Text extents are approximated (0.6 * height per character) because the entity
// layer has no font metrics; renderers refine this.
[[nodiscard]] katana::geometry::Box2 boundingBox(const Geometry& geometry);

// Distance from `p` to the drawn geometry (curves, not filled areas). Text and
// dimensions measure to their baseline / dimension line.
[[nodiscard]] double distanceTo(const Geometry& geometry, const katana::geometry::Point2& p);

// Applies a 2D similarity transform (translation, rotation, uniform scale,
// mirror). Returns InvalidArgument for non-similarity matrices (shear or
// non-uniform scale), which would turn circles into ellipses. Mirrors reverse
// arc direction and dimension side; text is repositioned but never rendered
// back to front.
[[nodiscard]] katana::core::Result<Geometry> transformed(const Geometry& geometry,
                                                         const katana::math::Mat3& transform);

} // namespace katana::entity
