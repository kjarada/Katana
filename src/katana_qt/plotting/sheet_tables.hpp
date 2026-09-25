#pragma once

// The sheet tables as they are drawn (docs/plotting.md, "The drawing register
// and the revision table").
//
// The model lays a table out (cad/plotting/tables.hpp) and the sheet painter
// draws it. What the Qt side adds is the one thing the model cannot know: the
// scale an automatic plan is drawn at, which needs the drawing's extent. The
// register decides those first, as the painter does for a title block, so
// the register and every sheet's title block report the same scale.

#include <vector>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "sheet_painter.hpp"

namespace katana::qt {

// `set` with every automatic plan and key-plan scale and centre decided
// (resolvePlanViewport with the set and the sheet), every automatic section's
// scale and exaggeration fitted (resolveSectionViewport, cutting through
// `cache` when one is given), and the viewports marked automatic no longer:
// the set as the painter draws it.
[[nodiscard]] katana::cad::plotting::SheetSet
resolvedSheetSet(const katana::cad::plotting::SheetSet& set, const SheetSource& source,
                 SheetPaintCache* cache = nullptr);

// The drawing register as it plots: drawingRegister of resolvedSheetSet.
[[nodiscard]] std::vector<katana::cad::plotting::RegisterRow>
drawnRegister(const katana::cad::plotting::SheetSet& set, const SheetSource& source);

} // namespace katana::qt
