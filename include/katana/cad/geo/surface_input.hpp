#pragma once

// What a surface built from the drawing is built from (docs/terrain.md,
// "Surfaces on every front end"): the points and breaklines of the entities a
// scope took, at the heights their `elevation` / `elevations` properties give
// them (entity::heightsOf, the one reader).
//
// Here, in katana_cad, rather than in the window, because katana_cli,
// katana_mcp and the window all build a surface from the drawing through the
// SURFACE FROM verb, and the rules must have one definition. They are the
// ones the window's Surface From Drawing kept, with one change:
//
//   - A point is a survey point; a polyline, and a line, a breakline through
//     its vertices, closed when the polyline is.
//   - A vertex with no height is left out and counted, and a breakline is
//     broken there: a null is "not surveyed", and triangulating it at zero
//     would dig a pit to the datum under it.
//   - A ring closes only when it lost none of its vertices.
//   - The change: an entity with no height property at all is left out and
//     counted too, as a vertex with a null height is. The window used to put
//     it on the datum (z = 0) - a whole 2D drawing then became a flat surface
//     at 0, and one plain line among levelled strings a trench to the datum
//     (absent is not zero, CLAUDE.md section 11).
//
// Everything else a scope takes (text, arcs, circles, dimensions ...) is
// skipped and counted by its type, never silently dropped.

#include <cstddef>
#include <map>
#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/terrain/tin_builder.hpp"

namespace katana::entity {
struct Model;
}

namespace katana::cad::geo {

struct SurfaceInput {
    katana::terrain::TinInput input;
    // Entities that gave at least one point.
    std::size_t used = 0;
    // Vertices left out for having no height, over every entity.
    std::size_t heightlessVertices = 0;
    // Entities left out, by reason: "heightless" (a point or line of which
    // no vertex has a height), or the type of an entity no surface is built
    // from ("text", "arc", "circle" ...).
    std::map<std::string, std::size_t> skipped;
};

// The survey points and breaklines of `ids`, in their order; an id the model
// has not is passed over. Deterministic: the same ids give the same input.
[[nodiscard]] SurfaceInput surfaceInput(const katana::entity::Model& model,
                                        const std::vector<katana::entity::EntityId>& ids);

} // namespace katana::cad::geo
