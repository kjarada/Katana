#pragma once

// A 12d linestyle or symbol definition -> plain geometry (PLAN.MD 20.3,
// slice 2).
//
// `entity::LineStyle` says what a definition IS; this says where its strokes
// land. Everything comes out as polylines and texts in MODEL coordinates, so
// the viewport, the plotter and a preview all paint the same thing from one
// definition - which is why this is here and not in the viewport, exactly as
// `cad::symbolStrokes` already is.
//
// Two ways to place a definition, and the definition itself says which:
//
//   * `atVertices` (12d's `mode vertex`) makes it a SYMBOL: the strokes are
//     put at a point, scaled and rotated. `symbolDrawing`.
//   * otherwise it is a LINESTYLE: the strokes repeat along a polyline, with
//     the definition's +x running along the line and its +y to the left.
//     `linestyleDrawing`.
//
// A definition's coordinates mean different things - see entity::StyleUnits -
// so both take `paperScale`, the model units one plot millimetre covers. A
// `worldstyle` ignores it; a `paperstyle` multiplies by it, which is what
// keeps a 1.5 mm tick 1.5 mm on the page at any zoom.

#include <string>
#include <vector>

#include "katana/entity/style_library.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

struct StyleStroke {
    katana::geometry::Polyline2 path{};
    // The 12d colour name in force, from a `colour` command. EMPTY means the
    // entity's own colour, which is what 12d's "view_colour" asks for.
    std::string pen{};
};

struct StyleTextMark {
    katana::geometry::Point2 at{};
    std::string text{};
    double height = 0.0;  // model units
    double angle = 0.0;   // radians counter-clockwise, the line's direction included
    std::string justify{}; // 12d's spelling: "middle-centre", "top-left"
    std::string font{};
    double widthFactor = 1.0;
    std::string pen{};
};

struct StyleDrawing {
    std::vector<StyleStroke> strokes{};
    std::vector<StyleTextMark> texts{};

    [[nodiscard]] bool empty() const { return strokes.empty() && texts.empty(); }
    // Every point of every stroke, plus each text's anchor.
    [[nodiscard]] katana::geometry::Box2 bounds() const;
};

// A definition placed at a point. `size` is the width the definition is
// scaled to - a 12d symbol `size`, which is a WIDTH the way Style::symbolSize
// is - or 0 to use the definition at its own scale. `rotation` is radians
// counter-clockwise.
[[nodiscard]] StyleDrawing symbolDrawing(const katana::entity::LineStyle& style,
                                         const katana::geometry::Point2& at, double size = 0.0,
                                         double rotation = 0.0, double paperScale = 1.0);

// A definition repeated along a line. The pattern's period is the
// definition's `length`, or the span of its own strokes when it says nothing.
//
// Each point is placed by ITS OWN distance along the line rather than by
// rigidly transforming a whole instance, so the pattern bends with the line
// instead of leaving it at a corner. On a straight run the two are identical.
[[nodiscard]] StyleDrawing linestyleDrawing(const katana::entity::LineStyle& style,
                                            const katana::geometry::Polyline2& line,
                                            double paperScale = 1.0);

// A definition drawn between two points - what a `twoptstyle` is for. The
// definition's two anchors are mapped onto `from` and `to`, so the strokes
// stretch to fill the span.
[[nodiscard]] StyleDrawing twoPointDrawing(const katana::entity::LineStyle& style,
                                           const katana::geometry::Point2& from,
                                           const katana::geometry::Point2& to,
                                           double paperScale = 1.0);

// Whichever of the three the definition asks for: a symbol at the first
// vertex, a two-point style across the whole line, or a linestyle along it.
// This is what a caller drawing an entity wants, so that the definition
// decides rather than the caller guessing.
[[nodiscard]] StyleDrawing styleDrawing(const katana::entity::LineStyle& style,
                                        const katana::geometry::Polyline2& line,
                                        double paperScale = 1.0);

} // namespace katana::cad
