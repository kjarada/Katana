#pragma once

// Terrain > Alignment Manager... (docs/cad.md, "The Alignment Manager"):
// the drawing's alignments, their PIs, their design profiles and their
// setting-out tables in one window, where before there was only the ALIGN
// verb and a message telling a person to type it.
//
// NOTHING IS CHANGED HERE. Every edit builds the ALIGN (or LABEL ALIGN) line a
// person would type and hands it to the window's one executor
// (command_runner.hpp), so it is echoed in the command log, kept in the
// history and undone as one step, exactly as if typed - and an agent does the
// same through the verb. What the dialog shows is read from the model and from
// the functions ALIGN STATIONS and ALIGN PROFILE print from
// (katana/cad/alignment_report.hpp), so the tables and the verbs agree.
//
// NON-MODAL, kept by the window between uses. It hears the Document through a
// DocumentWatcher, so an ALIGN typed on the command line, an undo or an import
// reloads it once, from the event loop, keeping the chosen alignment - and a
// grid's unapplied edits, while what the grid was filled from is unchanged.
//
// The two grids - PIs and PVIs - are buffers, as the Linetypes tab's pattern
// grid is: edited in place, applied by one line, one undo step, and never a
// line run from a table's own signal. They show the stored numbers exactly
// (shortest round trip), so applying an edit to one PI's easting writes every
// other number back as it was.
//
// Object names, which the widget tests and the headless --dialog, --fill and
// --press steps drive it by:
//   alignmentManagerDialog   the dialog (the action terrainAlignmentManager's data)
//   alignmentList            the alignments: name, PIs, start, end, length, profile
//   alignmentName            the name for New
//   alignmentPoints          the PIs for New, "x,y x,y ..."
//   alignmentUseSelection    fill alignmentPoints from the selected line or polyline
//   alignmentNew             ALIGN NEW "name" x,y x,y ...
//   alignmentDelete          ALIGN DELETE name
//   alignmentTabs            Horizontal, Vertical, Setting Out
//   Horizontal:
//     alignmentPiTable       #, easting, northing, radius, spiral in, spiral out:
//                            edited in place, then applied
//     alignmentAddPi, alignmentRemovePi      a row more or less in the grid
//     alignmentRevertPis     the grid as the drawing holds it again
//     alignmentApplyPis      ALIGN PIS name x,y[,radius[,spiralIn[,spiralOut]]] ...
//     alignmentStartStation, alignmentStart   ALIGN START name station
//   Vertical:
//     alignmentPviTable      chainage, level, curve length: edited, then applied
//     alignmentAddPvi, alignmentRemovePvi   a row more or less in the grid
//     alignmentRevertProfile the grid as the drawing holds it again
//     alignmentApplyProfile  ALIGN DESIGN name s,z[,L] ... from the grid
//     alignmentClearProfile  ALIGN CLEARPROFILE name
//     alignmentProfileElements  element, from, to, grade in, grade out, K (read only)
//     alignmentHighLow       the high and low points
//     alignmentProfilePreview   the profile drawn: grades, curves, high and low points
//   Setting Out:
//     alignmentInterval      the chainage interval (20 by default), applied as typed
//     alignmentStationsNote  how many stations and key stations, or why there are none
//     alignmentStations      chainage, easting, northing, azimuth, radius, key
//     alignmentStationsCopy  the table to the clipboard, tab separated
//     alignmentStationsCsv   Save CSV... (no file dialog in a headless run)
//     alignmentLabelStyle, alignmentLabelLayer, alignmentLabelChainages
//                            LABEL ALIGN "name" style="..." [layer=...]
//   alignmentStatus          what the last line replied, or why it was refused
//   alignmentClose

#include <QDialog>

#include <functional>
#include <memory>

#include "command_runner.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct AlignmentManagerContext {
    // The drawing. It may be destroyed before the dialog, which then does
    // nothing more (DocumentWatcher).
    katana::cad::Document* document = nullptr;
    // The window's one executor. May be empty (a dialog built without a
    // window): the dialog then says it cannot run the line.
    CommandRunner run{};
    // True in a headless session, which opens no file dialog. May be empty.
    std::function<bool()> headless{};
};

// No Q_OBJECT: it connects to lambdas, which keeps moc out of this target.
class AlignmentManagerDialog final : public QDialog {
  public:
    AlignmentManagerDialog(AlignmentManagerContext context, QWidget* parent = nullptr);
    ~AlignmentManagerDialog() override;

    // Chooses the alignment `name` in the list; false when there is none.
    bool selectAlignment(const QString& name);
    // The chosen alignment's name; empty when there is none.
    [[nodiscard]] QString currentAlignment() const;
    // What Save CSV writes, to `path`, without asking where: the setting-out
    // table of the chosen alignment at the interval shown.
    [[nodiscard]] katana::core::Status saveStationsCsv(const QString& path) const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
