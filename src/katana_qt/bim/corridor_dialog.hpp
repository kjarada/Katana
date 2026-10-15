#pragma once

// BIM > Corridors... (docs/road_design.md, "In the window"): the drawing's
// corridors - a road template swept along an alignment and its design
// profile - their regions, their superelevation, the surface their batters
// meet, and what they build: the 3D strings and the finished and subgrade
// surfaces.
//
// NOTHING IS CHANGED HERE. Every button builds the CORRIDOR line a person
// would type and hands it to the window's one executor (command_runner.hpp),
// so it is echoed, kept in the history and one undo step. What it shows is
// Document::corridors and the road model's problems (bim::bimModel), so the
// dialog and CORRIDOR INFO / PROBLEMS agree. Non-modal, kept by the window,
// reloaded through a DocumentWatcher.
//
// THE FIELDS ARE A BUFFER, applied by Apply. Choosing a corridor fills them
// from it; a reload (after any line, an undo, a redo, a new or opened
// drawing) refills them only when the chosen corridor itself changed, so a
// typed interval survives an unrelated command and a changed drawing is
// shown as it is. The combos show the name the corridor holds - found
// ignoring case, as the verbs find it, or, for a surface that was removed or
// an alignment that lost its profile, as an extra item - so Apply cannot
// re-point a corridor to whatever item happened to be showing.
//
// Object names:
//   corridorDialog          the dialog (the action bimCorridors' data)
//   corridorList            Corridor, Alignment, Template, Surface, Interval, From, To,
//                           Regions, Superelevation
//   corridorName            the name New and Rename use
//   corridorAlignment       the alignments with a design profile
//   corridorTemplate        the road templates
//   corridorSurface         (none) and the drawing's surfaces
//   corridorInterval, corridorFrom, corridorTo   metres; From and To may be empty
//   corridorNew             CORRIDOR NEW "name" ALIGNMENT a TEMPLATE t [surface=] interval=
//                           [from=] [to=]
//   corridorApply           CORRIDOR SET <chosen> alignment= template= surface= interval=
//                           from= to=
//   corridorRename          CORRIDOR RENAME <chosen> "name"
//   corridorDelete          CORRIDOR DELETE <chosen>
//   corridorRegions         From, To, Template
//   corridorRegionFrom, corridorRegionTo, corridorRegionTemplate
//   corridorRegionAdd       CORRIDOR REGION <c> ADD from to template
//   corridorRegionRemove    CORRIDOR REGION <c> REMOVE <the chosen row's from>
//   corridorSuperelevation  Station, Left %, Right %
//   corridorSuperStation, corridorSuperLeft, corridorSuperRight
//   corridorSuperAdd        CORRIDOR SUPERELEVATION <c> ADD station left right
//   corridorSuperRemove     CORRIDOR SUPERELEVATION <c> REMOVE <the chosen row's station>
//   corridorBuild           CORRIDOR BUILD <c>: sections, objects, cut and fill
//   corridorStrings         CORRIDOR STRINGS <c>: the 3D strings, as polylines
//   corridorSurfaces        CORRIDOR SURFACE <c>: the finished and subgrade surfaces
//   corridorProblems        what the chosen corridor could not build, a line each
//   corridorStatus          what the last line replied, or why it was refused
//   corridorClose

#include <QDialog>

#include <memory>

#include "command_runner.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct CorridorDialogContext {
    katana::cad::Document* document = nullptr;
    CommandRunner run{};
};

// No Q_OBJECT: it connects to lambdas.
class CorridorDialog final : public QDialog {
  public:
    CorridorDialog(CorridorDialogContext context, QWidget* parent = nullptr);
    ~CorridorDialog() override;

    // Chooses the corridor `name`; false when there is none.
    bool selectCorridor(const QString& name);
    [[nodiscard]] QString currentCorridor() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
