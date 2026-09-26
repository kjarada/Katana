#pragma once

// The arithmetic and the commands the draw tools and the draw verbs share
// (docs/drawing.md, "The draw tools" and "The command line"): the tangent
// arc a polyline's next arc segment is, the construction line or ray a
// base point and a direction make, the two sides of a double line, and the
// ONE command that creates what was drawn. A tool collects points from
// clicks and a verb from its arguments; what they make is decided here, so
// PLINE ... ARC ... typed by an agent and the Polyline tool's Arc mode draw
// the same arc.

#include <optional>
#include <string>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/curves2d.hpp"

namespace katana::cad {

// The direction of travel at the end of `path`'s last segment - an arc's end
// tangent, not its chord - or +x when it has fewer than two vertices.
[[nodiscard]] katana::geometry::Vec2 endHeading(const katana::geometry::CurvePolyline2& path);

// The bulge of the arc from `path`'s last vertex to `to` that leaves tangent
// to endHeading(): the chord makes half the sweep with the tangent. 0 for an
// empty path or a `to` on the last vertex.
[[nodiscard]] double tangentBulge(const katana::geometry::CurvePolyline2& path,
                                  const katana::geometry::Point2& to);

// A construction line through `through` along `direction` (a unit vector),
// kConstructionReach either way, or a ray from it the same reach forward.
[[nodiscard]] katana::geometry::Segment2
constructionSegment(const katana::geometry::Point2& through, const katana::geometry::Vec2& direction,
                    bool ray);

// ONE command that creates `lines` on the construction layer - named
// CREATE_RAY or CREATE_XLINE - and the layer first when `model` has none, so
// the aid and its layer are one undo step.
[[nodiscard]] katana::commands::CommandPtr
createConstruction(const katana::entity::Model& model, katana::commands::EntityAttributes attributes,
                   const std::vector<katana::geometry::Segment2>& lines, bool ray);

// The two sides of a double line of `width` along `path`, left then right;
// fewer than two when the path is too tight for the width to offset.
[[nodiscard]] std::vector<katana::geometry::Polyline2>
doubleLineSides(const katana::geometry::Polyline2& path, double width);

// An entity of `geometry` in `attributes`' layer, style and colour.
[[nodiscard]] katana::entity::Entity drawnEntity(katana::entity::Geometry geometry,
                                                 const katana::commands::EntityAttributes& attributes);

// ONE command named `name` that adds `entities`.
[[nodiscard]] katana::commands::CommandPtr createDrawn(std::string name,
                                                       std::vector<katana::entity::Entity> entities);

} // namespace katana::cad
