#pragma once

// The Vertices panel's table, headless (docs/drawing.md, "The Vertices
// panel"): one row per vertex of a polyline - its index, easting, northing,
// height and bulge, and the bearing and distance of the segment that starts
// there - and what typing into a cell does. The panel in
// src/katana_qt/drawing/ only shows these rows and hands edits back; the
// VERTEX LIST and VERTEX SET verbs read and write the same columns, so a
// value typed in the panel and one sent by an agent are the same edit.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/drawing/drafting.hpp"
#include "katana/geometry/polyline_vertices.hpp"

namespace katana::cad {

enum class VertexColumn {
    Index,
    Easting,
    Northing,
    Height,
    Bulge,
    Bearing,  // of the segment that starts at the vertex
    Distance, // its chord's length
};

inline constexpr int kVertexColumnCount = 7;

[[nodiscard]] const char* toString(VertexColumn column);
// "e", "easting", "x" -> Easting and so on, case-insensitively: the names
// the VERTEX SET verb takes. nullopt for an unknown name.
[[nodiscard]] std::optional<VertexColumn> vertexColumnFromString(std::string_view name);

struct VertexRow {
    std::size_t index = 0;
    double easting = 0.0;
    double northing = 0.0;
    std::optional<double> height;
    double bulge = 0.0;
    // The segment starting here; nullopt at the last vertex of an open
    // polyline, which starts none.
    std::optional<double> bearing; // radians, counter-clockwise from east (of the chord)
    std::optional<double> distance; // the chord's length
};

[[nodiscard]] std::vector<VertexRow> vertexRows(const katana::geometry::CurvePolyline2& polyline);

// A cell as the panel shows it: coordinates to 3 places, a bearing as a
// whole-circle D°MM'SS", no height as empty.
[[nodiscard]] std::string cellText(const VertexRow& row, VertexColumn column);

// The polyline after `text` is typed into row `index`, column `column`:
//   Easting / Northing   the vertex moves
//   Height               the vertex's height; empty or "none" clears it
//   Bulge                the segment's bulge (0 straight)
//   Bearing              the NEXT vertex moves to keep the segment's length
//                        on the typed bearing (angle convention as given)
//   Distance             the next vertex moves along the chord's bearing to
//                        that chord length (an arc keeps its bulge, so its
//                        shape; the arc length follows)
// InvalidArgument for a column that cannot be edited (Index), a value that
// does not parse, or a bearing or distance at the last vertex of an open
// polyline.
[[nodiscard]] katana::geometry::PolylineResult
editVertexCell(const katana::geometry::CurvePolyline2& polyline, std::size_t index,
               VertexColumn column, std::string_view text,
               AngleConvention angles = AngleConvention::Bearing);

} // namespace katana::cad
