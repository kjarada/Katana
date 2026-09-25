#pragma once

// Several viewports at once, and the paper geometry a sheet editor works
// with (docs/plotting.md, "Editing on the canvas"). What a hand does on the
// canvas - pick with a rubber band, move a group, snap to the paper grid,
// copy and paste, read the world under the cursor - is decided here, with no
// Qt, so an agent reaches every one of them without a widget and a test
// checks each one without a mouse.
//
//   the paper grid   5 mm lines through the paper's origin, the same origin
//                    the rulers count from; a drag snaps to the edges of the
//                    other viewports first and to the grid only where no
//                    edge was in reach
//   picking          a rubber band selects what it encloses dragged left to
//                    right (a window), what it touches dragged right to left
//                    (crossing), as CAD programs pick
//   the edits        move, remove, copy, paste and duplicate, each ONE
//                    undoable step through Document::setSheetSet, like every
//                    edit in sheet_commands.hpp
//   the world        the point of the drawing under a paper point of a plan:
//                    the sheet painter's mapping, inverted

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::plotting {

// ---- the paper grid ----------------------------------------------------------------

// The grid's spacing: fine enough to place a panel by eye, coarse enough that
// panels placed on it line up without a guide.
inline constexpr double kPaperGridMm = 5.0;

// `value` on the nearest line of a grid of `spacingMm` through the paper's
// origin; unchanged when the spacing is not positive (no grid). A value
// halfway between two lines goes to the higher one.
[[nodiscard]] double snapToGrid(double value, double spacingMm);

// A rectangle being dragged, snapped. First as snapRect snaps it: the
// smallest move within `toleranceMm` that puts one of its edges or its centre
// line on an edge or centre line of the drawing area or of `others`, which
// reports the line as a guide. Then, on an axis where nothing was in reach and
// `gridMm` is positive, the smallest move that puts its left or right (bottom
// or top) edge on a grid line - so a panel of any size lands with an edge on
// the grid, and a panel beside another lines up with it before the grid.
[[nodiscard]] SnapResult snapMovingRect(const Box2& moving, const Box2& drawingArea,
                                        std::span<const Box2> others, double toleranceMm,
                                        double gridMm);

// One edge of a rectangle being resized, snapped the same way: to the nearest
// of `targets` within `toleranceMm` (the target is the guide), else to the
// grid when `gridMm` is positive, else left where it is.
struct EdgeSnap {
    double value = 0.0;
    std::optional<double> guide;
};
[[nodiscard]] EdgeSnap snapEdge(double value, std::span<const double> targets, double toleranceMm,
                                double gridMm);

// ---- picking -----------------------------------------------------------------------

// How a rubber band picks: a Window takes what lies wholly inside it, a
// Crossing everything it touches. An editor makes a band dragged left to
// right a window and one dragged right to left a crossing.
enum class BandMode { Window, Crossing };

// The ids of the placed viewports of `sheet` that `band` picks, in the
// sheet's order (back to front).
[[nodiscard]] std::vector<std::string> viewportsInBand(const Sheet& sheet, const Box2& band,
                                                       BandMode mode);

// The rectangle around the viewports of `sheet` whose ids are in `ids`;
// empty when none of them is placed on it.
[[nodiscard]] Box2 viewportBounds(const Sheet& sheet, std::span<const std::string> ids);

// The placed viewport after `current` in the sheet's order (before it when
// not `forward`), wrapping round; the first (last) when `current` is empty or
// not on the sheet; empty when the sheet has no placed viewport. What Tab
// steps through.
[[nodiscard]] std::string cycleViewport(const Sheet& sheet, std::string_view current,
                                        bool forward);

// ---- a plan's paper and its world --------------------------------------------------

// The world point under paper point `paper` (mm) of a plan or key plan
// `viewport` drawn at 1:`scale` with the world point `centre` at its
// rectangle's centre and turned by its rotation - the sheet painter's mapping
// inverted. `scale` and `centre` are the ones drawn: the viewport's own, or
// what "auto" decided (resolvePlanViewport in the painter).
[[nodiscard]] geometry::Point2 planPaperToWorld(const Viewport& viewport, double scale,
                                                const geometry::Point2& centre,
                                                const geometry::Point2& paper);
// And back: where a world point is drawn on the paper.
[[nodiscard]] geometry::Point2 planWorldToPaper(const Viewport& viewport, double scale,
                                                const geometry::Point2& centre,
                                                const geometry::Point2& world);

// ---- the edits: each ONE undoable step ----------------------------------------------
//
// Each takes the sheet by its position and the viewports by id. An id that is
// not on that sheet is refused (NotFound, naming the id), as is an empty list
// (InvalidArgument); a repeated id counts once. A refused edit changes
// nothing, and an edit that would change nothing adds no step.

// Moves the viewports `ids` of sheet `sheetIndex` by `deltaMm`, as one step.
// A locked viewport stays where it is, as a drag leaves it; an unplaced one
// has nowhere to move from and stays unplaced.
[[nodiscard]] core::Status moveViewports(Document& document, std::size_t sheetIndex,
                                         std::span<const std::string> ids, geometry::Point2 deltaMm,
                                         std::string stepName = "MOVE_VIEWPORTS");

// Removes the viewports `ids` from sheet `sheetIndex`, as one step. Locked
// ones go too: the lock keeps a viewport where it is, not on the sheet.
[[nodiscard]] core::Status removeViewports(Document& document, std::size_t sheetIndex,
                                           std::span<const std::string> ids,
                                           std::string stepName = "REMOVE_VIEWPORTS");

// Copies of the viewports `ids` of sheet `sheetIndex` of `set`, in the
// sheet's order (back to front), ids and all: a clipboard. Pasting gives them
// new ids.
[[nodiscard]] core::Result<std::vector<Viewport>>
copyViewports(const SheetSet& set, std::size_t sheetIndex, std::span<const std::string> ids);

// How far a paste steps from where its copies were.
inline constexpr double kPasteStepMm = 5.0;

// Where a paste of `viewports` onto `sheet` lands, as a move from where they
// were: the first of 0, 1, 2 ... steps of `stepMm` right and down at which no
// pasted rectangle lies exactly on a rectangle already on the sheet. So a
// paste onto the sheet the copies came from lands one step from the
// originals, the next paste one step further, and a paste onto another sheet
// - or after a cut - lands where the copies were.
[[nodiscard]] geometry::Point2 pasteOffset(const Sheet& sheet, std::span<const Viewport> viewports,
                                           double stepMm = kPasteStepMm);

// Adds `viewports` - a clipboard, from this set or another - to the front of
// sheet `sheetIndex`, as one step, with new ids (newViewportIds) in their
// order, moved by `offsetMm` (pasteOffset when not given). Returns the new
// ids in the same order. InvalidArgument when there is nothing to paste.
[[nodiscard]] core::Result<std::vector<std::string>>
pasteViewports(Document& document, std::size_t sheetIndex, std::vector<Viewport> viewports,
               std::optional<geometry::Point2> offsetMm = std::nullopt,
               std::string stepName = "PASTE_VIEWPORTS");

// Copies of the viewports `ids` of sheet `sheetIndex` pasted onto the same
// sheet (one step from the originals, pasteOffset), as one step; returns the
// copies' ids in the sheet's order of their originals.
[[nodiscard]] core::Result<std::vector<std::string>>
duplicateViewports(Document& document, std::size_t sheetIndex, std::span<const std::string> ids,
                   std::string stepName = "DUPLICATE_VIEWPORTS");

} // namespace katana::cad::plotting
