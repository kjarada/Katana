// What a surface built from the drawing is built from (surface_input.hpp).

#include "katana/cad/geo/surface_input.hpp"

#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/curves2d.hpp"

namespace katana::cad::geo {

namespace {

using katana::geometry::Point2;
using katana::geometry::Point3;

// The vertices of one run of line: a polyline's, a line's two ends, a
// curve polyline's chord points.
void addLine(const std::vector<Point2>& vertices, bool closed,
             const std::vector<std::optional<double>>& heights, SurfaceInput& out,
             bool& gavePoints)
{
    katana::terrain::Breakline breakline;
    bool whole = true;
    const auto flush = [&] {
        if (breakline.vertices.size() >= 2) {
            out.input.breaklines.push_back(breakline);
        }
        breakline.vertices.clear();
    };
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const std::optional<double>& z = heights[i];
        if (!z) {
            ++out.heightlessVertices;
            whole = false;
            flush();
            continue;
        }
        const Point3 vertex(vertices[i].x, vertices[i].y, *z);
        breakline.vertices.push_back(vertex);
        out.input.points.push_back(vertex);
        gavePoints = true;
    }
    // Closing only makes sense for a ring that lost none of its vertices.
    breakline.closed = closed && whole;
    flush();
}

} // namespace

SurfaceInput surfaceInput(const katana::entity::Model& model,
                          const std::vector<katana::entity::EntityId>& ids)
{
    SurfaceInput out;
    for (const katana::entity::EntityId id : ids) {
        const katana::entity::Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        bool gavePoints = false;
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity->geometry)) {
            if (const auto z = katana::entity::heightsOf(entity->properties, 1).front()) {
                out.input.points.emplace_back(point->position.x, point->position.y, *z);
                gavePoints = true;
            } else {
                ++out.heightlessVertices;
            }
        } else if (const auto* polyline =
                       std::get_if<katana::geometry::Polyline2>(&entity->geometry)) {
            addLine(polyline->vertices, polyline->closed,
                    katana::entity::heightsOf(entity->properties, polyline->vertices.size()), out,
                    gavePoints);
        } else if (const auto* curve =
                       std::get_if<katana::geometry::CurvePolyline2>(&entity->geometry)) {
            // A string with arcs holds its heights itself; its arcs go in as
            // chords at the drawing's tolerance, each chord point at the
            // height interpolated by length along its segment and none where
            // an end has none (CurvePolyline2::heightAtStation) - the rule
            // the window's Surface From Drawing had for it.
            auto walk = curve->tessellateWithHeights(katana::geometry::kCurveChordTolerance);
            if (curve->closed && walk.size() > 1) {
                walk.pop_back(); // the closing point repeats the first
            }
            std::vector<Point2> positions;
            std::vector<std::optional<double>> heights;
            positions.reserve(walk.size());
            heights.reserve(walk.size());
            for (const auto& step : walk) {
                positions.push_back(step.position);
                heights.push_back(step.height);
            }
            addLine(positions, curve->closed, heights, out, gavePoints);
        } else if (const auto* line = std::get_if<katana::geometry::Segment2>(&entity->geometry)) {
            addLine({line->start, line->end}, false,
                    katana::entity::heightsOf(entity->properties, 2), out, gavePoints);
        } else {
            ++out.skipped[katana::core::lowered(
                std::string(katana::entity::toString(entity->type())))];
            continue;
        }
        if (gavePoints) {
            ++out.used;
        } else {
            ++out.skipped["heightless"];
        }
    }
    return out;
}

} // namespace katana::cad::geo
