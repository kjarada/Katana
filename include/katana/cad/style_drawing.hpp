#pragma once

// A 12d linestyle or symbol definition -> plain geometry (PLAN.MD 20.3,
// slice 2).
//
// `entity::LineStyle` says what a definition IS; this says where its strokes
// land. Everything comes out as polylines and texts in MODEL coordinates, so
// the viewport, the plotter and a preview all paint the same thing from one
// definition - which is why this is here and not in the viewport, exactly as
// `cad::symbolStrokes` already is. Which definition a NAME means is
// cad/style_resolver.hpp's question, not this file's.
//
// Two ways to place a definition, and the definition itself says which:
//
//   * `atVertices` (12d's `mode vertex`) makes it a SYMBOL: the strokes are
//     put at a point, scaled and rotated. `symbolDrawing`.
//   * otherwise it is a LINESTYLE: the strokes repeat along a polyline, with
//     the definition's +x running along the line and its +y to the left.
//     `linestyleDrawing`, or `layLinestyle` for a caller that can see only
//     part of the line.
//
// A definition's coordinates mean different things - see entity::StyleUnits -
// so both take `paperScale`, the model units one plot millimetre covers. A
// `worldstyle` ignores it; a `paperstyle` multiplies by it, which is what
// keeps a 1.5 mm tick 1.5 mm on the page at any zoom.
//
// Every placement starts from a FlatDefinition: the strokes flattened once
// into runs. A caller drawing thousands of points with one symbol flattens it
// once (style_resolver.hpp's DefinitionCache) instead of once per point per
// frame; the LineStyle overloads below flatten on every call.

#include <cstddef>
#include <optional>
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

// A definition's strokes flattened into runs of connected points, in its own
// coordinates with `factor` and the origin applied. It OWNS everything it
// holds - the texts are copied, not pointed at - so it outlives the library it
// was flattened from: a cache may keep one across a setStyleLibrary without
// anything in it dangling.
struct FlatDefinition {
    struct Run {
        std::vector<katana::geometry::Point2> points{};
        bool closed = false;
        std::string pen{};
    };
    struct Text {
        katana::geometry::Point2 at{};
        katana::entity::StrokeText text{};
        std::string pen{};
    };

    katana::entity::StyleUnits units = katana::entity::StyleUnits::World;
    bool atVertices = false;
    // The file's `length`, as LineStyle::length; 0 when it said nothing.
    double length = 0.0;
    // LineStyle::bounds(): what a symbol `size` is a width OF.
    katana::geometry::Box2 bounds{};
    // TwoPoint only: the anchors in the same frame as the runs.
    katana::geometry::Point2 anchor1{};
    katana::geometry::Point2 anchor2{};
    std::vector<Run> runs{};
    std::vector<Text> texts{};

    // How far the PEN travelled along the definition's x, moves included.
    //
    // This is the period of the pattern when the file gives no `length`, and
    // it is not the same as the span of what was drawn: a linestyle ends its
    // period with a bare `move`, and that move IS the gap.
    // "move 0 0 / draw 3 0 / move 5 0" is a three-unit dash and a two-unit
    // gap. Measuring only the drawn part gave a period of 3, so every dash
    // butted against the next and the whole linestyle came out as a solid
    // line - which is exactly what it looked like.
    double lowestX = 0.0;
    double highestX = 0.0;
    bool anyPoint = false;
    // The furthest any mark sits across the line (|y|), a text's height times
    // its length included: how far a pattern can reach into a view from a line
    // that is itself just outside it.
    double reachAcross = 0.0;

    [[nodiscard]] double naturalPeriod() const { return anyPoint ? highestX - lowestX : 0.0; }
};

[[nodiscard]] FlatDefinition flattenDefinition(const katana::entity::LineStyle& style);

// A definition placed at a point. `size` is the width the definition is
// scaled to - a 12d symbol `size`, which is a WIDTH the way Style::symbolSize
// is - or 0 to use the definition at its own scale. `rotation` is radians
// counter-clockwise.
[[nodiscard]] StyleDrawing symbolDrawing(const FlatDefinition& flat,
                                         const katana::geometry::Point2& at, double size = 0.0,
                                         double rotation = 0.0, double paperScale = 1.0);
[[nodiscard]] StyleDrawing symbolDrawing(const katana::entity::LineStyle& style,
                                         const katana::geometry::Point2& at, double size = 0.0,
                                         double rotation = 0.0, double paperScale = 1.0);

// How a linestyle is laid when the caller can see only part of the line and
// has a frame to paint.
struct LinestyleOptions {
    double paperScale = 1.0;
    // Pixels per model unit, for the level of detail. 0 lays the pattern at
    // any size, which is what a test or a caller with no screen wants.
    double viewScale = 0.0;
    // A pattern whose period is shorter than this on screen is not laid: the
    // caller draws the plain line. Below about two pixels the repeats merge
    // into a smudge the colour of the line, so the plain line is both what the
    // eye sees anyway and thousands of times cheaper.
    double minimumPeriodPixels = 2.0;
    // The part of the drawing the caller can see. When given, only the
    // repeats that can reach it are laid - the box is grown by one period and
    // by the pattern's reach across the line - and each is laid exactly where
    // it falls on the WHOLE line, so the pattern keeps its phase from the
    // first vertex and does not crawl as the view pans.
    std::optional<katana::geometry::Box2> visible{};
    // More repeats than this and NOTHING is laid (Outcome::OverBudget): the
    // caller draws the plain line. The all-or-nothing contract is forEachDash's
    // (dashing.hpp), for the same reason: a truncated pattern is a wrong
    // drawing with no error anywhere (audit CAD-04).
    std::size_t maximumInstances = 20000;
};

struct LinestyleLayout {
    enum class Outcome {
        // The drawing IS the line, gaps and all; the caller draws nothing else.
        // It can be empty when none of the line is in view.
        Laid,
        // Nothing to lay a pattern along (a degenerate line): plain line.
        NothingToLay,
        // Under LinestyleOptions::minimumPeriodPixels: plain line.
        TooFine,
        // Over LinestyleOptions::maximumInstances: plain line.
        OverBudget,
    };
    StyleDrawing drawing{};
    Outcome outcome = Outcome::NothingToLay;
    // How many repeats were laid; for Laid only.
    std::size_t instances = 0;
};

[[nodiscard]] LinestyleLayout layLinestyle(const FlatDefinition& flat,
                                           const katana::geometry::Polyline2& line,
                                           const LinestyleOptions& options);

// A definition repeated along the whole of a line. The pattern's period is the
// definition's `length`, or the span of its own strokes when it says nothing.
//
// Each point is placed by ITS OWN distance along the line rather than by
// rigidly transforming a whole instance, so the pattern bends with the line
// instead of leaving it at a corner. On a straight run the two are identical.
//
// Empty - never truncated - when the line would need more repeats than
// LinestyleOptions' default budget; layLinestyle says why.
[[nodiscard]] StyleDrawing linestyleDrawing(const katana::entity::LineStyle& style,
                                            const katana::geometry::Polyline2& line,
                                            double paperScale = 1.0);

// A definition drawn between two points - what a `twoptstyle` is for. The
// definition's two anchors are mapped onto `from` and `to`, so the strokes
// stretch to fill the span.
[[nodiscard]] StyleDrawing twoPointDrawing(const FlatDefinition& flat,
                                           const katana::geometry::Point2& from,
                                           const katana::geometry::Point2& to,
                                           double paperScale = 1.0);
[[nodiscard]] StyleDrawing twoPointDrawing(const katana::entity::LineStyle& style,
                                           const katana::geometry::Point2& from,
                                           const katana::geometry::Point2& to,
                                           double paperScale = 1.0);

// Whichever of the three the definition asks for: a symbol at EVERY vertex, a
// two-point style across the whole line, or a linestyle along it. This is what
// a caller drawing an entity wants, so that the definition decides rather than
// the caller guessing. The outcome is layLinestyle's for a linestyle and Laid
// (or NothingToLay for an empty line) for the other two.
[[nodiscard]] LinestyleLayout styleDrawing(const FlatDefinition& flat,
                                           const katana::geometry::Polyline2& line,
                                           const LinestyleOptions& options);
[[nodiscard]] StyleDrawing styleDrawing(const katana::entity::LineStyle& style,
                                        const katana::geometry::Polyline2& line,
                                        double paperScale = 1.0);

} // namespace katana::cad
