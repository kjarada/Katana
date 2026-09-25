#pragma once

// Laying viewports out on a sheet: the tiling presets and snapping
// (docs/plotting.md). Pure functions on paper rectangles.
//
// The eight presets are the owner's app's, cell for cell, with its 3 mm
// gutter and its 1 mm inset from the drawing area. What differs is the
// order viewports take the cells in. The app sorted by a table that had no
// entry for its map and locality panels, so its comparator returned NaN and a
// map could land in the big cell or anywhere; here EVERY kind has a rank, and
// the main drawing - a plan, else a long section, else cross sections - takes
// the first and largest cell.

#include <cstddef>
#include <optional>
#include <span>
#include <string_view>
#include <vector>

#include "katana/cad/plotting/sheet_set.hpp"

namespace katana::cad::plotting {

enum class TilingPreset {
    Full,          // one cell
    Columns2,      // two side by side
    Rows2,         // two, one above the other
    Quad,          // four quarters
    MainRight,     // main 66% wide, a panel to its right
    MainBelow,     // main 64% high, a panel below it
    MainCorner,    // main 66% high across the top, a panel in the bottom-right corner
    MainTwoRight,  // main 64% wide, two panels stacked to its right
};

inline constexpr std::size_t kTilingPresetCount = 8;
inline constexpr double kTilingGutterMm = 3.0;
inline constexpr double kTilingInsetMm = 1.0;

// The ids the owner's app gives them: full, cols2, rows2, quad, sectionR,
// sectionB, sectionBR, sectionMap3d. Stored in files, so never renamed.
[[nodiscard]] std::string_view presetId(TilingPreset preset);
[[nodiscard]] std::optional<TilingPreset> presetFromId(std::string_view id);
// A general name for a menu: "Full frame", "Main and panel below" ...
[[nodiscard]] std::string_view presetName(TilingPreset preset);

// Which cell a viewport of this kind takes: lower first. Plan 0, LongSection
// 1, CrossSections 2, Model3D 3, KeyPlan 4, Image 5, Legend 6, Notes 7,
// SheetIndex 8, Revisions 9.
[[nodiscard]] int tilingRank(ViewportKind kind);
// Notes are placed by hand and tiling leaves them where they are, as the
// owner's app does; every other kind tiles.
[[nodiscard]] bool isTileable(ViewportKind kind);

struct SizeMm {
    double width = 0.0;
    double height = 0.0;
};
// The smallest a tiled cell may make a viewport of this kind (the app's).
[[nodiscard]] SizeMm minimumSize(ViewportKind kind);

// The preset's cells inside `area`, first cell first: each the cell's share
// of the area less half the gutter on every side. `area` is where tiling
// happens - the drawing area already inset (tilingArea).
[[nodiscard]] std::vector<Box2> presetCells(TilingPreset preset, const Box2& area,
                                            double gutterMm = kTilingGutterMm);

// The drawing area inset by kTilingInsetMm: what the presets divide.
[[nodiscard]] Box2 tilingArea(const Sheet& sheet);

// Lays the sheet's tileable, unlocked viewports into the preset's cells in
// rank order (ties keep their order on the sheet), never smaller than the
// kind's minimum. Viewports beyond the cell count stay where they were.
// Returns how many were placed.
std::size_t tileViewports(Sheet& sheet, TilingPreset preset, double gutterMm = kTilingGutterMm);

// A moving rectangle snapped to the drawing area and to other viewports:
// the smallest move, within `toleranceMm` on each axis, that puts one of its
// edges or its centre line on an edge or centre line of a target. `guideX`
// and `guideY` are the lines it snapped to, for the editor to draw.
struct SnapResult {
    Box2 rect;
    std::optional<double> guideX;
    std::optional<double> guideY;
};
[[nodiscard]] SnapResult snapRect(const Box2& moving, const Box2& drawingArea,
                                  std::span<const Box2> others, double toleranceMm);

} // namespace katana::cad::plotting
