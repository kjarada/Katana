#include "road_workbench.hpp"

#include <QAction>
#include <QDialog>
#include <QMainWindow>
#include <QMenu>
#include <QObject>
#include <QTimer>
#include <QToolBar>

#include <utility>

#include "bim/corridor_dialog.hpp"
#include "bim/corridor_quantities_dialog.hpp"
#include "bim/road_template_manager.hpp"
#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/bim/bim_view.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "plan_painter.hpp"

namespace katana::qt {

namespace cad = katana::cad;

RoadWorkbench::RoadWorkbench(QMainWindow& window, RoadServices services, QMenu& bimMenu)
    : window_(window), services_(std::move(services)), guard_(std::make_unique<QObject>())
{
    templates_ = services_.makeAction(
        Icon::RoadTemplate, "Road &Template Manager...",
        "Road templates: lanes, shoulders, kerbs, footpaths, verges, medians, drains and "
        "batters, and their pavement courses",
        QKeySequence(), "bimRoadTemplates");
    templates_->setData(QStringLiteral("roadTemplateManagerDialog"));
    corridors_ = services_.makeAction(
        Icon::Corridor, "Corr&idors...",
        "Corridors: a road template along an alignment, its regions, superelevation and target "
        "surface; build its strings and surfaces",
        QKeySequence(), "bimCorridors");
    corridors_->setData(QStringLiteral("corridorDialog"));
    quantities_ = services_.makeAction(
        Icon::CorridorQuantities, "Corridor &Quantities...",
        "Each course's volume and area, each kerb's length, the earthworks cut and fill: the "
        "road model's quantities, and a CSV of them",
        QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Q), "bimCorridorQuantities");
    quantities_->setData(QStringLiteral("corridorQuantitiesDialog"));
    show_ = services_.makeAction(Icon::RoadModel, "Show Road Mod&el",
                                 "Draw the road model's 3D objects in the 3D views and their "
                                 "outlines in plan",
                                 QKeySequence(), "bimShowRoadModel");
    show_->setCheckable(true);
    show_->setChecked(roadModelShown());

    bimMenu.addSection("Templates");
    bimMenu.addAction(templates_);
    bimMenu.addSection("Corridors");
    bimMenu.addAction(corridors_);
    bimMenu.addSection("Model");
    bimMenu.addAction(show_);
    bimMenu.addSection("Quantities");
    bimMenu.addAction(quantities_);

    QObject::connect(templates_, &QAction::triggered, guard_.get(), [this] {
        open(templateManager_, [this] {
            RoadTemplateManagerContext context;
            context.document = services_.document;
            context.run = services_.run;
            context.headless = services_.headless;
            return new RoadTemplateManagerDialog(std::move(context), &window_);
        });
    });
    QObject::connect(corridors_, &QAction::triggered, guard_.get(), [this] {
        if (!corridorDialogsMayOpen()) {
            return;
        }
        open(corridorDialog_, [this] {
            CorridorDialogContext context;
            context.document = services_.document;
            context.run = services_.run;
            return new CorridorDialog(std::move(context), &window_);
        });
    });
    QObject::connect(quantities_, &QAction::triggered, guard_.get(), [this] {
        if (!corridorDialogsMayOpen()) {
            return;
        }
        open(quantitiesDialog_, [this] {
            CorridorQuantitiesContext context;
            context.document = services_.document;
            context.run = services_.run;
            context.headless = services_.headless;
            return new CorridorQuantitiesDialog(std::move(context), &window_);
        });
    });
    QObject::connect(show_, &QAction::toggled, guard_.get(), [this](bool on) {
        setRoadModelShown(on);
        syncViews();
        if (services_.log) {
            services_.log(QString("road model %1").arg(on ? "shown" : "hidden"), false);
        }
    });

    if (services_.document != nullptr) {
        listener_ = services_.document->addListener([this](const cad::DocumentChange& change) {
            // What the road model and its visibility are made from.
            if (change.has(cad::DocumentChange::RoadModel | cad::DocumentChange::Surfaces |
                           cad::DocumentChange::Alignments | cad::DocumentChange::Layers |
                           cad::DocumentChange::Replaced)) {
                scheduleSync();
            }
        });
    }
}

RoadWorkbench::~RoadWorkbench()
{
    listener_.reset();
    // The dialogs hold the document, which the window destroys before its
    // children: they go with this object.
    delete templateManager_.data();
    delete corridorDialog_.data();
    delete quantitiesDialog_.data();
}

void RoadWorkbench::fillToolBar(QToolBar& toolBar)
{
    toolBar.addAction(templates_);
    toolBar.addAction(corridors_);
    toolBar.addAction(quantities_);
    toolBar.addSeparator();
    toolBar.addAction(show_);
}

void RoadWorkbench::open(QPointer<QDialog>& slot, const std::function<QDialog*()>& make)
{
    if (slot.isNull()) {
        slot = make();
    }
    slot->show();
    slot->raise();
    slot->activateWindow();
}

bool RoadWorkbench::corridorDialogsMayOpen()
{
    // A corridor is made from an alignment with a design profile, so with
    // none and no corridor there is nothing for the dialogs to do but say
    // where one is made. A drawing that already has corridors has something to
    // show whatever its alignments are: a corridor whose profile was cleared
    // or whose alignment was deleted is exactly what the Corridors dialog's
    // problems are there to name, and to rename, delete or re-point.
    if (services_.document != nullptr && !services_.document->corridors().corridors.empty()) {
        return true;
    }
    return haveProfiledAlignment();
}

bool RoadWorkbench::haveProfiledAlignment()
{
    if (services_.document == nullptr) {
        return false;
    }
    for (const katana::entity::Alignment& alignment :
         services_.document->model().alignments.all()) {
        if (alignment.vertical.has_value()) {
            return true;
        }
    }
    if (services_.log) {
        services_.log("No alignment has a design profile. Define one in Terrain > Alignment "
                      "Manager (Vertical), or with ALIGN DESIGN name s,z ...",
                      true);
    }
    // Where one is defined, opened for the person; a headless run has
    // nobody to show it to.
    const bool headless = services_.headless && services_.headless();
    if (!headless && services_.showAlignmentManager) {
        services_.showAlignmentManager();
    }
    return false;
}

void RoadWorkbench::scheduleSync()
{
    // From the event loop, once however many changes arrived: a listener
    // must not rebuild inside the command that notified, and a script of a
    // hundred template edits makes the model once.
    if (syncPending_) {
        return;
    }
    syncPending_ = true;
    QTimer::singleShot(0, guard_.get(), [this] {
        syncPending_ = false;
        syncViews();
    });
}

void RoadWorkbench::syncViews()
{
    if (services_.document == nullptr || !services_.setRoadMeshes) {
        return;
    }
    std::shared_ptr<const cad::bim::BimModel> model;
    std::vector<cad::SceneMesh> meshes;
    if (show_->isChecked()) {
        model = cad::bim::bimModel(*services_.document);
        meshes = cad::bim::sceneMeshes(*model, services_.document->model(), cad::kNoLayerOverrides);
    }
    // Nothing to hand over when it is the same model showing the same
    // objects: a layer switched on elsewhere would make every 3D view build
    // its terrain again for nothing.
    std::vector<const katana::geometry::TriangleMesh*> showing;
    showing.reserve(meshes.size());
    for (const cad::SceneMesh& mesh : meshes) {
        showing.push_back(mesh.mesh);
    }
    if (model == shown_ && showing == showing_ && handedOver_) {
        return;
    }
    // The views take the new meshes before the model the old ones point
    // into is let go.
    services_.setRoadMeshes(std::move(meshes));
    shown_ = std::move(model);
    showing_ = std::move(showing);
    handedOver_ = true;
    if (services_.refreshViews) {
        services_.refreshViews();
    }
}

} // namespace katana::qt
