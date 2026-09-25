#pragma once

// A plan viewport's coordinate grid in the sheet editor's properties
// (docs/plotting.md, "The coordinate grid and the live key plan"): its style
// and its spacing. A thin front end: what it collects goes to setPlanGrid
// (plan_grid.hpp), which an agent calls directly.

#include <functional>

#include "katana/cad/plotting/sheet_set.hpp"

class QFormLayout;
class QWidget;

namespace katana::qt {

// Adds the grid's two rows to `form`, filled from `viewport`: the style
// (a list, objectName "sheetGridStyle") and the spacing in metres, 0 shown as
// "Auto" (a spin box, "sheetGridInterval"). `automaticIntervalM` is what
// "Auto" comes to at the viewport's scale now, for its tooltip. A change to
// either calls `apply` with both.
void addGridRows(QFormLayout& form, QWidget* parent,
                 const katana::cad::plotting::Viewport& viewport, double automaticIntervalM,
                 std::function<void(katana::cad::plotting::GridStyle, double)> apply);

} // namespace katana::qt
