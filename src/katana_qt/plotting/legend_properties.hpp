#pragma once

// The Legend viewport's rows in the sheet editor's properties (docs/plotting.md,
// "The smart legend"): what it lists, and what that comes to now.
//
// A thin front end: the choice is cad::plotting::setLegendScope, one undoable
// step, and the summary is the legend the painter prints (gatherLegend).

#include <cstddef>
#include <functional>

#include <QString>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "sheet_painter.hpp"

class QFormLayout;

namespace katana::qt {

// "12 entries from this sheet's 2 plans"; "3 entries from 5 plans of the
// set - this sheet has no plan"; "1 entry from the whole drawing". Says
// where a fall-back led, since a legend listing the whole set when it was
// asked for this sheet is otherwise a puzzle.
[[nodiscard]] QString legendSummary(const katana::cad::plotting::Legend& legend,
                                    katana::cad::plotting::LegendScope asked);

// Adds to `form` a choice of scope (objectName "sheetLegendScope") and a line
// saying what the legend lists (objectName "sheetLegendSummary"). A change of
// scope is committed at once; `report` hears of a refusal.
void addLegendProperties(QFormLayout& form, katana::cad::Document& document,
                         const katana::cad::plotting::SheetSet& set, std::size_t sheetIndex,
                         const katana::cad::plotting::Viewport& legend, const SheetSource& source,
                         const std::function<void(const QString&, bool)>& report);

} // namespace katana::qt
