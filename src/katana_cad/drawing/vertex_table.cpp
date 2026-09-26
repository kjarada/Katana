#include "katana/cad/drawing/vertex_table.hpp"

#include <cmath>
#include <cstdio>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace geo = katana::geometry;
using katana::core::ErrorCode;
using katana::geometry::CurvePolyline2;
using katana::geometry::Vec2;

const char* toString(VertexColumn column)
{
    switch (column) {
    case VertexColumn::Index:
        return "Index";
    case VertexColumn::Easting:
        return "Easting";
    case VertexColumn::Northing:
        return "Northing";
    case VertexColumn::Height:
        return "Height";
    case VertexColumn::Bulge:
        return "Bulge";
    case VertexColumn::Bearing:
        return "Bearing";
    case VertexColumn::Distance:
        return "Distance";
    }
    return "Unknown";
}

std::optional<VertexColumn> vertexColumnFromString(std::string_view name)
{
    const std::string lowered = katana::core::lowered(katana::core::trimmed(name));
    if (lowered == "e" || lowered == "x" || lowered == "easting") {
        return VertexColumn::Easting;
    }
    if (lowered == "n" || lowered == "y" || lowered == "northing") {
        return VertexColumn::Northing;
    }
    if (lowered == "z" || lowered == "h" || lowered == "height" || lowered == "elevation") {
        return VertexColumn::Height;
    }
    if (lowered == "bulge" || lowered == "b") {
        return VertexColumn::Bulge;
    }
    if (lowered == "bearing" || lowered == "brg") {
        return VertexColumn::Bearing;
    }
    if (lowered == "distance" || lowered == "dist" || lowered == "d") {
        return VertexColumn::Distance;
    }
    return std::nullopt;
}

std::vector<VertexRow> vertexRows(const CurvePolyline2& polyline)
{
    std::vector<VertexRow> rows;
    rows.reserve(polyline.vertices.size());
    for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
        const auto& vertex = polyline.vertices[i];
        VertexRow row;
        row.index = i;
        row.easting = vertex.position.x;
        row.northing = vertex.position.y;
        row.height = vertex.height;
        row.bulge = vertex.bulge;
        if (i < polyline.segmentCount()) {
            const auto& next = polyline.vertices[polyline.segmentEnd(i)].position;
            // The chord's bearing and length, as a traverse table gives an arc:
            // what editing either cell changes (editVertexCell).
            row.bearing = (next - vertex.position).angle();
            row.distance = (next - vertex.position).length();
        }
        rows.push_back(row);
    }
    return rows;
}

std::string cellText(const VertexRow& row, VertexColumn column)
{
    char buffer[64];
    switch (column) {
    case VertexColumn::Index:
        return std::to_string(row.index);
    case VertexColumn::Easting:
        std::snprintf(buffer, sizeof(buffer), "%.3f", row.easting);
        return buffer;
    case VertexColumn::Northing:
        std::snprintf(buffer, sizeof(buffer), "%.3f", row.northing);
        return buffer;
    case VertexColumn::Height:
        if (!row.height) {
            return {};
        }
        std::snprintf(buffer, sizeof(buffer), "%.3f", *row.height);
        return buffer;
    case VertexColumn::Bulge:
        if (row.bulge == 0.0) {
            return "0";
        }
        std::snprintf(buffer, sizeof(buffer), "%.6f", row.bulge);
        return buffer;
    case VertexColumn::Bearing:
        return row.bearing ? formatBearing(*row.bearing) : std::string();
    case VertexColumn::Distance:
        if (!row.distance) {
            return {};
        }
        std::snprintf(buffer, sizeof(buffer), "%.3f", *row.distance);
        return buffer;
    }
    return {};
}

namespace {

katana::core::Error refused(std::string why)
{
    return katana::core::Error{ErrorCode::InvalidArgument, std::move(why), {}};
}

} // namespace

geo::PolylineResult editVertexCell(const CurvePolyline2& polyline, std::size_t index,
                                   VertexColumn column, std::string_view text,
                                   AngleConvention angles)
{
    if (index >= polyline.vertices.size()) {
        return refused("there is no vertex " + std::to_string(index));
    }
    const std::string_view value = katana::core::trimmed(text);
    const auto number = [&]() { return katana::core::parseFiniteDouble(value); };
    switch (column) {
    case VertexColumn::Index:
        return refused("a vertex's index is its place in the polyline and cannot be typed");
    case VertexColumn::Easting:
    case VertexColumn::Northing: {
        const auto coordinate = number();
        if (!coordinate) {
            return refused("'" + std::string(value) + "' is not a coordinate");
        }
        auto moved = polyline.vertices[index].position;
        (column == VertexColumn::Easting ? moved.x : moved.y) = *coordinate;
        return geo::moveVertex(polyline, index, moved);
    }
    case VertexColumn::Height: {
        if (value.empty() || katana::core::equalsIgnoringCase(value, "none")) {
            return geo::setVertexHeight(polyline, index, std::nullopt);
        }
        const auto height = number();
        if (!height) {
            return refused("'" + std::string(value) + "' is not a height; type a number or none");
        }
        return geo::setVertexHeight(polyline, index, *height);
    }
    case VertexColumn::Bulge: {
        const auto bulge = number();
        if (!bulge) {
            return refused("'" + std::string(value) + "' is not a bulge");
        }
        return geo::setSegmentBulge(polyline, index, *bulge);
    }
    case VertexColumn::Bearing:
    case VertexColumn::Distance: {
        if (index >= polyline.segmentCount()) {
            return refused("the last vertex of an open polyline starts no segment");
        }
        const std::size_t next = polyline.segmentEnd(index);
        const auto& from = polyline.vertices[index].position;
        const Vec2 chord = polyline.vertices[next].position - from;
        double direction = chord.angle();
        double length = chord.length();
        if (column == VertexColumn::Bearing) {
            const auto parsed = parseDirection(value, angles);
            if (!parsed) {
                return parsed.error();
            }
            direction = *parsed;
        } else {
            const auto parsed = number();
            if (!parsed || !(*parsed > 0.0)) {
                return refused("'" + std::string(value) + "' is not a distance greater than zero");
            }
            length = *parsed;
        }
        return geo::moveVertex(polyline, next,
                               from + Vec2(std::cos(direction), std::sin(direction)) * length);
    }
    }
    return refused("unknown column");
}

} // namespace katana::cad
