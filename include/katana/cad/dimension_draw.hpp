#pragma once

// Turning a dimension and its style into plain geometry (PLAN.MD Phase 09).
//
// One definition, used by the 2D viewport, the 3D scene builder and the extent
// calculation, so a dimension cannot look different in two views or be culled
// at a size it is not drawn at.
//
// EVERYTHING HERE IS IN MODEL UNITS. The viewport previously drew a 5-pixel
// tick and a 12-pixel label, which look right on screen and plot at whatever
// size the paper happens to give them - a dimension is part of the drawing, so
// its text has to keep its size relative to the geometry it annotates.
//
// The extent this returns covers the text, the arrows and the extension
// overshoot. entity::boundingBox for a Dimension covers only the measured
// points and the dimension line, which is smaller than what is drawn, so using
// it to cull culls a dimension whose label is still on screen.

#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;

struct DimensionDrawing {
    // The two extension lines, from just off each measured point to just past
    // the dimension line.
    std::vector<Segment2> extensionLines;
    Segment2 dimensionLine;
    // Arrow strokes, for the tick and open heads.
    std::vector<Segment2> arrowStrokes;
    // Closed arrow outlines, one polygon per end, for the filled and dot heads.
    // A renderer that cannot fill draws them as closed polylines; the shape is
    // still correct, only hollow.
    std::vector<std::vector<Point2>> arrowFills;

    std::string text;
    Point2 textAnchor;     // left end of the text baseline
    double textHeight = 0.0;
    double textRotation = 0.0; // radians, counter-clockwise

    // Everything above, including the text. What a viewport should cull
    // against.
    Box2 extent;

    [[nodiscard]] bool empty() const { return text.empty() && extensionLines.empty(); }
};

// Builds the drawing for `dimension` under `style`.
//
// A degenerate dimension - the two measured points coincident, so there is no
// direction to offset along - produces an empty drawing rather than a division
// by zero or an arbitrary orientation. The model rejects such a dimension on
// the way in; this is the renderer refusing to guess if one ever arrives.
[[nodiscard]] DimensionDrawing buildDimension(const katana::entity::DimensionGeometry& dimension,
                                              const katana::entity::DimensionStyle& style);

// The style a dimension resolves to: its layer's named style, else the
// document default. Follows the same ByLayer shape as
// katana::entity::resolveDisplay, and returns the default rather than null so
// that a renderer always has something to draw with.
[[nodiscard]] const katana::entity::DimensionStyle&
resolveDimensionStyle(const katana::entity::Model& model, const katana::entity::Entity& entity);

} // namespace katana::cad
