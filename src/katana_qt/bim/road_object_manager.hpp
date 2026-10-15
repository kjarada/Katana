#pragma once

// BIM > Road Objects... (docs/road_objects.md, "In the window"): the
// drawing's road objects in one window - listed, edited, copied, deleted,
// zoomed to - beside the dialog that places them (road_object_dialog.hpp).
//
// NOTHING IS CHANGED HERE. Every edit builds the ROADOBJECT line a person
// would type - SET, COPY, DELETE - or the ZOOM WINDOW line that frames the
// chosen objects, and hands it to the window's one executor, so it is echoed,
// kept in the history and undone as one step, exactly as if typed. What the
// table and the details show is read from the drawing and from the verb's
// own INFO, so the window and the command line say the same thing.
//
// BY SCOPE. Beside the table stand the same "Apply to" and "Only those that
// match" controls every dialog on drawing data shows (the shared
// ScopeFilterWidget, customisation/scope_filter_widget.hpp), and three buttons
// that put what they say into ROADOBJECT LIST, QUANTITIES and DELETE
// (docs/road_objects.md, "Scope and filter"): Select Matching runs LIST <scope>
// and chooses the rows of the objects it took, Quantities runs QUANTITIES
// <scope> and shows the answer in the details, Delete Matching runs DELETE
// <scope> - one undo step. A scope that takes nothing is said, not refused.
// The filter's entity-only controls (types, style, text) are greyed: a road
// object has none, and the verb would refuse them by name.
//
// NON-MODAL, kept by the workbench. It hears the Document through a
// DocumentWatcher, so a ROADOBJECT typed on the command line, an undo or an
// open reloads it once, from the event loop, keeping the chosen objects.
//
// Object names, which the widget tests and the headless --dialog, --fill and
// --press steps drive it by:
//   roadObjectManagerDialog   the dialog (the action bimRoadObjects' data)
//   roadObjectTable           an object a row: id, kind, name, placement,
//                             from, to, offset, layer, quantities, problem
//   roadObjectCount           how many objects, and how many problems
//   roadObjectInfo            ROADOBJECT INFO of the current row (read-only)
//   roadObjectKeys            key=value ... for Apply, and for Copy (cleared once run)
//   roadObjectApply           ROADOBJECT SET <id> <keys>
//   roadObjectCopy            ROADOBJECT COPY <id> [the keys]
//   roadObjectDelete          ROADOBJECT DELETE <ids> - the chosen rows
//   roadObjectManagerByScope  the group of the scope controls and their buttons
//   roadObjectManagerScope    the shared scope and filter widget (ScopeFilterWidget
//                             with the prefix roadObjectManager: roadObjectManagerScopeDrawing,
//                             roadObjectManagerScopeLayers, roadObjectManagerFilterProperty ...)
//   roadObjectSelectMatching  ROADOBJECT LIST <scope>: chooses the rows it took
//   roadObjectQuantities      ROADOBJECT QUANTITIES <scope>: shown in the details
//   roadObjectDeleteMatching  ROADOBJECT DELETE <scope>
//   roadObjectZoom            ZOOM WINDOW round the chosen objects
//   roadObjectNew             the placing dialog, on the current row's kind
//   roadObjectManagerStatus   what the last line replied, or why it was refused
//   roadObjectManagerClose

#include <QDialog>
#include <QString>
#include <QStringList>

#include <functional>
#include <memory>
#include <optional>

#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct RoadObjectManagerContext {
    // The drawing. It may be destroyed before the dialog, which then does
    // nothing more (DocumentWatcher).
    katana::cad::Document* document = nullptr;
    // The window's one executor. Empty: the dialog says it cannot run.
    CommandRunner run{};
    // Opens the placing dialog on a kind. Empty: New does nothing.
    std::function<void(katana::cad::road::RoadObjectKind)> place{};
    // The open views, for the scope's "What a view shows". Empty: none.
    std::function<std::vector<ScopeFilterView>()> views{};
};

// The plan box round the solids of the objects `ids` names, widened by a
// tenth each way and to 10 m at least, so a sign is framed with what is
// round it; nullopt when none of them is made.
[[nodiscard]] std::optional<katana::geometry::Box2>
roadObjectsFrame(const katana::cad::Document& document, const QStringList& ids);

// No Q_OBJECT: it connects to lambdas, which keeps moc out of this target.
class RoadObjectManagerDialog final : public QDialog {
  public:
    RoadObjectManagerDialog(RoadObjectManagerContext context, QWidget* parent = nullptr);
    ~RoadObjectManagerDialog() override;

    // Chooses the row of `id` alone; false when there is none.
    bool selectObject(const QString& id);
    // The chosen rows' ids, in table order.
    [[nodiscard]] QStringList selectedIds() const;
    // The table again from the drawing, keeping what was chosen.
    void reload();

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
