// BIM > Road Objects (bim/road_object_workbench.hpp).

#include "bim/road_object_workbench.hpp"

#include <QAction>
#include <QKeySequence>
#include <QMainWindow>
#include <QMenu>
#include <QToolBar>

#include <cstddef>
#include <utility>
#include <vector>

#include "bim/road_object_dialog.hpp"
#include "bim/road_object_manager.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

using katana::cad::road::RoadObjectKind;

namespace {

// The section, in the order of the dialog's pages. The letters are ones the
// BIM menu's road model section leaves (--check-shortcuts): B, G, N, L, U, W,
// F, M and, for the manager, O.
struct KindAction {
    RoadObjectKind kind;
    Icon icon;
    const char* text;
    const char* tip;
    const char* name;
    // Shown on the BIM toolbar too: the ones placed most.
    bool onToolbar;
};

constexpr std::array<KindAction, katana::cad::road::kRoadObjectKindCount> kActions{{
    {RoadObjectKind::Barrier, Icon::RoadBarrier, "&Barrier...",
     "Place a safety barrier - W-beam, thrie-beam, concrete F-shape or single slope, wire rope "
     "- along an alignment or along lines in the drawing: the ROADOBJECT ADD line, one undo step",
     "bimRoadBarrier", true},
    {RoadObjectKind::GuidePost, Icon::RoadGuidePosts, "&Guide Posts...",
     "Place guide posts at a spacing along an alignment or a line, or at points",
     "bimRoadGuidePosts", false},
    {RoadObjectKind::Sign, Icon::RoadSign, "Sig&n...",
     "Place a sign - its plate's shape and size, one post or two, its code and legend - at a "
     "station and offset, along a line or at points",
     "bimRoadSign", true},
    {RoadObjectKind::LightPole, Icon::RoadLightPole, "&Light Poles...",
     "Place light poles - mounting height, outreach arm, lantern - at a spacing along an "
     "alignment or a line, or at points",
     "bimRoadLightPoles", true},
    {RoadObjectKind::Culvert, Icon::RoadCulvert, "C&ulvert...",
     "Place a culvert - pipe or box, cells, size, skew, inverts or cover, headwalls - across an "
     "alignment at a station, or between two points",
     "bimRoadCulvert", false},
    {RoadObjectKind::Wall, Icon::RoadWall, "&Wall...",
     "Place a retaining wall or a noise wall - height, thickness, footing - along an alignment "
     "or a line",
     "bimRoadWall", false},
    {RoadObjectKind::Fence, Icon::RoadFence, "&Fence...",
     "Place a fence - posts at a spacing and its wires - along an alignment or a line",
     "bimRoadFence", false},
    {RoadObjectKind::Marking, Icon::RoadLineMarking, "Line &Marking...",
     "Place a line marking - edge, centre, lane or continuity line, with its dash pattern - "
     "along an alignment at an offset, or along a line",
     "bimRoadLineMarking", true},
}};

} // namespace

RoadObjectWorkbench::RoadObjectWorkbench(QMainWindow& window, RoadObjectServices services,
                                         QMenu& bimMenu, QToolBar& bimBar)
    : window_(window), services_(std::move(services))
{
    bimMenu.addSection("Road Objects");
    std::vector<QAction*> onToolbar;
    for (const KindAction& entry : kActions) {
        QAction* made =
            services_.makeAction(entry.icon, entry.text, entry.tip, QKeySequence(), entry.name);
        // The dialog it opens, by object name: how --dialog finds it, since
        // eight actions share one dialog.
        made->setData(QString("roadObjectDialog"));
        const RoadObjectKind kind = entry.kind;
        QObject::connect(made, &QAction::triggered, &window_, [this, kind] { open(kind); });
        bimMenu.addAction(made);
        actions_[static_cast<std::size_t>(kind)] = made;
        if (entry.onToolbar) {
            onToolbar.push_back(made);
        }
    }
    manager_ = services_.makeAction(
        Icon::RoadObjects, "Road &Objects...",
        "The drawing's road objects: list them with their quantities and problems, set their "
        "keys, copy, delete and zoom to them",
        QKeySequence(), "bimRoadObjects");
    manager_->setData(QString("roadObjectManagerDialog"));
    QObject::connect(manager_, &QAction::triggered, &window_, [this] { openManager(); });
    bimMenu.addAction(manager_);
    onToolbar.push_back(manager_);

    // A group of their own after whatever the bar holds already - its name,
    // and the road model's buttons.
    bimBar.addSeparator();
    for (QAction* each : onToolbar) {
        bimBar.addAction(each);
    }
}

RoadObjectWorkbench::~RoadObjectWorkbench()
{
    // The dialogs are the window's children only so that they float over it;
    // they call back into this object, so they go with it.
    delete managerDialog_.data();
    delete dialog_.data();
}

QAction* RoadObjectWorkbench::action(RoadObjectKind kind) const
{
    return actions_[static_cast<std::size_t>(kind)];
}

RoadObjectDialog& RoadObjectWorkbench::dialog()
{
    if (dialog_ == nullptr) {
        RoadObjectDialogContext context;
        context.document = services_.document;
        context.run = services_.run;
        if (services_.views != nullptr) {
            ViewWorkspace* views = services_.views;
            context.views = [views] { return scopeFilterViews(views->viewSet()); };
        }
        dialog_ = new RoadObjectDialog(std::move(context), &window_);
    }
    return *dialog_;
}

RoadObjectManagerDialog& RoadObjectWorkbench::manager()
{
    if (managerDialog_ == nullptr) {
        RoadObjectManagerContext context;
        context.document = services_.document;
        context.run = services_.run;
        context.place = [this](RoadObjectKind kind) { open(kind); };
        if (services_.views != nullptr) {
            ViewWorkspace* views = services_.views;
            context.views = [views] { return scopeFilterViews(views->viewSet()); };
        }
        managerDialog_ = new RoadObjectManagerDialog(std::move(context), &window_);
    }
    return *managerDialog_;
}

void RoadObjectWorkbench::open(RoadObjectKind kind)
{
    RoadObjectDialog& shown = dialog();
    shown.showKind(kind);
    shown.show();
    shown.raise();
    shown.activateWindow();
}

void RoadObjectWorkbench::openManager()
{
    RoadObjectManagerDialog& shown = manager();
    shown.reload();
    shown.show();
    shown.raise();
    shown.activateWindow();
}

} // namespace katana::qt
