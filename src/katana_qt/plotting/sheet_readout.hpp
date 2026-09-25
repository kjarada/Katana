#pragma once

// What is under the cursor on a sheet (docs/plotting.md, "Editing on the
// canvas"): the paper position in millimetres, the viewport there with its
// kind and scale, and - over a plan or key plan - the point of the drawing
// drawn there, so a sheet can be checked against the survey without leaving
// the editor. The sheet editor shows it in its status bar; an agent asks the
// canvas for it (SheetCanvas::readoutAt).

#include <functional>
#include <optional>
#include <string>

#include <QString>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "sheet_painter.hpp"

namespace katana::qt {

struct SheetCursorReadout {
    // Whether there was a sheet to read, and whether the point is on its paper.
    bool onSheet = false;
    bool onPaper = false;
    // Millimetres from the paper's bottom-left corner, Y up: what the rulers
    // count.
    katana::geometry::Point2 paper{};
    // The topmost placed viewport under the point; empty for none.
    std::string viewportId;
    katana::cad::plotting::ViewportKind kind = katana::cad::plotting::ViewportKind::Plan;
    // The scale that viewport is drawn at ("1:500", "H 1:500 V 1:50"); empty
    // for a kind with no scale.
    std::string scale;
    // Plans and key plans: the world point (easting, northing) drawn there.
    std::optional<katana::geometry::Point2> world;
};

// How a plan viewport is drawn once "auto" is decided: the painter's
// resolvePlanViewport, or a cached answer from it.
using PlanResolver = std::function<ResolvedViewport(const katana::cad::plotting::Viewport&)>;

// The readout at paper point `paper` of `sheet`.
[[nodiscard]] SheetCursorReadout sheetCursorReadout(const katana::cad::plotting::Sheet& sheet,
                                                    const katana::geometry::Point2& paper,
                                                    const PlanResolver& resolve);

// One line for a status bar: "X 220.0  Y 185.0 mm   vp1 Plan 1:500   E 1010.000
// N 2005.000"; empty when there is no sheet.
[[nodiscard]] QString readoutText(const SheetCursorReadout& readout);

} // namespace katana::qt
