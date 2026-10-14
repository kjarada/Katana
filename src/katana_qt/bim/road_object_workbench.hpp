#pragma once

// BIM > Road Objects (docs/road_objects.md, "In the window"): the ROADOBJECT
// verb in the window.
//
// Built as the utilities' workbench is (survey/utility_workbench.hpp):
// MainWindow only constructs it, hands it the BIM menu and toolbar and a few
// callbacks (RoadObjectServices), and keeps it. The verb is the
// CommandInterpreter's, shared with katana_cli and katana_mcp, and a typed
// ROADOBJECT line reaches it as any other does; what this class adds is what
// only a window has:
//   - the menu's Road Objects section: Barrier, Guide Posts, Sign, Light
//     Poles, Culvert, Wall, Fence and Line Marking (bimRoadBarrier,
//     bimRoadGuidePosts, bimRoadSign, bimRoadLightPoles, bimRoadCulvert,
//     bimRoadWall, bimRoadFence, bimRoadLineMarking), each opening the one
//     placing dialog (road_object_dialog.hpp) on its own page, and Road
//     Objects... (bimRoadObjects), the manager (road_object_manager.hpp);
//   - toolbar buttons for the ones placed most: barrier, sign, light poles,
//     line marking, and the manager.
// Both dialogs run their lines through the window's one executor
// (RoadObjectServices::run, MainWindow::runVerbLine), so a line from a dialog
// is echoed, kept in the history and undone exactly as a typed one.

#include <QPointer>
#include <QString>

#include <array>
#include <functional>

#include "command_runner.hpp"
#include "icons.hpp"
#include "katana/cad/road/road_objects.hpp"

class QAction;
class QKeySequence;
class QMainWindow;
class QMenu;
class QToolBar;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class RoadObjectDialog;
class RoadObjectManagerDialog;
class ViewWorkspace;

struct RoadObjectServices {
    // The drawing, whose alignments, surfaces and layers the dialogs list.
    // May be null (a test's).
    katana::cad::Document* document = nullptr;
    // The window's views, offered to the placing dialog's scope. May be null.
    ViewWorkspace* views = nullptr;
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    // The window's one executor (MainWindow::runVerbLine).
    CommandRunner run;
};

class RoadObjectWorkbench {
  public:
    // Adds the "Road Objects" section and its actions to `bimMenu`, after
    // what is already there, and the common ones to `bimBar`.
    RoadObjectWorkbench(QMainWindow& window, RoadObjectServices services, QMenu& bimMenu,
                        QToolBar& bimBar);
    // Deletes the dialogs, which call back into this object.
    ~RoadObjectWorkbench();

    RoadObjectWorkbench(const RoadObjectWorkbench&) = delete;
    RoadObjectWorkbench& operator=(const RoadObjectWorkbench&) = delete;

    [[nodiscard]] QAction* action(katana::cad::road::RoadObjectKind kind) const;
    [[nodiscard]] QAction* managerAction() const { return manager_; }

    // The dialogs, each made the first time it is asked for.
    RoadObjectDialog& dialog();
    RoadObjectManagerDialog& manager();
    // Shows the placing dialog on `kind`'s page.
    void open(katana::cad::road::RoadObjectKind kind);
    void openManager();

  private:
    QMainWindow& window_;
    RoadObjectServices services_;
    std::array<QAction*, katana::cad::road::kRoadObjectKindCount> actions_{};
    QAction* manager_ = nullptr;
    QPointer<RoadObjectDialog> dialog_;
    QPointer<RoadObjectManagerDialog> managerDialog_;
};

} // namespace katana::qt
