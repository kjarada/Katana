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

#include "katana/entity/annotation.hpp"
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
    // False for the kinds that have no straight dimension line - an
    // angular dimension's is its arc, in `curves` - so a consumer does not
    // draw a zero-length one.
    bool hasDimensionLine = true;
    // Polylines: an angular dimension's arc, chorded to the drawing's own
    // accuracy, and a radial or ordinate dimension's leader.
    std::vector<std::vector<Point2>> curves;
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

// Builds the drawing for `dimension` under `style`, for every kind
// (entity.hpp, DimensionKind, says what each measures and where its line
// goes).
//
// `scale` is the annotation scale the drawing is for, 1 : scale. It matters
// only to a paper-sized style (DimensionStyle::paperSized), whose sizes are
// millimetres on paper and are drawn at size x scale / 1000 model units; a
// model-unit style draws the same at every scale. The default is the
// default annotation scale, which is what the spatial index and a caller
// with no view use.
//
// A degenerate dimension - the two measured points coincident, so there is no
// direction to offset along - produces an empty drawing rather than a division
// by zero or an arbitrary orientation. The model rejects such a dimension on
// the way in; this is the renderer refusing to guess if one ever arrives.
[[nodiscard]] DimensionDrawing buildDimension(const katana::entity::DimensionGeometry& dimension,
                                              const katana::entity::DimensionStyle& style,
                                              double scale = katana::entity::kDefaultAnnotationScale);

// `style` with its sizes in model units at 1 : `scale`: itself for a
// model-unit style, its paper millimetres x scale / 1000 for a paper-sized one.
[[nodiscard]] katana::entity::DimensionStyle
dimensionStyleAtScale(const katana::entity::DimensionStyle& style, double scale);

// The text a dimension shows: an override verbatim, else the measured value
// formatted by the style - lengths by formatMeasurement, an angle in degrees,
// minutes and seconds, a radius with "R" and a diameter with the diameter
// sign before it when the style has no prefix of its own.
[[nodiscard]] std::string dimensionText(const katana::entity::DimensionGeometry& dimension,
                                        const katana::entity::DimensionStyle& style);

// The style a dimension resolves to: its layer's named style, else the
// document default. Follows the same ByLayer shape as
// katana::entity::resolveDisplay, and returns the default rather than null so
// that a renderer always has something to draw with.
[[nodiscard]] const katana::entity::DimensionStyle&
resolveDimensionStyle(const katana::entity::Model& model, const katana::entity::Entity& entity);

} // namespace katana::cad
