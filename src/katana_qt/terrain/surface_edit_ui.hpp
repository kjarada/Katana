#pragma once

// Terrain > Edit Surface and the Surface Edit toolbar (docs/surface_editing.md,
// "In the window"), made apart from the window so main_window.cpp only places
// them.
//
//   terrainEditMenu        Terrain > Edit Surface: the catalogue's Terrain
//                          tools (filled by tools::fillToolMenus, the window
//                          handing it this menu as the "Terrain" category's),
//                          then the dialogs:
//   terrainSurfaceHeights    Raise, Lower or Set Level... (surfaceHeightsDialog)
//   terrainSurfaceSmooth     Smooth... (surfaceSmoothDialog)
//   terrainSurfaceThin       Thin... (surfaceThinDialog)
//   terrainSurfaceDeleteLong Delete Long Triangles... (surfaceDeleteLongDialog)
//   terrainSurfaceDeletePoints    Delete Points Inside... (surfaceDeletePointsDialog)
//   terrainSurfaceDeleteTriangles Delete Triangles by Polygons... (surfaceDeleteTrianglesDialog)
//   terrainSurfaceAddPoints       Add Points... (surfaceAddPointsDialog)
//   terrainCombineSurfaces   Terrain > Combine Surfaces... (surfaceCombineDialog)
//   SurfaceEditToolBar     the tools again, the dialogs, and at its head
//   surfaceEditActive      the surface the tools act on (cad::setSurfaceEditTarget):
//                          how a person picks it, besides a tool's own [Surface],
//                          which it follows (cad::addSurfaceEditTargetObserver)
//
// Each dialog's action names its dialog in its data, as --dialog finds one.

#include <QKeySequence>
#include <QPointer>
#include <QString>

#include <functional>
#include <memory>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "icons.hpp"
#include "katana/cad/document.hpp"

class QAction;
class QComboBox;
class QMenu;
class QToolBar;
class QWidget;

namespace katana::qt {

class SurfaceCombineDialog;
class SurfaceLevelsDialog;

struct SurfaceEditServices {
    QWidget* window = nullptr; // the dialogs' parent
    katana::cad::Document* document = nullptr;
    CommandRunner run;
    std::function<std::vector<ScopeFilterView>()> views;
    // The window's own makers, so the items look and are named as its others.
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& name)>
        makeAction;
    std::function<QToolBar*(const QString& title)> makeToolBar;
};

class SurfaceEditUi {
  public:
    explicit SurfaceEditUi(SurfaceEditServices services);
    ~SurfaceEditUi();
    SurfaceEditUi(const SurfaceEditUi&) = delete;
    SurfaceEditUi& operator=(const SurfaceEditUi&) = delete;

    // Made before the tool catalogue fills the menus, so it can fill these.
    [[nodiscard]] QMenu* menu() const { return menu_; }
    [[nodiscard]] QToolBar* toolBar() const { return toolBar_; }
    // After the tools are in: the dialogs' items below them, and the Edit
    // Surface submenu and Combine Surfaces at the end of `terrain` (its
    // Surfaces section, where the window calls this).
    void finish(QMenu& terrain);
    // Refills the toolbar's surface list from the drawing.
    void refreshSurfaces();
    // Points the list at the surface the tools act on now, without choosing it
    // again: a tool's own [Surface] option sets the target, not the list.
    void syncActive();

  private:
    void showLevels(int kind);
    void showCombine();

    SurfaceEditServices services_;
    QMenu* menu_ = nullptr;
    QToolBar* toolBar_ = nullptr;
    QPointer<QComboBox> active_;
    std::vector<QAction*> dialogActions_;
    QAction* combine_ = nullptr;
    katana::cad::Document::ListenerHandle listener_;
    std::size_t targetObserver_ = 0;
};

} // namespace katana::qt
