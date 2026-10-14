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
    QObject::connect(active_, &QComboBox::activated, active_, [this](int index) {
        if (index >= 0) {
            cad::setSurfaceEditTarget(active_->itemText(index).toStdString());
        }
    });

    if (services_.document != nullptr) {
        listener_ = services_.document->addListener([this](const cad::DocumentChange& change) {
            if (change.has(cad::DocumentChange::Surfaces)) {
                refreshSurfaces();
            }
        });
    }
    refreshSurfaces();
}

SurfaceEditUi::~SurfaceEditUi() = default;

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
    // The letters are each one item's in the submenu: the tools take D, F, A,
    // P, T, B, O and H (tools::withMnemonic), so these take R, M, N and L.
    static const Item kItems[] = {
        {Icon::SurfaceLevels, "&Raise, Lower or Set Level...",
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

void SurfaceEditUi::refreshSurfaces()
{
    if (active_ == nullptr) {
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
