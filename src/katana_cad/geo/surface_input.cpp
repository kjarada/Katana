// What a surface built from the drawing is built from (surface_input.hpp).

#include "katana/cad/geo/surface_input.hpp"

#include <optional>
#include <string>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::geo {

namespace {

using katana::geometry::Point2;
using katana::geometry::Point3;

// The vertices of one run of line: a polyline's, a line's two ends.
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
