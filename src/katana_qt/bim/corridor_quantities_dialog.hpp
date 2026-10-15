#pragma once

// BIM > Corridor Quantities... (docs/road_design.md, "Quantities"): the road
// model's quantities - each course's volume and area, each kerb's length,
// the earthworks cut and fill - for one corridor or all, with totals by kind
// and material, copied or saved as CSV.
//
// It runs CORRIDOR QUANTITIES <corridor>|ALL through the window's executor
// and shows the records it replies, so the table and the verb cannot
// disagree; Save CSV runs the same line with csv=<file>. A headless run
// opens no file dialog: the path is typed into its field.
//
// THE LISTING IS CLEARED when the drawing changes under it: an edit, an undo,
// a redo, a new or opened drawing that leaves the road model it was listed
// from (bim::bimModel, what the verb reads) no longer the drawing's. The
// status then says to press List Quantities again, and Copy hands out nothing.
// It is not listed again by itself, which would put a CORRIDOR QUANTITIES line
// in the command log after every edit; a change that left the road model as it
// was (a line drawn) leaves the listing as it is.
//
// Object names:
//   corridorQuantitiesDialog    the dialog (the action bimCorridorQuantities' data)
//   corridorQuantitiesCorridor  All, then each corridor
//   corridorQuantitiesRun       CORRIDOR QUANTITIES <corridor>|ALL
//   corridorQuantitiesTable     Object, Kind, Length, Width, Depth, Height, Area, Volume
//   corridorQuantitiesTotals    the cut, fill and net, and the totals by kind and material
//   corridorQuantitiesCsvPath   the file Save CSV writes
//   corridorQuantitiesBrowse    a file dialog for it (none in a headless run)
//   corridorQuantitiesSaveCsv   CORRIDOR QUANTITIES <corridor>|ALL csv="<file>" [OVERWRITE]
//   corridorQuantitiesOverwrite replace a file already there (the line gets OVERWRITE)
//   corridorQuantitiesCopy      the table to the clipboard, tab separated
//   corridorQuantitiesStatus    what the last line replied, or why it was refused
//   corridorQuantitiesClose

#include <QDialog>

#include <functional>
#include <memory>

#include "command_runner.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct CorridorQuantitiesContext {
    katana::cad::Document* document = nullptr;
    CommandRunner run{};
    // True in a headless session, which opens no file dialog. May be empty.
    std::function<bool()> headless{};
};

class CorridorQuantitiesDialog final : public QDialog {
  public:
    CorridorQuantitiesDialog(CorridorQuantitiesContext context, QWidget* parent = nullptr);
    ~CorridorQuantitiesDialog() override;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
