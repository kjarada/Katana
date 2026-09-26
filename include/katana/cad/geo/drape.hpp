#pragma once

// Draping the drawing on a surface or an elevation raster (docs/terrain.md,
// "Sampling and drape"): each point, and each vertex of a line or a
// polyline, is given the height of the ground under it, through
// entity::setHeights - the one writer of heights - as ONE undo step.
//
// Here, in katana_cad, rather than beside the verb, because what a drape
// changes and how it is undone are the drawing's rules; where the heights
// come from is not. The ground is a callback: the DRAPE verb hands a TIN's
// elevationAt or a GDAL raster sampler (gis/raster_sampling.hpp) through it,
// so katana_cad never sees GDAL.
//
// The rules:
//   - A point takes the height at its position; a line its two ends; a
//     polyline every vertex (a closed one's too). A curve polyline (one with
//     arcs) takes a height at every vertex too, written into its geometry,
//     where it keeps its heights (docs/drawing.md): its arcs stay arcs, and
//     the height between two vertices is the geometry's own rule, linear by
//     length. Nothing else has vertices a height belongs to - an arc, a
//     circle, an ellipse, a spline, a text - and those are left as they are
//     and counted by type.
//   - A vertex off the ground, or on a hole in it, is left WITHOUT a height
//     and counted: the drape defines the heights of what it takes, and a
//     height kept from before would be another surface's, mixed in
//     silently. Absent is not zero.
//   - Heights are computed first (drapeHeights, pure, on a worker) and
//     written later (drapeCommand, on the thread that owns the drawing), so
//     a front end can run the sampling as a background job.

#include <cstddef>
#include <functional>
#include <map>
#include <optional>
#include <stop_token>
#include <string>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {
struct Model;
}

namespace katana::cad::geo {

// The height of the ground at a plan position; none off it.
using HeightAt = std::function<std::optional<double>(const katana::geometry::Point2&)>;

// The plan positions whose heights an entity carries, in the order
// entity::heightsOf reads them: a point's position, a line's two ends, a
// polyline's vertices - a curve polyline's in its vertex order, the order
// its geometry holds its heights in. Empty for any other entity.
[[nodiscard]] std::vector<katana::geometry::Point2>
heightVertices(const katana::entity::Entity& entity);

// The heights one entity is to be given, one per vertex.
struct DrapedEntity {
    katana::entity::EntityId id = katana::entity::kInvalidEntityId;
    std::vector<std::optional<double>> heights;
};

struct DrapeHeights {
    std::vector<DrapedEntity> entities; // in the order they were given
    std::size_t vertices = 0;           // over every entity draped
    std::size_t verticesOff = 0;        // of which off the ground: left heightless
    // Entities no vertex of which is on the ground.
    std::size_t entitiesOff = 0;
    // Entities left as they are, by type ("arc", "circle", "text" ...).
    std::map<std::string, std::size_t> skipped;
    // Whether `stop` was asked before every entity was sampled; the heights
    // are then incomplete and must not be written.
    bool stopped = false;
};

// The ground under every vertex of `entities`. Deterministic: the same
// entities and ground give the same heights. Checks `stop` between
// entities.
[[nodiscard]] DrapeHeights drapeHeights(const std::vector<katana::entity::Entity>& entities,
                                        const HeightAt& ground, const std::stop_token& stop = {});

struct DrapeCommand {
    // Null when nothing changes: every entity already has those heights.
    katana::commands::CommandPtr command;
    std::size_t changed = 0;   // entities whose heights the command changes
    std::size_t unchanged = 0; // entities already at those heights
    std::size_t missing = 0;   // entities the model no longer has
};

// The ONE command that gives each entity of `heights` its heights
// (entity::setHeights on a copy of its properties, then the `elevation` /
// `elevations` properties set or removed to match), named `name` - the verb
// line, so the history says what ran. It reads the model and changes
// nothing: the caller executes it, which makes the drape one undo step.
[[nodiscard]] DrapeCommand drapeCommand(const katana::entity::Model& model,
                                        const std::vector<DrapedEntity>& heights,
                                        const std::string& name);

} // namespace katana::cad::geo
