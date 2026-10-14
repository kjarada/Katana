#pragma once

// The Surfaces panel (docs/terrain.md, "Surfaces in the drawing"): the
// drawing's surfaces as a live table - shown or not, name, display style,
// source, triangles - and the tools that act on the one picked. It does no
// work itself: every change builds the verb line a person would type
// (SURFACE SHOW|HIDE|RENAME|STYLE|COPY|REMOVE, cad/terrain/surface_verbs.hpp
// and the geoprocessing SURFACE REMOVE) and hands it to the window's one
// executor (command_runner.hpp), so it is logged, undone and scripted like
// a typed line.
//
//   surfacesPanel          the panel
//   surfacesTable          one row a surface: its Name, with a check box for
//                          whether the views show it (SURFACE SHOW|HIDE),
//                          edited in place (SURFACE RENAME); Style (a combo
//                          per row, "surfaceStyle": SURFACE STYLE); Source;
//                          Triangles
//   surfacesRename         starts editing the picked surface's name
//   surfacesCopy           SURFACE COPY <name> <name (2)>
//   surfacesRemove         SURFACE REMOVE <name> (one undo step)
//   surfacesZoomTo         frames the surface in the plan views
//   surfacesEditStyles     Terrain > Surface Styles... (terrainSurfaceStyles)
//
// installSurfacesPanel makes its dock ("SurfacesDock", hidden until asked
// for), View > Panels > Surfaces ("viewSurfacesPanel") and Terrain >
// Surfaces Panel ("terrainSurfacesPanel"), in one call so the window gains a
// few lines.

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <string_view>

#include <QString>
#include <QWidget>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/geometry/primitives2d.hpp"

class QAction;
class QDockWidget;
class QLabel;
class QMainWindow;
class QMenu;
class QTableWidget;
class QTableWidgetItem;
class QToolButton;

namespace katana::qt {

class SurfacesPanel final : public QWidget {
  public:
    struct Services {
        // The window's one executor; every change is a line through it.
        CommandRunner run;
        // Frames a box in the plan views (Zoom To). Empty: the button is off.
        std::function<void(const katana::geometry::Box2&)> zoomTo;
        // Opens the style manager (Edit Styles...). Empty: the button is off.
        std::function<void()> editStyles;
    };

    // The table's columns.
    // The name first, holding the shown check box: a row is found by its
    // name (a headless --fill picks a row by the first column's text).
    enum Column { Name = 0, Style, Source, Triangles, kColumns };

    SurfacesPanel(katana::cad::Document& document, Services services,
                  QWidget* parent = nullptr);

    // Reads the drawing's surfaces and styles again when either has changed
    // since the last look (their generations); the picked surface stays
    // picked. A change of the document is read on the event loop
    // (scheduleRefresh), never inside the notification.
    void refresh();

    // The picked surface's name, if a row is picked.
    [[nodiscard]] std::optional<std::string> pickedSurface() const;
    // Picks the surface of that name (any case); false when there is none.
    bool pickSurface(std::string_view name);
    // What the last line the panel ran said, or why it was refused.
    [[nodiscard]] QString lastReply() const;

  private:
    void onItemChanged(QTableWidgetItem* item);
    void onStyleChosen(int row, int index);
    void renamePicked();
    void copyPicked();
    void removePicked();
    void zoomToPicked();
    void updateButtons();
    // One refresh queued on the event loop, however often it is asked for
    // before it runs.
    void scheduleRefresh();
    // Runs `line` through the executor; what it said is lastReply().
    void run(const QString& line);

    katana::cad::Document& document_;
    Services services_;
    katana::cad::Document::ListenerHandle listener_;
    QLabel* status_ = nullptr;
    QTableWidget* table_ = nullptr;
    QToolButton* rename_ = nullptr;
    QToolButton* copy_ = nullptr;
    QToolButton* remove_ = nullptr;
    QToolButton* zoomTo_ = nullptr;
    QToolButton* editStyles_ = nullptr;
    std::uint64_t surfacesSeen_ = 0;
    std::uint64_t stylesSeen_ = 0;
    bool shownOnce_ = false;
    bool filling_ = false; // the table is being filled, not edited
    bool refreshPending_ = false;
    QString lastReply_;
};

// The panel's dock and its two ways in.
struct SurfacesPanelUi {
    QDockWidget* dock = nullptr;
    SurfacesPanel* panel = nullptr;
    QAction* toggle = nullptr;      // View > Panels > Surfaces
    QAction* terrainItem = nullptr; // Terrain > Surfaces Panel
    // Shows and raises the dock.
    void show() const;
};

// `panels` is View > Panels and `terrainMenu` the Terrain menu (either may be
// null); the Terrain item goes after Surface From Drawing when the menu has
// it ("surfaceFromDrawing"), else at the menu's end. `beside` is the dock the
// panel is tabbed with when it is shown; may be null.
SurfacesPanelUi installSurfacesPanel(QMainWindow& window, katana::cad::Document& document,
                                     SurfacesPanel::Services services, QMenu* panels,
                                     QMenu* terrainMenu, QDockWidget* beside);

} // namespace katana::qt
