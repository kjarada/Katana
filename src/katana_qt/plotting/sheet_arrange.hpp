#pragma once

// The sheet editor's Arrange commands (docs/plotting.md, "Arranging a sheet"):
// the Arrange menu on the toolbar and in the canvas's context menu, and the
// "Choose paper for this scale" button of the sheet's properties.
//
// Each command is a thin front end over include/katana/cad/plotting/
// arrange_commands.hpp: it gathers the ids it acts on and what a view shows,
// makes ONE undoable step, and says what it did - the same step whether a
// person clicked it, a test called it or an agent ran the command-line verb.
// What the editor adds is what only the window knows: the reference layers
// and meshes it shows, and the scale the painter drew an automatic view at.
//
// The ids come from arrangeTargets, the one place the canvas's selection is
// read, so a multiple selection plugs in there and every command follows.

#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plotting/arrange.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "sheet_painter.hpp"

class QMenu;
class QPushButton;
class QToolButton;
class QWidget;

namespace katana::qt {

class SheetEditor;

// Everything a plan with `layers` hidden draws, as world points: the
// drawing's outline (plotting::drawnOutline) and the corners of the shown
// reference layers and meshes - what planDrawnBounds frames, as points, so a
// rotation can be fitted to it. The hull, counter-clockwise.
[[nodiscard]] std::vector<katana::geometry::Point2>
drawingContent(const SheetSource& source,
               const katana::cad::LayerOverrides& layers = katana::cad::LayerOverrides{});

// What a plan or key plan viewport shows, as world points: its stretch of its
// alignment, or the drawing as its hidden layers leave it (drawingContent); a
// key plan adds the sheet outlines it marks. Empty for any other kind, or
// with nothing to show.
[[nodiscard]] std::vector<katana::geometry::Point2>
viewportContent(const katana::cad::plotting::Viewport& viewport, const SheetSource& source);

// The scale a viewport is drawn at: an automatic plan's as the painter
// resolves it, else its own.
[[nodiscard]] double drawnScaleOf(const katana::cad::plotting::Viewport& viewport,
                                  const SheetSource& source);

// The viewports the commands act on: the canvas's selection, or with nothing
// selected every placed viewport on the current sheet, in the sheet's order.
[[nodiscard]] std::vector<std::string> arrangeTargets(const katana::cad::Document& document,
                                                      const SheetEditor& editor);

// The commands. Each is one undoable step on the editor's current sheet (on
// the set, for matchSelectionScale) and returns what it did, in words for the
// log, or why it did nothing.
//
//   arrangeSheet              plotting::autoArrangeSheet
//   alignSelection            the targets; one view alone goes to the tiling
//                             area's edge
//   distributeSelection       the targets when they are three or more, else
//                             every placed view on the sheet
//   matchSelectionScale       the selection (every view drawn to a scale on
//                             the sheet when nothing is selected) takes the
//                             scale `fromId` is drawn at
//   fitSelectionToContent     the selected plan or key plan, else the sheet's
//   rotateSelectionToBestFit  main plan (plotting::mainPlanOf)
//   choosePaperForSheet       the sheet's main plan: the paper that holds its
//                             content at the scale it is drawn at
katana::core::Result<std::string> arrangeSheet(katana::cad::Document& document,
                                               SheetEditor& editor);
katana::core::Result<std::string> alignSelection(katana::cad::Document& document,
                                                 SheetEditor& editor,
                                                 katana::cad::plotting::AlignEdge edge);
katana::core::Result<std::string> distributeSelection(katana::cad::Document& document,
                                                      SheetEditor& editor,
                                                      katana::cad::plotting::DistributeAxis axis);
katana::core::Result<std::string> matchSelectionScale(katana::cad::Document& document,
                                                      SheetEditor& editor,
                                                      const std::string& fromId);
katana::core::Result<std::string> fitSelectionToContent(katana::cad::Document& document,
                                                        SheetEditor& editor);
katana::core::Result<std::string> rotateSelectionToBestFit(katana::cad::Document& document,
                                                           SheetEditor& editor);
katana::core::Result<std::string> choosePaperForSheet(katana::cad::Document& document,
                                                      SheetEditor& editor);

// Fills `menu` with the commands, each reporting through the editor's log and
// status bar. The object names are stable, for tests and agents:
// sheetArrangeAuto, sheetAlignLeft, sheetAlignRight, sheetAlignTop,
// sheetAlignBottom, sheetAlignHCentre, sheetAlignVCentre,
// sheetDistributeHorizontal, sheetDistributeVertical, sheetMatchScaleMenu
// (its items sheetMatchScale_<viewport id>, or sheetMatchScaleNone, filled
// by fillMatchScaleMenu when it opens), sheetFitToContent and sheetRotateToBestFit.
void fillArrangeMenu(QMenu& menu, katana::cad::Document& document, SheetEditor& editor);
// The Match Scale To list: every view drawn to a scale in the set but the
// selected one, by sheet, with the scale it is drawn at.
void fillMatchScaleMenu(QMenu& menu, katana::cad::Document& document, SheetEditor& editor);

// The toolbar's Arrange button (sheetArrangeButton), its menu
// (sheetArrangeMenu) filled as above.
[[nodiscard]] QToolButton* arrangeToolButton(QWidget* parent, katana::cad::Document& document,
                                             SheetEditor& editor);

// The sheet properties' "Choose paper for this scale" button
// (sheetChoosePaper), disabled on a sheet with no plan.
[[nodiscard]] QPushButton* choosePaperButton(QWidget* parent, katana::cad::Document& document,
                                             SheetEditor& editor);

} // namespace katana::qt
