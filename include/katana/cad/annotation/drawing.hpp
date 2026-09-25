#pragma once

// What an annotation draws, as plain model-space primitives
// (docs/annotation.md, "Drawing").
//
// Text in its styles, labels, leaders and callouts are laid out HERE, in
// katana_cad, into this value; a painter only strokes, fills and sets the
// pieces. So the layout - where a centred line of a readable, rotated,
// masked, three-line note goes; where a label ended up after the placer; how
// big a leader's arrow is at 1 : 500 - is decided once, tested headlessly,
// and the same on the screen, on a plot and on every sheet viewport, the way
// dimension_draw.hpp already made the dimensions one definition.
//
// Everything is in MODEL units, worked out for one annotation scale: the
// paper sizes have already been multiplied by scale / 1000.

#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::annotation {

using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Vec2;

// The face a run of text is set in: what a painter makes its font from and
// what a TextMeasure measures.
struct TextFace {
    std::string family{}; // empty: the painter's plain-text face
    bool bold = false;
    bool italic = false;
    friend bool operator==(const TextFace&, const TextFace&) = default;
};

// One line of text, placed.
struct TextRun {
    std::string text;
    // The left end of the line's baseline, before the oblique slant.
    Point2 origin{};
    // The text height in model units: the em size a painter sets the font at,
    // as the plain text of a TextGeometry always was.
    double height = 0.0;
    double rotation = 0.0; // radians, counter-clockwise
    // Horizontal stretch of the face (1: as designed) and the slant of the
    // verticals (radians, positive leaning right).
    double widthFactor = 1.0;
    double oblique = 0.0;
    TextFace face{};
};

// How wide `line` is when set in `face` at height 1, before the width
// factor: the painters measure with their fonts, the tests and the entity
// layer estimate (kApproximateGlyphAspect per character). A layout is only as
// exact as its measure; with the painter's, a centred line is centred on the
// sheet to the font's own advance widths.
using TextMeasure = std::function<double(std::string_view line, const TextFace& face)>;

// The estimate: 0.6 per character, whatever the face.
[[nodiscard]] TextMeasure estimatedMeasure();

struct Drawing {
    // Filled with the medium's background before anything else is drawn:
    // text masks, and the inside of a callout's frame.
    std::vector<std::vector<Point2>> masks;
    // Open polylines in the annotation's pen: leader lines, ticks, markers.
    std::vector<std::vector<Point2>> strokes;
    // Closed outlines in the pen: callout boxes and circles (chorded).
    std::vector<std::vector<Point2>> outlines;
    // Filled with the pen's colour: closed arrowheads, dots.
    std::vector<std::vector<Point2>> fills;
    std::vector<TextRun> texts;
    // A colour the annotation's style gives it, overriding the entity's
    // resolved (ByLayer) pen colour; empty keeps the entity's.
    std::optional<katana::entity::Color> colour{};
    // Everything above.
    Box2 extent{};

    [[nodiscard]] bool empty() const
    {
        return masks.empty() && strokes.empty() && outlines.empty() && fills.empty() &&
               texts.empty();
    }
    // Recomputes `extent` from the pieces; text by its measured block, which
    // the layout records as a mask-sized outline of its own (textBoxes).
    void updateExtent();
    // Appends another drawing's pieces.
    void append(const Drawing& other);

    // The rotated rectangle each laid-out text block occupies (before any
    // mask margin): what the label placer tests for overlap and what
    // updateExtent counts for text. Four corners, counter-clockwise.
    std::vector<std::vector<Point2>> textBoxes;
};

// A circle as a closed polyline of `segments` chords.
[[nodiscard]] std::vector<Point2> circlePolygon(const Point2& centre, double radius,
                                                int segments = 48);

} // namespace katana::cad::annotation
