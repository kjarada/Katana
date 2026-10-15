// Terrain > Edit Surface and the Surface Edit toolbar (surface_edit_ui.hpp).

#include "terrain/surface_edit_ui.hpp"

#include <QAction>
#include <QComboBox>
#include <QDialog>
#include <QMenu>
#include <QToolBar>
#include <QWidget>

#include <utility>

#include "katana/cad/terrain/surface_edit_verbs.hpp"
#include "terrain/surface_edit_dialogs.hpp"

namespace katana::qt {

SurfaceEditUi::SurfaceEditUi(SurfaceEditServices services) : services_(std::move(services))
{
    menu_ = new QMenu("Edi&t Surface", services_.window);
    menu_->setObjectName("terrainEditMenu");
    menu_->setIcon(icon(Icon::SurfaceEdit));
    menu_->menuAction()->setStatusTip(
        "Edit the active surface's triangles, points, breaklines, boundary and levels, each one "
        "undo step (SURFACE EDIT)");

    toolBar_ = services_.makeToolBar("Surface Edit");
    // The name the brief and the tests know it by; makeToolBar's would hold a blank.
    toolBar_->setObjectName("SurfaceEditToolBar");
    active_ = new QComboBox(toolBar_);
    active_->setObjectName("surfaceEditActive");
    active_->setMinimumContentsLength(10);
    active_->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    active_->setToolTip("<b>Active surface</b><br>The surface the Edit Surface tools act on. A "
                        "tool's own S option chooses it too.");
    toolBar_->addWidget(active_);
    toolBar_->addSeparator();
    // currentIndexChanged, not activated: a choice made by the program (the
    // headless driver fills the list) must point the tools too, and a person's
    // choice changes the index as well. refreshSurfaces and syncActive block
    // the signal, so neither sets the target again.
    QObject::connect(active_, &QComboBox::currentIndexChanged, active_, [this](int index) {
        if (index >= 0) {
            cad::setSurfaceEditTarget(active_->itemText(index).toStdString());
        }
    });
    targetObserver_ = cad::addSurfaceEditTargetObserver([this] { syncActive(); });

    if (services_.document != nullptr) {
        listener_ = services_.document->addListener([this](const cad::DocumentChange& change) {
            if (change.has(cad::DocumentChange::Surfaces)) {
                refreshSurfaces();
            }
        });
    }
    refreshSurfaces();
}

SurfaceEditUi::~SurfaceEditUi() { cad::removeSurfaceEditTargetObserver(targetObserver_); }

void SurfaceEditUi::finish(QMenu& terrain)
{
    struct Item {
        Icon icon;
        const char* text;
        const char* tip;
        const char* name;
        const char* dialog;
        int kind;
    };
    // The letters are each one item's in the submenu. The tools choose theirs
    // (tools::withMnemonic) after Combine Surfaces has B, so Add Breakline,
    // finding A and B taken, takes R: the levels item is V, not R, which
    // --check-shortcuts found taken twice. These take V, M, N and L.
    static const Item kItems[] = {
        {Icon::SurfaceLevels, "Raise, Lower or Set Le&vel...",
         "Raise, lower, set or scale the levels of the surface, or of the part inside a polygon",
         "terrainSurfaceHeights", "surfaceHeightsDialog",
         static_cast<int>(SurfaceLevelsKind::Heights)},
        {Icon::SurfaceSmooth, "S&mooth...",
         "Smooth the surface's levels, holding its rim and breaklines", "terrainSurfaceSmooth",
         "surfaceSmoothDialog", static_cast<int>(SurfaceLevelsKind::Smooth)},
        {Icon::SurfaceThin, "Thi&n...",
         "Remove the points that move the surface by no more than a tolerance",
         "terrainSurfaceThin", "surfaceThinDialog", static_cast<int>(SurfaceLevelsKind::Thin)},
        {Icon::SurfaceDeleteLong, "Delete &Long Triangles...",
         "Delete the triangles with a side too long or an angle too thin",
         "terrainSurfaceDeleteLong", "surfaceDeleteLongDialog",
         static_cast<int>(SurfaceLevelsKind::DeleteLong)},
        // The tools take one picked entity; these take any number, by a scope
        // (the polygons of a layer, the selection, what a view shows) as every
        // tool that reads the drawing does. No underlined letter: the tools
        // above took the free ones, and --check-shortcuts counts each once.
        {Icon::SurfaceEdit, "Delete Points Inside Polygons...",
         "Delete the points of the surface inside the polygons a scope takes (breakline and rim "
         "points are kept)",
         "terrainSurfaceDeletePoints", "surfaceDeletePointsDialog",
         static_cast<int>(SurfaceLevelsKind::DeletePoints)},
        {Icon::SurfaceDeleteLong, "Delete Triangles by Polygons...",
         "Delete the triangles inside, or outside, the union of the polygons a scope takes",
         "terrainSurfaceDeleteTriangles", "surfaceDeleteTrianglesDialog",
         static_cast<int>(SurfaceLevelsKind::DeleteTriangles)},
        {Icon::SurfaceLevels, "Add Points from the Drawing...",
         "Add the points and levelled vertices a scope takes to the surface; a point off it "
         "extends it",
         "terrainSurfaceAddPoints", "surfaceAddPointsDialog",
         static_cast<int>(SurfaceLevelsKind::AddPoints)},
    };
    menu_->addSection("Levels and Triangles");
    toolBar_->addSeparator();
    for (const Item& item : kItems) {
        QAction* action = services_.makeAction(item.icon, item.text, item.tip, QKeySequence(),
                                               item.name);
        action->setData(QString(item.dialog));
        const int kind = item.kind;
        QObject::connect(action, &QAction::triggered, action, [this, kind] { showLevels(kind); });
        menu_->addAction(action);
        toolBar_->addAction(action);
        dialogActions_.push_back(action);
    }
    combine_ = services_.makeAction(
        Icon::SurfaceCombine, "Com&bine Surfaces...",
        "Merge surfaces, paste one into another with a clean seam, or make a clipped or "
        "offset copy",
        QKeySequence(), "terrainCombineSurfaces");
    combine_->setData(QString("surfaceCombineDialog"));
    QObject::connect(combine_, &QAction::triggered, combine_, [this] { showCombine(); });
    toolBar_->addAction(combine_);
    terrain.addMenu(menu_);
    terrain.addAction(combine_);
}

void SurfaceEditUi::syncActive()
{
    if (active_.isNull() || services_.document == nullptr) {
        return;
    }
    const auto name = cad::activeEditSurface(*services_.document);
    if (!name) {
        return;
    }
    const int at = active_->findText(QString::fromStdString(*name));
    if (at >= 0 && at != active_->currentIndex()) {
        active_->blockSignals(true);
        active_->setCurrentIndex(at);
        active_->blockSignals(false);
    }
}

void SurfaceEditUi::refreshSurfaces()
{
    if (active_.isNull()) {
        return;
    }
    active_->blockSignals(true);
    active_->clear();
    if (services_.document != nullptr) {
        for (const auto& surface : services_.document->surfaces().all()) {
            active_->addItem(QString::fromStdString(surface.name));
        }
        if (const auto name = cad::activeEditSurface(*services_.document)) {
            active_->setCurrentIndex(active_->findText(QString::fromStdString(*name)));
        }
    }
    active_->setEnabled(active_->count() > 0);
    active_->blockSignals(false);
}

void SurfaceEditUi::showLevels(int kind)
{
    const auto levels = static_cast<SurfaceLevelsKind>(kind);
    const QString name = surfaceLevelsPrefix(levels) + "Dialog";
    auto* dialog = services_.window->findChild<QDialog*>(name);
    if (dialog == nullptr) {
        dialog = new SurfaceLevelsDialog(
            levels, SurfaceEditDialogContext{services_.run, services_.document, services_.views},
            services_.window);
    }
    dialog->show();
    dialog->raise();
    dialog->activateWindow();
}

void SurfaceEditUi::showCombine()
{
    auto* dialog = services_.window->findChild<QDialog*>("surfaceCombineDialog");
    if (dialog == nullptr) {
        dialog = new SurfaceCombineDialog(
            SurfaceEditDialogContext{services_.run, services_.document, services_.views},
            services_.window);
    }
    dialog->show();
    dialog->raise();
    dialog->activateWindow();
}

} // namespace katana::qt
