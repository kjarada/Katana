#pragma once

// Smart sheet generators (docs/plotting.md): each turns a simple request into
// sheets, deterministically - the same request gives the same sheets, byte for
// byte, so a generated set can be tested and regenerated without surprises.
//
//   fitToSheet          an extent on one sheet at the largest scale that fits
//   gridSheets          an area in tiles at a chosen scale, with overlap,
//                       match lines and a key plan
//   stripSheets         an alignment in strips rotated to its bearing, with
//                       match lines at chainages
//   crossSectionSheets  cross sections in rows and columns at one scale and
//                       exaggeration, in chainage order, onto as many sheets
//                       as it takes
//   sheetsFromPlotFrames  one sheet per imported plot frame
//   smartLayout         "show me this" - composes the above with the presets
//
// None of them touches a Document: they return sheets, and the caller adds
// them in one undoable step (sheet_commands.hpp).

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/section.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/alignment.hpp"

namespace katana::cad::plotting {

// The paper every generated sheet is on. A3 landscape with the built-in frame
// by default, as the frame itself is.
struct SheetTemplate {
    PaperSize paper = PaperSize::A3;
    bool landscape = true;
    std::string frame{kBuiltInFrameId};
};

// A blank sheet of `paper` named `name`.
[[nodiscard]] Sheet blankSheet(const SheetTemplate& paper, std::string name);

// One sheet with one plan viewport filling the tiling area, centred on
// `extent`, at the largest standard scale (kSheetScales) at which all of it
// fits. InvalidArgument for an empty extent (a single point has nothing to
// scale to; a line along an axis does, and fits).
[[nodiscard]] core::Result<std::vector<Sheet>> fitToSheet(const Box2& extent,
                                                          const SheetTemplate& paper = {});

struct GridRequest {
    Box2 area;             // world
    double scale = 500.0;  // 1 : scale
    double overlapM = 0.0; // how much neighbouring tiles share, in metres
    bool keyPlan = true;   // a first sheet showing every tile, numbered
    SheetTemplate paper;
};

// Tiles covering `area` at the scale, centred on it, numbered in reading
// order (rows from the top, left to right). Each tile's plan carries a match
// line in the middle of each overlap it shares, labelled with the sheet
// across it. InvalidArgument for an empty area, a non-positive scale, or an
// overlap as large as a tile.
[[nodiscard]] core::Result<std::vector<Sheet>> gridSheets(const GridRequest& request);

struct StripRequest {
    double scale = 500.0;
    double overlapM = 0.0;
    // The chainage range to cover; the whole alignment when not given.
    std::optional<double> fromChainage;
    std::optional<double> toChainage;
    bool keyPlan = false;
    SheetTemplate paper;
    // The paper rectangle each strip's plan fills: the tiling area when not
    // given; the top cell of "Main and panel below" when a long section goes
    // under it (smartLayout).
    std::optional<Box2> planRect;
};

// Strips along `alignment` (named `name` in the viewports' source), each a
// plan rotated so the alignment runs left to right, advancing by the tile's
// width less the overlap - shortened where a curve would leave the strip -
// with match lines across the alignment at each change of sheet.
[[nodiscard]] core::Result<std::vector<Sheet>>
stripSheets(const geometry::SolvedAlignment& alignment, const std::string& name,
            const StripRequest& request);

struct CrossSectionRequest {
    double interval = 20.0;       // every so many metres of chainage, ends included
    std::vector<double> stations; // or exactly these chainages
    double halfWidth = 20.0;      // each section runs this far either side
    std::size_t rows = 4;
    std::size_t columns = 2;
    double scale = 0.0;        // 0: the largest standard scale the widest fits
    double exaggeration = 0.0; // 0: the largest of 1, 2, 2.5, 4, 5, 8, 10 that
                               // fits the deepest section (1 without surfaces)
    // Surfaces to sample, to centre each section on its ground and choose the
    // exaggeration; without any, each section is centred when it is drawn.
    std::vector<SectionSurfaceInput> surfaces;
    SheetTemplate paper;
};

// Cross sections of `alignment` at the requested chainages, rows x columns
// per sheet, in chainage order down each column and then across, all at one
// scale and exaggeration, onto as many sheets as they need.
[[nodiscard]] core::Result<std::vector<Sheet>>
crossSectionSheets(const geometry::SolvedAlignment& alignment, const std::string& name,
                   const CrossSectionRequest& request);

// One sheet per imported plot frame in `model` - a closed polyline carrying
// plot_frame.width, .height, .scale and the four margins - showing exactly
// the ground inside the frame's margins, on the ISO paper its size is (within
// 2 mm), with the frame's own layer hidden in the viewport. Where the paper
// lies and which way it is turned come from the outline itself (its first
// corner and first edge), so a frame imported with an origin shift, or moved
// or rotated since, is plotted where it is drawn; the file's .xorigin,
// .yorigin and .rotation are not used. Frames that cannot be read - no ISO
// size, no scale, an outline that is no longer four corners - are skipped and
// described in `skipped`.
[[nodiscard]] core::Result<std::vector<Sheet>>
sheetsFromPlotFrames(const entity::Model& model, const SheetTemplate& paper = {},
                     std::vector<std::string>* skipped = nullptr);

// What to show; smartLayout decides how.
struct LayoutRequest {
    std::optional<Box2> planArea;  // a plan of this area
    std::string alignment;         // an alignment in the model, for the three below
    bool planAlongAlignment = false; // a plan following the alignment
    bool longSection = false;
    double crossSectionInterval = 0.0; // cross sections every so many metres; 0: none
    double crossSectionHalfWidth = 20.0;
    bool model3d = false;
    bool legend = false;
    double scale = 0.0; // 0: auto
    SheetTemplate paper;
};

// Composes sheets for `request` using the presets:
//   - a plan along an alignment with its long section: plan-and-profile
//     sheets, the plan above (MainBelow) and the long section of the same
//     chainages below at the same horizontal scale;
//   - a plan of an area: one sheet at the fitted scale ("auto"), or at a
//     fixed scale, tiles when the area does not fit one sheet at it;
//   - the 3D snapshot and legend beside the plan (MainRight, MainTwoRight)
//     when a plan of an area or one strip along the alignment is the only
//     plan and, made for that narrower cell, still shows all of it - an
//     automatic scale is fitted to the cell, a fixed one must hold it -
//     otherwise on a sheet of their own after the plan;
//   - cross sections on sheets of their own after the rest.
// "auto" fits everything on as few sheets as it can: an area or an alignment
// on one sheet. NotFound for an alignment the model does not have;
// InvalidArgument for a request that shows nothing.
[[nodiscard]] core::Result<std::vector<Sheet>> smartLayout(const entity::Model& model,
                                                           const LayoutRequest& request);

// Gives every sheet and viewport of `sheets` a new id (newSheetIds,
// newViewportIds - never one `set` uses or one of its marks still names), and
// moves the key plans' and match lines' references among `sheets` to their
// new ids. Generators number from one; this is how their output joins a set
// that already has sheets.
void prepareForAppend(const SheetSet& set, std::vector<Sheet>& sheets);

} // namespace katana::cad::plotting
