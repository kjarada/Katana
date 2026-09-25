#pragma once

// Arranging a sheet, and advice on its paper, scale and rotation
// (docs/plotting.md, "Arranging a sheet"). Pure functions on sheets and world
// points: the editor's Arrange menu and the command line call the same ones.
//
//   bestFitRotation      the turn that shows the content at the largest scale
//                        in a rectangle: rotating calipers over its hull
//   leastRotationToFit   the smallest turn at which it fits at a given scale;
//   bestStandardRotation   and the smallest turn that reaches the largest
//                        standard scale - a drawing is turned only when that
//                        buys a larger scale, and no further than it must be
//   suggestPaper         the smallest ISO sheet that holds it at a scale
//   suggestScale         the largest standard scale it fits at
//   fitPaperToViewport   a sheet put on the paper its main view needs
//   autoArrange          no two views overlapping, the main view first
//   alignViewports, distributeViewports, matchScale, fitViewportToContent,
//   rotateToBestFit      the editor's Arrange commands, on ids
//
// "Content" is a set of world points: the drawing's outline (drawnOutline), a
// stretch of an alignment, a key plan's outlines (viewportContent gathers
// what a view shows). Only its convex hull matters, so a caller may pass
// every vertex or just the hull.
//
// These change a Sheet or SheetSet value; arrange_commands.hpp makes each of
// them ONE undoable step on a document.
//
// Rotations follow Viewport::rotation: radians counter-clockwise, the world
// direction that runs left to right across the paper. A rotation returned
// here is in (-90, 90] degrees, since a drawing turned half a turn needs the
// same room upside down.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::plotting {

// A best rotation within this of a multiple of 90 degrees is snapped to it: a
// drawing turned 0.4 degrees gains nothing a reader would notice, and costs a
// north arrow that is not quite up.
inline constexpr double kRotationSnapDegrees = 1.0;

// How much room a viewport's own fit leaves round its content: the painter's
// automatic scale keeps 4% to spare (sheet_painter.cpp), so a rectangle sized
// to 5% more than the content is chosen the same scale by it.
inline constexpr double kFitSpare = 1.05;
// The room the painter's automatic scale leaves: what rotateToBestFit fits
// inside, so the scale it writes is the one the painter will choose.
inline constexpr double kAutoScaleSpare = 1.04;

// The content turned to `rotation` in a rectangle: how big it is and the
// scale at which it just fills the rectangle.
struct RotationFit {
    double rotation = 0.0; // radians, as Viewport::rotation
    // The world point at the rectangle's centre: the middle of the content
    // as turned, so it sits centred on the paper.
    Point2 centre{};
    // The content's size along the rotation (across the paper) and square
    // to it (up the paper), metres.
    double width = 0.0;
    double height = 0.0;
    // 1 : scale fills the rectangle exactly; standardScale is the first of
    // kSheetScales at or above it (sheetScaleAtLeast).
    double scale = 0.0;
    double standardScale = 0.0;

    friend bool operator==(const RotationFit&, const RotationFit&) = default;
};

// The content at `rotation` in a rectangle of `rectangle` millimetres.
// InvalidArgument for a rectangle with no size, a point that is not finite,
// or content with no size (fewer than two distinct points).
[[nodiscard]] core::Result<RotationFit> fitAtRotation(std::span<const Point2> content,
                                                      SizeMm rectangle, double rotation);

// The rotation at which the content fills a rectangle of `rectangle`'s
// aspect at the largest scale. Found exactly: over the hull's rotating
// calipers the needed scale is, between two orientations where an edge lies
// flat, the larger of two concave curves, so it is least at such an
// orientation or where the width and the height ask the same scale - both
// kinds are tried. Of rotations that tie, the one nearest 0 wins (the
// positive one of two equally near); a result within kRotationSnapDegrees of
// a multiple of 90 degrees is snapped to it, at the scale that costs.
// A line fits best along the rectangle's diagonal. Errors as fitAtRotation.
[[nodiscard]] core::Result<RotationFit> bestFitRotation(std::span<const Point2> content,
                                                        SizeMm rectangle);

// The rotation nearest 0 at which the content fits the rectangle at 1 :
// `scale` or larger: 0 when it already does. Snapped to a multiple of 90
// degrees within kRotationSnapDegrees when that still fits. InvalidArgument
// as fitAtRotation, for a scale that is not positive, and when no rotation
// fits (the message says the scale the best rotation needs).
[[nodiscard]] core::Result<RotationFit> leastRotationToFit(std::span<const Point2> content,
                                                           SizeMm rectangle, double scale);

// The rotation nearest 0 that reaches the largest STANDARD scale any rotation
// reaches: the content is turned only when turning buys a larger scale off
// kSheetScales, and then no further than that scale needs. What "Rotate to
// Best Fit" and the fitted generator use.
[[nodiscard]] core::Result<RotationFit> bestStandardRotation(std::span<const Point2> content,
                                                             SizeMm rectangle);

// The convex hull (counter-clockwise) of everything `view` draws of `model`:
// each drawn entity by its vertices - arcs and circles by a polygon round
// them with corners no more than 1/32 of a turn apart, so the hull holds the
// whole curve; texts and dimensions by their boxes - and every alignment, as
// planDrawnBounds counts it. What a plan of the drawing shows.
[[nodiscard]] std::vector<Point2> drawnOutline(const entity::Model& model,
                                               const LayerOverrides& view);

// What a plan or key plan viewport shows of `model`, as the convex hull of
// world points: its stretch of the alignment its source names (the whole
// alignment when the chainage range is empty), else the drawing as its
// hidden layers leave it (drawnOutline); a key plan adds the outlines it
// marks. Empty for any other kind, and when there is nothing to show. The
// sheet editor adds the window's reference layers and meshes to this.
[[nodiscard]] std::vector<Point2> viewportContent(const entity::Model& model,
                                                  const Viewport& viewport);

// ---- Paper and scale ------------------------------------------------------------------

struct PaperRequest {
    double scale = 500.0;   // 1 : scale
    double rotation = 0.0;  // the content turned as a viewport turns it
    // The frame a landscape sheet is given; empty for none. A portrait sheet
    // has no frame (frame.hpp) and its drawing area is the paper less 10 mm.
    std::string frame{kBuiltInFrameId};
    // How much of the tiling area's width and height the content may use: 1
    // for a view that fills the sheet; a main cell's share when panels sit
    // beside it (0.64 of the width for "Main and two panels right").
    double shareAcross = 1.0;
    double shareUp = 1.0;

    friend bool operator==(const PaperRequest&, const PaperRequest&) = default;
};

struct PaperAdvice {
    PaperSize paper = PaperSize::A3;
    bool landscape = true;
    std::string frame; // what the sheet's frame becomes: empty on portrait
    Box2 area;         // that sheet's tiling area
    // How much of the room the content uses: the larger of its width's and
    // its height's share, in (0, 1].
    double fill = 0.0;

    friend bool operator==(const PaperAdvice&, const PaperAdvice&) = default;
};

// The smallest ISO sheet, A4 to A0 and landscape before portrait at each
// size, whose tiling area holds the content at the request's scale and
// rotation. InvalidArgument for a scale or share that is not positive and
// errors as fitAtRotation; NotFound when not even A0 holds it, saying the
// scale at which A0 would.
[[nodiscard]] core::Result<PaperAdvice> suggestPaper(std::span<const Point2> content,
                                                     const PaperRequest& request);
// The same for a world extent, unrotated.
[[nodiscard]] core::Result<PaperAdvice> suggestPaper(const Box2& extent,
                                                     const PaperRequest& request);

// The largest standard scale (the first of kSheetScales at or above what it
// needs) at which the content, turned to `rotation`, fits `rectangle`; beyond
// 1 : 50 000 the scale it needs, as sheetScaleAtLeast gives. Errors as
// fitAtRotation.
[[nodiscard]] core::Result<double> suggestScale(std::span<const Point2> content,
                                                const Box2& rectangle, double rotation);
// The same for the whole of a sheet's tiling area.
[[nodiscard]] core::Result<double> suggestScale(std::span<const Point2> content,
                                                const Sheet& sheet, double rotation);

// Puts the sheet on the advised paper: its size, orientation and frame. Every
// placed viewport keeps its place in proportion - its rectangle mapped from
// the old tiling area onto the new one - so a view that filled the sheet
// fills the new one. Returns the ids of the viewports that moved, in order.
std::vector<std::string> applyPaper(Sheet& sheet, const PaperAdvice& advice);

struct PaperChange {
    PaperAdvice advice;
    std::vector<std::string> moved; // as applyPaper returns them

    friend bool operator==(const PaperChange&, const PaperChange&) = default;
};

// "Choose paper for this scale": puts `sheet` on the smallest paper that
// holds what its viewport `viewportId` shows (`content`) at 1 : `scale`,
// turned as the view turns it, with the painter's kAutoScaleSpare round it,
// in the share of the tiling area the view's rectangle takes now
// (suggestPaper, then applyPaper). The frame asked for is the sheet's own on
// landscape paper; a portrait sheet asks for the built-in frame, which a
// landscape suggestion then gets. The view is given `scale` and centred on
// the content, its automatic scale and centring turned off, so the new paper
// does not choose another. NotFound for an id not on the sheet;
// InvalidArgument for a view that is not a plan or key plan or is not
// placed, or a scale that is not positive; errors as suggestPaper.
[[nodiscard]] core::Result<PaperChange> fitPaperToViewport(Sheet& sheet,
                                                           std::string_view viewportId,
                                                           std::span<const Point2> content,
                                                           double scale);

// ---- Arranging viewports --------------------------------------------------------------

struct ArrangeResult {
    // The viewports whose rectangle changed, in the sheet's order.
    std::vector<std::string> moved;
    // Those still overlapping another, and those still unplaced: no room for
    // them even at their kind's minimum size. Both empty when the sheet was
    // arranged completely.
    std::vector<std::string> overlapping;
    std::vector<std::string> unplaced;
    // Whether the main view had to be made smaller to make room.
    bool mainShrunk = false;

    friend bool operator==(const ArrangeResult&, const ArrangeResult&) = default;
};

// Removes the overlaps between the sheet's unlocked viewports, inside the
// tiling area:
//   - locked viewports stay where they are, and others keep clear of them;
//   - the main view - the lowest tiling rank, ties in the sheet's order -
//     keeps its place and size;
//   - every other view that overlaps nothing, lies inside the tiling area
//     (one that strayed outside is brought in first) and is no larger than
//     the main view stays where it is;
//   - the rest, in rank order, are packed into the free space one gutter
//     from everything, each at the highest, then left-most, place it fits;
//     when they do not all fit they shrink together, in 5% steps, never
//     below their kind's minimum; when that is not enough every view but the
//     main one is packed again; and only then is the main view made smaller,
//     from its top-left corner, in 10% steps. A view that gives way is never
//     made larger than the main view.
// A main view that overlaps a locked one, or is not placed, is packed first,
// with the rest, and none of them is made larger than it is packed. An
// unplaced viewport is packed with the size of the cell tiling would give it.
// A view with no room left even at its minimum stays where it was and is
// reported; the views placed after it keep clear of it.
// Deterministic: the same sheet is always arranged the same way, and a sheet
// arranged completely does not change when it is arranged again.
ArrangeResult autoArrange(Sheet& sheet, double gutterMm = kTilingGutterMm);

enum class AlignEdge {
    Left,
    Right,
    Top,
    Bottom,
    HorizontalCentre, // centres on one vertical line
    VerticalCentre,   // centres on one horizontal line
};

// "left", "right", "top", "bottom", "hcentre", "vcentre".
[[nodiscard]] std::string_view toString(AlignEdge edge);
[[nodiscard]] std::optional<AlignEdge> alignEdgeFrom(std::string_view name);

// Lines up the viewports `ids` of `sheet` on the outermost of their edges
// (the leftmost left edge, the highest top...) or on the middle of the box
// round them. One viewport alone is aligned to the tiling area. A locked
// viewport counts where it is and does not move; an unplaced one is left
// out. Returns the ids that moved, in the sheet's order. NotFound for an id
// not on the sheet; InvalidArgument when none of them is placed.
[[nodiscard]] core::Result<std::vector<std::string>>
alignViewports(Sheet& sheet, std::span<const std::string> ids, AlignEdge edge);

enum class DistributeAxis { Horizontal, Vertical };

// "horizontal", "vertical".
[[nodiscard]] std::string_view toString(DistributeAxis axis);
[[nodiscard]] std::optional<DistributeAxis> distributeAxisFrom(std::string_view name);

// Spaces the viewports `ids` so the gaps between them are equal: ordered by
// their left (bottom) edges, the first and last stay and the others move
// between them. Locked and unplaced viewports are left out. Returns the ids
// that moved. NotFound for an id not on the sheet; InvalidArgument for fewer
// than three that can move.
[[nodiscard]] core::Result<std::vector<std::string>>
distributeViewports(Sheet& sheet, std::span<const std::string> ids, DistributeAxis axis);

// Whether a viewport of this kind is drawn to a scale: a plan, a key plan, a
// long section or cross sections.
[[nodiscard]] bool hasScale(ViewportKind kind);

// Gives the viewports `ids`, on any sheet of `set`, the scale of `fromId` -
// `fromScale` when given, which is how a caller passes the scale an automatic
// viewport is drawn at - and turns their automatic scale off. A section
// matched to a section takes its vertical exaggeration too. Viewports with no
// scale (hasScale) are left out. Returns the ids that changed. NotFound for
// an unknown id; InvalidArgument when `fromId` has no scale or the scale is
// not positive.
[[nodiscard]] core::Result<std::vector<std::string>>
matchScale(SheetSet& set, std::span<const std::string> ids, std::string_view fromId,
           std::optional<double> fromScale = std::nullopt);

// Sizes a viewport to its content at its scale: the rectangle grows or
// shrinks about its centre until the content, with kFitSpare round it, just
// fits, and the view is centred on the content. Never smaller than the kind's
// minimum; kept inside `drawingArea`, which may leave it smaller than the
// content. A plan or key plan measures the content turned to its rotation; a
// section measures (chainage or offset, level) points, the level exaggerated.
// The scale is kept and the centre set, so automatic scale and centring are
// turned off: the caller gives an automatic view the scale it is drawn at
// first. InvalidArgument for another kind, an unplaced viewport or content
// with no size.
[[nodiscard]] core::Status fitViewportToContent(Viewport& viewport,
                                                std::span<const Point2> content,
                                                const Box2& drawingArea);

// Turns a plan or key plan to the rotation that shows its content at the
// largest standard scale inside its rectangle (bestStandardRotation, leaving
// the painter's kAutoScaleSpare), centres it on the content and gives it that
// scale, turning automatic scale and centring off: the painter's automatic
// fit measures the drawing's box, which a turned drawing overfills. The
// rectangle does not change. Returns the fit. InvalidArgument for another
// kind or an unplaced viewport, and errors as fitAtRotation.
[[nodiscard]] core::Result<RotationFit> rotateToBestFit(Viewport& viewport,
                                                        std::span<const Point2> content);

// smartLayout for a plan of an area, with the drawing turned to fill the
// sheet. `content` is what the plan shows (drawnOutline).
//   - An automatic scale: the plan is turned when that buys a larger
//     standard scale (bestStandardRotation), and left square otherwise.
//   - A fixed scale that needs more than one sheet square - tiles, or the
//     3D view and legend on a sheet of their own - is tried turned on one
//     sheet, beside its panels (leastRotationToFit); the tiles stay when it
//     does not fit.
// InvalidArgument for a request with no plan area or with an alignment's
// plan or sections (a strip already follows its alignment); and errors as
// smartLayout. Content with no size is not turned.
[[nodiscard]] core::Result<std::vector<Sheet>>
smartLayoutRotated(const entity::Model& model, const LayoutRequest& request,
                   std::span<const Point2> content);

} // namespace katana::cad::plotting
