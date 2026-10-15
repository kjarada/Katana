#pragma once

// The BIM menu's road model (docs/road_design.md, "In the window"): road
// templates, corridors, their quantities, and the road model shown in the
// views. The owner's request of 2026-10-15: horizontal BIM - roads, not
// buildings.
//
// MainWindow makes the BIM menu and toolbar (the objects work adds its
// items to them through its own file), hands them over with RoadServices and
// keeps this object alive; everything it needs from the window arrives
// through those callbacks, so it never includes main_window.hpp.
//
// The menu, in sections:
//   Templates   Road Template Manager...          (bimRoadTemplates)
//   Corridors   Corridors...                      (bimCorridors)
//   Model       Show Road Model                   (bimShowRoadModel, checkable)
//   Quantities  Corridor Quantities...            (bimCorridorQuantities, Ctrl+Shift+Q)
// Corridors... and Corridor Quantities... are shown in Terrain's Corridors
// section too - the same QAction objects, as File shows the imports of the
// other menus - where Corridor Quantities... and Corridor Surface... were
// (docs/road_design.md, "Where the old corridor commands went"). In a
// drawing with no corridor and no alignment that has a design profile they
// open nothing and say where one is made; once it has a corridor they always
// open. Each
// action's data names its dialog (roadTemplateManagerDialog, corridorDialog,
// corridorQuantitiesDialog), which is how --dialog finds it.
//
// THE ROAD MODEL IN THE VIEWS. Each 3D view draws bim::bimModel's objects as
// meshes in their colours (bim::sceneMeshes, put in the window's list of
// meshes through setRoadMeshes), and each plan view, plot and sheet draws
// their plan outlines (plan_painter.hpp, PlanSource::roadModel). Show Road
// Model takes them out of both; an object also follows its layer
// (cad/bim/bim_view.hpp). The meshes are made again, from the event loop,
// when the road model, the surfaces, the alignments or the layers change.

#include <functional>
#include <memory>
#include <vector>

#include <QKeySequence>
#include <QPointer>
#include <QString>

#include "command_runner.hpp"
#include "icons.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scene.hpp"

class QAction;
class QDialog;
class QMainWindow;
class QMenu;
class QObject;
class QToolBar;

namespace katana::cad::bim {
struct BimModel;
}

namespace katana::qt {

struct RoadServices {
    katana::cad::Document* document = nullptr;
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    // The command log; isError also flashes the message in the status bar.
    std::function<void(const QString& text, bool isError)> log;
    // The window's one executor (command_runner.hpp).
    CommandRunner run;
    // True in a headless session. May be empty: never headless.
    std::function<bool()> headless;
    // Terrain > Alignment Manager, where an alignment and its design profile
    // are made: opened when a corridor command finds none to use.
    std::function<void()> showAlignmentManager;
    // Replaces the road model's meshes among the 3D views' meshes. They
    // point into objects this workbench keeps alive until it hands over the
    // next ones.
    std::function<void(std::vector<katana::cad::SceneMesh> meshes)> setRoadMeshes;
    // Repaints every view, for the plan outlines.
    std::function<void()> refreshViews;
};

class RoadWorkbench {
  public:
    // Fills `bimMenu`, which the window has made; the toolbar is filled by
    // fillToolBar once the window has placed it.
    RoadWorkbench(QMainWindow& window, RoadServices services, QMenu& bimMenu);
    ~RoadWorkbench();

    RoadWorkbench(const RoadWorkbench&) = delete;
    RoadWorkbench& operator=(const RoadWorkbench&) = delete;

    void fillToolBar(QToolBar& toolBar);
    // For Terrain's Corridors section: the same actions.
    [[nodiscard]] QAction* corridorsAction() const { return corridors_; }
    [[nodiscard]] QAction* quantitiesAction() const { return quantities_; }
    // Makes the road model's 3D meshes again now (the window calls it when
    // it has put a new drawing in place).
    void syncViews();

  private:
    void open(QPointer<QDialog>& slot, const std::function<QDialog*()>& make);
    // False, having said why and where an alignment is made, when no
    // alignment has a design profile.
    bool haveProfiledAlignment();
    // Whether Corridors... and Corridor Quantities... open: when the drawing
    // has a corridor (its dialogs show it, and its problems, whatever became of
    // its alignment), else when an alignment has a design profile to make one
    // from (haveProfiledAlignment, which says where one is made when none has).
    bool corridorDialogsMayOpen();
    void scheduleSync();

    QMainWindow& window_;
    RoadServices services_;
    QAction* templates_ = nullptr;
    QAction* corridors_ = nullptr;
    QAction* quantities_ = nullptr;
    QAction* show_ = nullptr;
    QPointer<QDialog> templateManager_;
    QPointer<QDialog> corridorDialog_;
    QPointer<QDialog> quantitiesDialog_;
    // The model whose meshes the views were last handed; kept alive while
    // they point into it.
    std::shared_ptr<const katana::cad::bim::BimModel> shown_;
    std::vector<const katana::geometry::TriangleMesh*> showing_;
    bool handedOver_ = false;
    bool syncPending_ = false;
    std::unique_ptr<QObject> guard_;
    // Last, so it goes first: it captures this object.
    katana::cad::Document::ListenerHandle listener_;
};

} // namespace katana::qt
