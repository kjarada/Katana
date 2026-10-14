// The window's part of surface display (docs/surface_display.md, "In the
// window"): Terrain > Surface Styles... and the style manager it opens, and
// the 3D views' surfaces drawn in their styles. MainWindow's own members,
// kept out of main_window.cpp so that file's edit is a few lines.

#include <QAction>
#include <QTimer>

#include "katana/cad/document.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "main_window.hpp"
#include "terrain/surface_style_manager.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

namespace cad = katana::cad;

QAction* MainWindow::buildSurfaceDisplay()
{
    QAction* styles = makeAction(Icon::SurfaceStyles, "Surface St&yles...",
                                 "Make, edit and choose how surfaces are drawn: triangles, "
                                 "contours, faces coloured by height, slope or aspect, in plan and 3D",
                                 {}, "terrainSurfaceStyles");
    // The dialog it shows, by object name: how --dialog finds it.
    styles->setData(QString("surfaceStyleManagerDialog"));
    connect(styles, &QAction::triggered, this, [this] { showSurfaceStyleManager(); });

    // An edit of the styles (SURFACESTYLE, the manager, an undo of either)
    // or another drawing restyles the 3D views' surfaces. QUEUED, as every
    // refresh here is (the listener fires inside Document::execute), and
    // once for a burst of changes.
    surfaceStylesListener_ = document_.addListener([this](const cad::DocumentChange& change) {
        if (!change.has(cad::DocumentChange::SurfaceStyles | cad::DocumentChange::Replaced) ||
            surfaceRestylePending_) {
            return;
        }
        surfaceRestylePending_ = true;
        QTimer::singleShot(0, this, [this] { restyleSceneSurfaces(); });
    });
    return styles;
}

void MainWindow::showSurfaceStyleManager()
{
    if (surfaceStyleManager_ == nullptr) {
        SurfaceStyleManagerContext context;
        context.document = &document_;
        context.run = commandRunner();
        surfaceStyleManager_ = new SurfaceStyleManagerDialog(std::move(context), this);
    }
    surfaceStyleManager_->show();
    surfaceStyleManager_->raise();
    surfaceStyleManager_->activateWindow();
}

void MainWindow::styleSceneSurfaces(std::vector<cad::SceneSurface>& items) const
{
    const cad::SurfaceStyleSet& styles = document_.surfaceStyles();
    for (cad::SceneSurface& item : items) {
        const katana::terrain::NamedSurface* named = document_.surfaces().find(item.name);
        if (named == nullptr || named->surface.get() != item.surface) {
            continue;
        }
        // Its shading and colours from its style (cad::styleOf: its own, else
        // the drawing's default), and shown as the drawing says.
        cad::applySurfaceDisplay(item, cad::styleOf(styles, *named));
        item.visible = named->visible;
    }
}

void MainWindow::restyleSceneSurfaces()
{
    surfaceRestylePending_ = false;
    styleSceneSurfaces(sceneSurfaces_);
    // The views hold a pointer to the vector, so this is a refresh: every
    // 3D view rebuilds its terrain at its next frame.
    views_->setSurfaces(&sceneSurfaces_);
    views_->refreshAll();
}

} // namespace katana::qt
