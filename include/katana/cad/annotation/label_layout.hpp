#pragma once

// Labels, laid out for one scale with collision avoidance
// (docs/annotation.md, "The placer").
//
// Every label entity says WHAT to label and HOW (its style); where its text
// goes depends on the scale it is looked at - at 1 : 200 two point numbers
// are metres apart on the sheet, at 1 : 2000 they would print on top of each
// other - so placement is worked out per view, here, from the model and the
// scale, and never stored.
//
// The placer is a deterministic greedy candidate placer, the classical
// cartographic method (Imhof's positions for points, positions slid along
// lines, the centroid first for areas):
//
//   1. Each piece of each label (labelPieces: a point, a segment, an arc, an
//      area, a chainage mark) gets an ordered list of candidate positions -
//      its style's preferred place first, then the others the style allows,
//      then the same further out (DISPLACED, with a leader back to what it
//      labels when the style asks for one).
//   2. Pieces are placed in a fixed order: a dragged label first, then by
//      style priority (higher first), then by label id and piece index. No
//      container's iteration order and no pointer takes part (Rule 7).
//   3. Each piece takes its first candidate whose text box overlaps no text
//      placed before it and crosses no linework it was asked to keep out of.
//      A piece with no free candidate is SUPPRESSED and counted; a dragged
//      label is always placed where it was put.
//
// Overlap is exact for the rotated text boxes (separating axes), found
// through a uniform grid over their bounding boxes, so a thousand labels cost
// what their neighbours cost rather than a million pair tests.
//
// Headless, and tested headlessly with the estimated measure; the painters
// pass their font's.

#include <cstddef>
#include <vector>

#include "katana/cad/annotation/drawing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/label_values.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::annotation {

struct LabelLayoutOptions {
    // The annotation scale, 1 : scale.
    double scale = katana::entity::kDefaultAnnotationScale;
    TextMeasure measure = estimatedMeasure();
    // Only pieces whose anchor lies in this box (grown by the reach of a
    // label) are laid out; empty lays out everything. A view passes what it
    // shows, so zoomed in it places what it can see.
    Box2 visible{};
    // Linework the text keeps out of: the painter passes the lines of the
    // entities in view. Empty keeps text out of other text only.
    std::vector<katana::geometry::Segment2> linework{};
    // The model direction of the sheet's horizontal, radians: minus a
    // viewport's turn (PlanFrame::rotation), so a point label turned with its
    // viewport still reads level on the sheet.
    double horizontal = 0.0;
    // No collision avoidance at all: every piece at its first candidate.
    // What a plot of a drawing whose labels were arranged by hand wants, and
    // what the tests compare the placer against.
    bool avoidCollisions = true;
};

// One piece of a label, laid out.
struct PlacedLabel {
    katana::entity::EntityId label = 0;
    std::size_t piece = 0;
    // The text and whatever goes with it: a leader when displaced, a point's
    // marker, a chainage tick.
    Drawing drawing;
    // Which candidate it took (0: the style's own place) and whether that
    // was one of the displaced ring.
    std::size_t candidate = 0;
    bool displaced = false;
};

// A piece of a label, by the label's id and the piece's index.
struct LabelPieceRef {
    katana::entity::EntityId label = 0;
    std::size_t piece = 0;
    friend bool operator==(const LabelPieceRef&, const LabelPieceRef&) = default;
};

struct LabelLayout {
    std::vector<PlacedLabel> placed;
    // Marks drawn whatever the text does - chainage ticks, spot-level
    // markers - of pieces whose text was suppressed, kept so the mark still
    // shows where the label would have gone.
    std::vector<PlacedLabel> marksOnly;
    std::size_t considered = 0; // pieces with text to place
    std::size_t suppressed = 0; // of those, no room
    std::size_t displaced = 0;  // of those, placed away from their first place
    std::size_t orphaned = 0;   // labels with no target, style or text
    // The suppressed pieces themselves, in the placing order: what LABEL
    // LAYOUT names, so a person or an agent can select them and give them
    // room (Annotate > Label Layout Report's Select Suppressed).
    std::vector<LabelPieceRef> suppressedPieces;
};

// Lays out `labels` (label entities; anything else is ignored) for the
// model as it is.
[[nodiscard]] LabelLayout layoutLabels(const katana::entity::Model& model,
                                       const std::vector<const katana::entity::Entity*>& labels,
                                       const LabelLayoutOptions& options);

// Every label entity in the model, in id order: what a caller with no view
// (the command line's LABEL LAYOUT, a test) lays out.
[[nodiscard]] std::vector<const katana::entity::Entity*>
labelEntities(const katana::entity::Model& model);

// A label's pieces (entity::labelPieces) with their `code` value found where
// survey coding finds a code (codePropertyCandidates): what every caller in
// this layer asks for, so a label's {code} is the code a mapfile rule reads.
[[nodiscard]] std::vector<katana::entity::LabelPiece>
labelPiecesOf(const katana::entity::Model& model, const katana::entity::LabelGeometry& label,
              const katana::entity::LabelStyle& style);

// The survey code `entity` carries, where survey coding looks for one (the
// first of codePropertyCandidates it has); empty when none.
[[nodiscard]] std::string surveyCode(const katana::entity::Entity& entity);

// The text a label's piece shows: its override, or its style's template
// filled from the piece's values. Empty when the template's every line
// needed a value the piece does not have.
[[nodiscard]] std::string labelText(const katana::entity::LabelGeometry& label,
                                    const katana::entity::LabelStyle& style,
                                    const katana::entity::LabelPiece& piece);

// The words a whole label shows now: each piece's labelText, the empty ones
// and chainage ticks left out, joined by " | ". Empty when its style is gone
// or it says nothing. What LABEL LIST replies as text= and the window's
// Properties show, so the two cannot disagree.
[[nodiscard]] std::string shownLabelText(const katana::entity::Model& model,
                                         const katana::entity::LabelGeometry& label);

// Whether two convex polygons overlap (touching edges count as apart): the
// separating axis test the placer uses, exposed for its tests.
[[nodiscard]] bool convexOverlap(const std::vector<Point2>& a, const std::vector<Point2>& b);

// What a note, leader or dimension puts in a label's way, as segments
// appended to `out` (at most up to `limit` in all): its strokes and outlines,
// and each text box's four edges and two diagonals - the diagonals so that
// a label wholly inside a note's box still meets it. The painter adds these
// to LabelLayoutOptions::linework for the annotation in view, so a label
// keeps clear of a dimension's figures and a callout's box as well as of the
// lines; a dimension's text box is its estimated extent (the placer is a
// judgement of where there is room, not a typesetter).
void appendKeepOut(const Drawing& drawing, std::vector<katana::geometry::Segment2>& out,
                   std::size_t limit);
void appendKeepOut(const DimensionDrawing& dimension, std::vector<katana::geometry::Segment2>& out,
                   std::size_t limit);

// The straight pieces of a line, polyline, arc or circle (arcs chorded at
// sixteen a turn: the placer asks only whether a text box crosses the
// curve, and at a label's size that is the curve), appended to `out` up to
// `limit`. Nothing for any other kind.
void appendLinework(const katana::entity::Geometry& geometry,
                    std::vector<katana::geometry::Segment2>& out, std::size_t limit);

// Everything a label keeps out of in the whole drawing at 1 : `scale`: the
// linework and the notes', leaders' and dimensions' keep-out of every entity
// the document draws (isDrawn, no view's layer overrides), labels aside, up
// to `limit` segments. What a caller with no view (LABEL LAYOUT)
// passes as LabelLayoutOptions::linework, so the command line places labels
// where the plan view does; the painter gathers the same for what it has in
// view as it draws.
[[nodiscard]] std::vector<katana::geometry::Segment2>
labelKeepOut(const katana::entity::Model& model, double scale, const TextMeasure& measure,
             std::size_t limit);

} // namespace katana::cad::annotation
