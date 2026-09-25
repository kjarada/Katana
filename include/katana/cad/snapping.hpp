#pragma once

// Object snapping (PLAN.MD Phases 08/09): endpoint, midpoint, centre,
// intersection, perpendicular, tangent, nearest and grid.
//
// Resolution order: the closest candidate among the "exact" modes (endpoint,
// midpoint, centre, intersection, perpendicular, tangent) wins; ties prefer the
// order just listed. Only if none is inside the aperture does Nearest apply,
// and only then Grid. This mirrors how drafters expect snaps to behave: an
// endpoint always beats merely being somewhere on the line.

#include <cstdint>
#include <optional>

#include "katana/cad/layer_overrides.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/spatial_index.hpp"

namespace katana::cad {

enum class SnapMode : std::uint32_t {
    None = 0,
    Endpoint = 1u << 0,
    Midpoint = 1u << 1,
    Center = 1u << 2,
    Intersection = 1u << 3,
    Perpendicular = 1u << 4, // needs SnapRequest::from
    Tangent = 1u << 5,       // needs SnapRequest::from
    Nearest = 1u << 6,
    Grid = 1u << 7,
};

using SnapModes = std::uint32_t;

[[nodiscard]] constexpr SnapModes operator|(SnapMode a, SnapMode b)
{
    return static_cast<SnapModes>(a) | static_cast<SnapModes>(b);
}
[[nodiscard]] constexpr SnapModes operator|(SnapModes a, SnapMode b)
{
    return a | static_cast<SnapModes>(b);
}
[[nodiscard]] constexpr bool hasMode(SnapModes modes, SnapMode mode)
{
    return (modes & static_cast<SnapModes>(mode)) != 0;
}

inline constexpr SnapModes kDefaultSnapModes =
    SnapMode::Endpoint | SnapMode::Midpoint | SnapMode::Center | SnapMode::Intersection;
inline constexpr SnapModes kAllSnapModes = 0xFF;

[[nodiscard]] const char* toString(SnapMode mode);

struct SnapRequest {
    katana::geometry::Point2 cursor;
    double aperture = 0.0; // model units; candidates farther than this are ignored
    SnapModes modes = kDefaultSnapModes;
    // Start of the segment being drawn; enables Perpendicular and Tangent.
    std::optional<katana::geometry::Point2> from{};
    double gridSpacing = 0.0; // model units; <= 0 disables Grid
    // The view the cursor is in; null for the document rule alone. A layer
    // hidden in that view is not snappable there - a snap to a line nobody can
    // see puts a point somewhere the user cannot explain.
    const LayerOverrides* view = nullptr;
};

struct SnapResult {
    katana::geometry::Point2 point;
    SnapMode mode = SnapMode::None;
    katana::entity::EntityId entity = katana::entity::kInvalidEntityId; // 0 for Grid
};

// Considers drawn entities only (visible, on visible layers; locked layers are
// valid snap targets). nullopt when nothing is within the aperture.
//
// `index`, when supplied, narrows the search to the entities near the cursor
// instead of scanning the model (PLAN.MD Phase 18). It must be in step with
// `model`; katana::cad::Document keeps one that is. The result is IDENTICAL
// either way - the index is a broad phase and every exact test still runs -
// which is asserted by a test that compares the two paths over random models.
[[nodiscard]] std::optional<SnapResult> snap(const katana::entity::Model& model,
                                             const SnapRequest& request,
                                             const katana::geometry::SpatialIndex* index = nullptr);

// The named point of the entity `snap` found that sits where it snapped - a
// line's or an arc's start, end or middle, an arc's or a circle's centre, a
// polyline's vertex or the middle of one of its segments, a point's or a
// text's position, a leader's tip or last vertex - as the reference that
// names it (entity/anchor.hpp), so annotation made from the snapped point can
// follow the entity as one made from a typed #id.end point does. Endpoint,
// Midpoint and Center snaps only: an Intersection, Perpendicular, Tangent or
// Nearest point is not a point one entity keeps as it is edited, and Grid is
// no entity's. Nullopt too when the entity is gone, or none of its named
// points is within the geometric tolerance of the snapped point.
[[nodiscard]] std::optional<katana::entity::AnchorRef> snapAnchor(const katana::entity::Model& model,
                                                                  const SnapResult& snap);

} // namespace katana::cad
