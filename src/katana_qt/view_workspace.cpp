#include "view_workspace.hpp"

#include <algorithm>
#include <array>

#include <QActionGroup>
#include <QCloseEvent>
#include <QCoreApplication>
#include <QKeyEvent>
#include <QMenu>
#include <QStyle>
#include <QTimer>
#include <QToolButton>

#include "view_layers_popup.hpp"

namespace katana::qt {

using katana::cad::ViewId;
using katana::cad::ViewKind;
using katana::cad::ViewState;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// Every view dock lives in one area of the nested window. Which one does not
// matter - there is no central widget for the areas to surround - but it must
// be the same for all of them, or two views could not be split against each
// other.
constexpr Qt::DockWidgetArea kViewArea = Qt::LeftDockWidgetArea;

bool isRenderKind(ViewKind kind)
{
    return kind == ViewKind::Model3D || kind == ViewKind::Elevation;
}

Icon kindIcon(ViewKind kind)
{
    switch (kind) {
    case ViewKind::Plan:
        return Icon::ViewPlan;
    case ViewKind::Model3D:
        return Icon::View3D;
    case ViewKind::Section:
        return Icon::ViewSection;
    case ViewKind::Elevation:
        return Icon::ViewElevation;
    }
    return Icon::ViewPlan;
}

constexpr std::array kKinds{ViewKind::Plan, ViewKind::Model3D, ViewKind::Section,
                            ViewKind::Elevation};

} // namespace

ViewDock::ViewDock(ViewId id, QWidget* parent) : QDockWidget(parent), id_(id)
{
    // The saved layout finds a dock by its object name, so the id is in it.
    setObjectName(QString("View%1").arg(id));
}

void ViewDock::closeEvent(QCloseEvent* event)
{
    event->ignore();
    if (onCloseRequested) {
        // Queued: the request may come from a button inside this dock.
        QTimer::singleShot(0, this, [this] {
            if (onCloseRequested) {
                onCloseRequested();
            }
        });
    }
}

QWidget* ViewWorkspace::View::widget() const
{
    if (plan != nullptr) {
        return plan;
    }
    if (render != nullptr) {
        return render;
    }
    return section;
}

ViewWorkspace::ViewWorkspace(katana::cad::Document& document, QWidget* parent)
    : QMainWindow(parent), document_(document)
{
    // QMainWindow makes itself a top-level window whatever parent it is given,
    // and a window placed as another main window's central widget is an EMPTY
    // layout item: it was given no space at all and the views vanished.
    setWindowFlags(Qt::Widget);
    // No central widget: the docks are the whole of this window.
    setDockNestingEnabled(true);
    // NOT GroupedDragging. With it, dragging a tabbed view drags its whole
    // tab group out into a QDockWidgetGroupWindow - a window of Qt's with a
    // native frame and none of the chrome - and a view in it can no longer be
    // floated or docked on its own. Without it every floating view is a
    // QDockWidget wearing its own title bar, whatever was dragged.
    setDockOptions(QMainWindow::AnimatedDocks | QMainWindow::AllowNestedDocks |
                   QMainWindow::AllowTabbedDocks);
    // Tabs above the views they switch between, where the eye is looking
    // for a title, rather than under the drawing.
    setTabPosition(Qt::AllDockWidgetAreas, QTabWidget::North);
    openView(ViewKind::Plan);
}

ViewWorkspace::~ViewWorkspace()
{
    for (View& view : docks_) {
        delete view.dock;
    }
    docks_.clear();
}

ViewContext ViewWorkspace::contextFor() const
{
    ViewContext context;
    context.document = &document_;
    context.surfaces = surfaces_;
    context.meshes = meshes_;
    context.options = options_;
    return context;
}

ViewWorkspace::View* ViewWorkspace::find(ViewId id)
{
    for (View& view : docks_) {
        if (view.id == id) {
            return &view;
        }
    }
    return nullptr;
}

const ViewWorkspace::View* ViewWorkspace::find(ViewId id) const
{
    for (const View& view : docks_) {
        if (view.id == id) {
            return &view;
        }
    }
    return nullptr;
}

void ViewWorkspace::buildContent(View& view, ViewState& state)
{
    // A tool running in a plan view that is about to be deleted is stopped
    // first, as Esc stops it: deleted with its widget it would never report
    // that it ended, and the menu and toolbar would go on showing it
    // checked with nothing running.
    if (view.plan != nullptr && view.plan->toolActive()) {
        view.plan->setTool(Tool::Select);
    }
    QWidget* old = view.widget();
    view.plan = nullptr;
    view.render = nullptr;
    view.section = nullptr;
    const ViewId id = state.id;

    switch (state.kind) {
    case ViewKind::Plan: {
        auto* plan = new ViewportWidget(document_, state, view.dock);
        plan->setReferenceData(reference_);
        plan->setMeshes(meshes_);
        plan->setGridVisible(gridVisible_);
        plan->setSnapEnabled(snapEnabled_);
        plan->setSnapModes(snapModes_);
        plan->onPrompt = [this](const QString& text) {
            if (onPrompt) {
                onPrompt(text);
            }
        };
        plan->onError = [this](const QString& text) {
            if (onError) {
                onError(text);
            }
        };
        plan->onCursorMoved = [this](const katana::geometry::Point2& world,
                                     const std::optional<katana::cad::SnapResult>& snap) {
            if (onCursorMoved) {
                onCursorMoved(world, snap);
            }
        };
        wireTools(*plan, id);
        plan->onActivated = [this, id] { activate(id); };
        view.plan = plan;
        break;
    }
    case ViewKind::Model3D:
    case ViewKind::Elevation: {
        auto* render = new RenderViewWidget(contextFor(), state, view.dock);
        render->onActivated = [this, id] { activate(id); };
        render->onStatus = [this](const QString& text) {
            if (onStatus) {
                onStatus(text);
            }
        };
        render->onFrameStats = [this](const QString& text) {
            if (onFrameStats) {
                onFrameStats(text);
            }
        };
        view.render = render;
        break;
    }
    case ViewKind::Section: {
        auto* section = new SectionViewWidget(state, view.dock);
        section->onActivated = [this, id] { activate(id); };
        section->onStatus = [this](const QString& text) {
            if (onStatus) {
                onStatus(text);
            }
        };
        section->onFrameStats = [this](const QString& text) {
            if (onFrameStats) {
                onFrameStats(text);
            }
        };
        view.section = section;
        break;
    }
    }

    view.dock->setWidget(view.widget());
    delete old;
    updateTitle(view);
}

void ViewWorkspace::updateTitle(const View& view)
{
    const ViewState* state = views_.find(view.id);
    if (state == nullptr) {
        return;
    }
    view.dock->setWindowTitle(QString::fromStdString(katana::cad::ViewSet::title(*state)));
    if (view.titleBar != nullptr) {
        view.titleBar->setIcon(kindIcon(state->kind));
    }
    if (view.kindButton != nullptr) {
        view.kindButton->setIcon(icon(kindIcon(state->kind)));
    }
}

void ViewWorkspace::setChrome(DockChrome* chrome)
{
    chrome_ = chrome;
    for (View& view : docks_) {
        installChrome(view);
    }
    updateActiveMarks();
}

void ViewWorkspace::installChrome(View& view)
{
    const ViewState* state = views_.find(view.id);
    if (chrome_ == nullptr || state == nullptr || view.titleBar != nullptr) {
        return;
    }
    const ViewId id = view.id;
    DockTitleBar* bar = chrome_->install(view.dock, kindIcon(state->kind), DockRole::View);
    view.titleBar = bar;
    bar->onPressed = [this, id] { activate(id); };

    // The kind switcher stands where the icon was: what a view shows is the
    // first thing about it, and the icon already said it.
    QToolButton* kind = makeTitleBarButton(
        bar, kindIcon(state->kind), "ViewKindButton", "What This View Shows",
        "Plan, 3D, section or elevation. The view keeps its zoom and the layers it hides for "
        "when it comes back.");
    kind->setPopupMode(QToolButton::InstantPopup);
    // Room for the menu arrow beside the icon, which says this one opens a
    // menu where its neighbours act at once.
    kind->setFixedWidth(34);
    auto* menu = new QMenu(kind);
    auto* group = new QActionGroup(menu);
    for (const ViewKind choice : kKinds) {
        QAction* action = menu->addAction(icon(kindIcon(choice)), katana::cad::toString(choice));
        action->setCheckable(true);
        action->setData(static_cast<int>(choice));
        group->addAction(action);
        // setViewKind and zoomExtents(id) below fail only for an id that is
        // not open, and this menu and that button are deleted with the view's
        // dock: there is no failure here to report.
        connect(action, &QAction::triggered, this, [this, id, choice] {
            (void)setViewKind(id, choice);
            activate(id);
        });
    }
    connect(menu, &QMenu::aboutToShow, this, [this, id, menu] {
        const ViewState* current = views_.find(id);
        for (QAction* action : menu->actions()) {
            action->setChecked(current != nullptr &&
                               action->data().toInt() == static_cast<int>(current->kind));
        }
    });
    kind->setMenu(menu);
    bar->setLeadingWidget(kind);
    view.kindButton = kind;

    QToolButton* layers = makeTitleBarButton(bar, Icon::Layers, "ViewLayersButton", "Layers", {});
    connect(layers, &QToolButton::clicked, this, [this, id] { (void)showLayersPopup(id); });
    bar->addTool(layers);
    view.layersButton = layers;

    QToolButton* extents = makeTitleBarButton(
        bar, Icon::ZoomExtents, "ViewZoomExtentsButton", "Zoom Extents",
        "Frame everything this view shows. View > Zoom Extents does the same for the active "
        "view.");
    connect(extents, &QToolButton::clicked, this, [this, id] { (void)zoomExtents(id); });
    bar->addTool(extents);

    // Pressing a view's own tools, or its Float or Maximise, is working in
    // that view as a press on its title is (onPressed above) - but a button
    // takes the press, so the bar never sees it. The menus act on the active
    // view, and a view maximised while another stayed active hid the very
    // view they act on. On `pressed`, not `clicked`, so that the view is
    // active before the button acts. Not Minimise or Close: they put it away.
    for (QToolButton* button :
         {kind, layers, extents, bar->floatButton(), bar->maximiseButton()}) {
        if (button != nullptr) {
            connect(button, &QToolButton::pressed, this, [this, id] { activate(id); });
        }
    }
    // From the chrome rather than the Minimise button's click, so that a view
    // minimised any other way (by name, after a saved layout) hands on too.
    // The dock is already hidden when this runs.
    bar->onMinimised = [this, id] { activateShowingInsteadOf(id); };
    // Back from the tray while the active view is still hidden - every view
    // was minimised, the active one among them: the view the user has just
    // brought back is the one the menus should act on.
    bar->onRestored = [this, id] {
        const View* active = find(views_.activeId());
        if (active == nullptr || active->dock->isHidden()) {
            activate(id);
        }
    };

    updateLayersButton(view);
}

bool ViewWorkspace::onScreen(const View& view) const
{
    // A tab page behind the current one is not hidden: Qt moves it off the
    // window instead (DockChrome::snapshot relies on the same). So "not
    // hidden" is not "showing", and only a docked view within this window's
    // rectangle, or a floating one, is where the user can see it.
    return !view.dock->isHidden() &&
           (view.dock->isFloating() || rect().intersects(view.dock->geometry()));
}

void ViewWorkspace::activateShowingInsteadOf(ViewId hidden)
{
    // Leaving the active view hidden would leave the menus acting on a view
    // nobody can see, and no title bar marked on screen. A view the user can
    // see takes over: the first in the order they were opened. The most
    // recently active would be the better choice, but ViewSet keeps that
    // order to itself (it uses it when the active view is REMOVED).
    if (views_.activeId() != hidden) {
        return;
    }
    for (const View& view : docks_) {
        if (view.id != hidden && onScreen(view)) {
            activate(view.id);
            return;
        }
    }
    // None where the user can see it, by geometry. Not the tab page that
    // takes over from a hidden current page: Qt lays the group out again as
    // the page is hidden (measured on Qt 6.11.2 - the new current page is
    // already inside the window here), so the loop above finds it. What is
    // left is a window never laid out (not yet shown, or headless), where no
    // geometry means anything. Raising the view makes it the current page of
    // any tab group it is in, so the view made active is the one that will
    // show.
    for (const View& view : docks_) {
        if (view.id != hidden && !view.dock->isHidden()) {
            activate(view.id);
            view.dock->raise();
            return;
        }
    }
}

void ViewWorkspace::updateActiveMarks()
{
    const ViewId active = views_.activeId();
    for (const View& view : docks_) {
        if (view.titleBar != nullptr) {
            view.titleBar->setActive(view.id == active);
        }
    }
}

std::size_t ViewWorkspace::hiddenCount(ViewId id) const
{
    const ViewState* state = views_.find(id);
    if (state == nullptr) {
        return 0;
    }
    std::size_t references = 0;
    if (reference_ != nullptr) {
        // Only ids that still name a layer: a removed image's id stays in the
        // view's set (ids are never reused, so it is harmless there) and must
        // not make the view look filtered.
        for (const auto& raster : reference_->rasters()) {
            references += state->hiddenReferences.contains(raster.id) ? 1 : 0;
        }
        for (const auto& cloud : reference_->pointClouds()) {
            references += state->hiddenReferences.contains(cloud.id) ? 1 : 0;
        }
    }
    return state->layers.size() + references;
}

void ViewWorkspace::updateLayersButton(const View& view)
{
    if (view.layersButton == nullptr) {
        return;
    }
    const ViewState* state = views_.find(view.id);
    const std::size_t hidden = hiddenCount(view.id);
    const QString title =
        state != nullptr ? QString::fromStdString(katana::cad::ViewSet::title(*state)) : "this view";
    // A filtered view must never be mistaken for missing data, so the button
    // changes its glyph and its frame, not only its tooltip.
    view.layersButton->setIcon(icon(hidden > 0 ? Icon::ViewLayersFiltered : Icon::Layers));
    view.layersButton->setProperty("filtered", hidden > 0);
    view.layersButton->style()->unpolish(view.layersButton);
    view.layersButton->style()->polish(view.layersButton);
    // "Of its own": the count is the view's own entries, and hiding a parent
    // is one entry however many layers lie beneath it.
    const QString tip =
        hidden > 0
            ? QString("%1 hides %2 layer(s) or reference layer(s) of its own, with what lies "
                      "beneath them. Choose which layers it shows; the Layers panel hides a "
                      "layer in every view.")
                  .arg(title)
                  .arg(hidden)
            : QString("Choose which layers %1 shows. It shows everything the drawing shows; the "
                      "Layers panel hides a layer in every view.")
                  .arg(title);
    view.layersButton->setToolTip(QString("<b>Layers in this view</b><br>%1").arg(tip));
    view.layersButton->setAccessibleName("Layers in this view");
    view.layersButton->setAccessibleDescription(tip);
}

ViewLayersPopup* ViewWorkspace::showLayersPopup(ViewId id)
{
    View* view = find(id);
    if (view == nullptr) {
        return nullptr;
    }
    auto* popup = new ViewLayersPopup(*this, id, view->dock);
    popup->setAttribute(Qt::WA_DeleteOnClose);
    popup->popup(view->layersButton != nullptr ? static_cast<QWidget*>(view->layersButton)
                                               : static_cast<QWidget*>(view->dock));
    return popup;
}

ViewState& ViewWorkspace::openView(ViewKind kind, bool activateIt)
{
    if (chrome_ != nullptr) {
        // A split made while the other views were hidden would be made
        // against the wrong neighbours.
        chrome_->unmaximiseDocked(*this);
    }
    ViewState& state = views_.add(kind);
    const ViewId previous = views_.activeId();

    View view;
    view.id = state.id;
    view.dock = new ViewDock(state.id, this);
    view.dock->onCloseRequested = [this, id = state.id] { (void)closeView(id); };
    docks_.push_back(view);
    View& added = docks_.back();
    buildContent(added, state);
    installChrome(added);

    // Splitting the active view when it is docked here, so a new view shares
    // the space the user was looking at rather than squeezing in at an edge.
    // !isHidden rather than isVisible: before the window is first shown no
    // dock is visible, and a headless import that opens a 3D view must still
    // split the plan rather than stack beside it.
    const View* beside = previous != state.id ? find(previous) : nullptr;
    if (beside != nullptr && !beside->dock->isFloating() && !beside->dock->isHidden()) {
        // Along the longer side, so that both halves keep a usable shape.
        // Before the first layout a dock's size means nothing: side by side.
        const QSize size = beside->dock->size();
        const Qt::Orientation direction =
            !beside->dock->isVisible() || size.width() >= size.height() ? Qt::Horizontal
                                                                        : Qt::Vertical;
        splitDockWidget(beside->dock, added.dock, direction);
        // Shown now, as arrange() does after each split. Qt shows a dock it
        // has just laid out only from a queued call, and resizeDocks leaves a
        // hidden dock out of the total it gives the enclosing row: in a split
        // nested inside another (a stacked split of a view already side by
        // side) the new halves came out 397 and 197 px of 600, not 297 each.
        added.dock->show();
        // Equal halves of the space the active view had. Qt gives the new
        // dock its size hint otherwise, and what the user sees is a sliver.
        // Sizes in pixels, not a ratio: the other docks of the same row keep
        // theirs only if the two here add up to exactly what `beside` had.
        const int separator = style()->pixelMetric(QStyle::PM_DockWidgetSeparatorExtent, nullptr, this);
        const int extent = direction == Qt::Horizontal ? size.width() : size.height();
        const int first = std::max(1, (extent - separator) / 2);
        const int second = std::max(1, extent - separator - first);
        resizeDocks({beside->dock, added.dock}, {first, second}, direction);
    } else {
        addDockWidget(kViewArea, added.dock);
    }

    // Opened while the active view is hidden - every view was minimised - the
    // new view is the one the user can see, so the menus act on it even when
    // the caller (ensureView, arrange) did not ask for it.
    const bool activeHidden = beside != nullptr && beside->dock->isHidden();
    if (activateIt || activeHidden) {
        activate(state.id);
    }
    updateActiveMarks();
    if (onViewsChanged) {
        onViewsChanged();
    }
    return state;
}

Status ViewWorkspace::closeView(ViewId id)
{
    const auto at = std::ranges::find_if(docks_, [&](const View& view) { return view.id == id; });
    if (at == docks_.end()) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    ViewDock* dock = at->dock;
    docks_.erase(at);
    const ViewId wasActive = views_.activeId();
    // Before it goes: closing the maximised view brings back the views it
    // hid, and a minimised one's tray button goes with it.
    if (chrome_ != nullptr) {
        chrome_->forget(dock);
    }
    // The dock and its widget go first: the widget holds a reference to the
    // state, which must outlive it.
    removeDockWidget(dock);
    delete dock;
    (void)views_.remove(id);
    if (wasActive == id) {
        // ViewSet hands on to the view used most recently, which knows
        // nothing of docks: it may be minimised, or a tab page parked behind
        // another. A minimised one hands on again to a view that shows; a tab
        // page is raised, which makes it the current page of its group.
        if (const View* now = find(views_.activeId())) {
            if (now->dock->isHidden()) {
                activateShowingInsteadOf(now->id);
            } else if (!now->dock->isFloating()) {
                now->dock->raise();
            }
        }
    }
    updateActiveMarks();
    if (wasActive == id && onActiveChanged) {
        onActiveChanged();
    }
    if (onViewsChanged) {
        onViewsChanged();
    }
    return {};
}

Status ViewWorkspace::setViewKind(ViewId id, ViewKind kind)
{
    View* view = find(id);
    ViewState* state = views_.find(id);
    if (view == nullptr || state == nullptr) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    if (state->kind == kind) {
        return {};
    }
    if (auto status = views_.setKind(id, kind); !status) {
        return status;
    }
    buildContent(*view, *state);
    if (onActiveChanged) {
        onActiveChanged();
    }
    if (onViewsChanged) {
        onViewsChanged();
    }
    return {};
}

Status ViewWorkspace::activateView(ViewId id)
{
    if (find(id) == nullptr) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    activate(id);
    return {};
}

void ViewWorkspace::activate(ViewId id)
{
    if (views_.activeId() == id) {
        return;
    }
    if (!views_.activate(id)) {
        return;
    }
    updateActiveMarks();
    if (onActiveChanged) {
        onActiveChanged();
    }
}

ViewState& ViewWorkspace::ensureView(ViewKind kind)
{
    if (ViewState* state = views_.mostRecent(kind)) {
        if (const View* view = find(state->id)) {
            // Through the tray when it is there, so that it comes back at the
            // size it had and its button goes.
            if (chrome_ == nullptr || !chrome_->restore(view->dock)) {
                view->dock->show();
                view->dock->raise();
            }
        }
        return *state;
    }
    return openView(kind, false);
}

void ViewWorkspace::arrange(katana::cad::LayoutKind kind)
{
    if (chrome_ != nullptr) {
        chrome_->unmaximiseDocked(*this);
    }
    const std::size_t places = katana::cad::cellCount(kind);
    std::vector<ViewId> ids;
    for (const View& view : docks_) {
        if (!view.dock->isFloating()) {
            ids.push_back(view.id);
        }
    }
    // Open the kinds the arrangement expects and the user does not have yet,
    // in the preset's order: plan, 3D, section, elevation.
    for (std::size_t slot = 0; ids.size() < places && slot < 4; ++slot) {
        const ViewKind wanted = katana::cad::defaultViewKind(slot);
        const bool present = std::ranges::any_of(ids, [&](ViewId id) {
            const ViewState* state = views_.find(id);
            return state != nullptr && state->kind == wanted;
        });
        if (!present) {
            ids.push_back(openView(wanted, false).id);
        }
    }
    while (ids.size() < places) {
        ids.push_back(openView(katana::cad::defaultViewKind(ids.size()), false).id);
    }

    std::vector<ViewDock*> docks;
    for (const ViewId id : ids) {
        docks.push_back(find(id)->dock);
    }
    for (ViewDock* dock : docks) {
        removeDockWidget(dock);
    }
    addDockWidget(kViewArea, docks[0]);
    docks[0]->show();
    const std::vector<katana::cad::DockSplit> steps = katana::cad::dockSplits(kind);
    for (const katana::cad::DockSplit& step : steps) {
        splitDockWidget(docks[step.existing], docks[step.added],
                        step.sideBySide ? Qt::Horizontal : Qt::Vertical);
        docks[step.added]->show();
    }
    for (std::size_t i = places; i < docks.size(); ++i) {
        tabifyDockWidget(docks[places - 1], docks[i]);
        docks[i]->show();
    }
    // Equal halves at every split, as the preset's picture shows them.
    for (const katana::cad::DockSplit& step : steps) {
        resizeDocks({docks[step.existing], docks[step.added]}, {1000, 1000},
                    step.sideBySide ? Qt::Horizontal : Qt::Vertical);
    }
    if (onViewsChanged) {
        onViewsChanged();
    }
}

ViewDock* ViewWorkspace::dockFor(ViewId id) const
{
    const View* view = find(id);
    return view != nullptr ? view->dock : nullptr;
}

QWidget* ViewWorkspace::widgetFor(ViewId id) const
{
    const View* view = find(id);
    return view != nullptr ? view->widget() : nullptr;
}

ViewportWidget* ViewWorkspace::planView(ViewId id) const
{
    const View* view = find(id);
    return view != nullptr ? view->plan : nullptr;
}

RenderViewWidget* ViewWorkspace::renderView(ViewId id) const
{
    const View* view = find(id);
    return view != nullptr ? view->render : nullptr;
}

SectionViewWidget* ViewWorkspace::sectionView(ViewId id) const
{
    const View* view = find(id);
    return view != nullptr ? view->section : nullptr;
}

ViewKind ViewWorkspace::activeViewKind()
{
    const ViewState* state = views_.active();
    return state != nullptr ? state->kind : ViewKind::Plan;
}

ViewportWidget* ViewWorkspace::activePlanView()
{
    const ViewState* state = views_.mostRecent(ViewKind::Plan);
    return state != nullptr ? planView(state->id) : nullptr;
}

RenderViewWidget* ViewWorkspace::activeRenderView()
{
    if (const ViewState* active = views_.active(); active != nullptr && isRenderKind(active->kind)) {
        return renderView(active->id);
    }
    const ViewState* state = views_.mostRecent(ViewKind::Model3D);
    if (state == nullptr) {
        state = views_.mostRecent(ViewKind::Elevation);
    }
    return state != nullptr ? renderView(state->id) : nullptr;
}

SectionViewWidget* ViewWorkspace::activeSectionView()
{
    const ViewState* state = views_.mostRecent(ViewKind::Section);
    return state != nullptr ? sectionView(state->id) : nullptr;
}

std::vector<ViewportWidget*> ViewWorkspace::planViews() const
{
    std::vector<ViewportWidget*> out;
    for (const View& view : docks_) {
        if (view.plan != nullptr) {
            out.push_back(view.plan);
        }
    }
    return out;
}

void ViewWorkspace::setReferenceData(katana::interop::ReferenceData* reference)
{
    reference_ = reference;
    for (ViewportWidget* plan : planViews()) {
        plan->setReferenceData(reference);
    }
    for (const View& view : docks_) {
        updateLayersButton(view);
    }
}

void ViewWorkspace::setSurfaces(const std::vector<katana::cad::SceneSurface>* surfaces)
{
    surfaces_ = surfaces;
    for (View& view : docks_) {
        if (view.render != nullptr) {
            view.render->setContext(contextFor());
        }
    }
}

void ViewWorkspace::setMeshes(const std::vector<katana::cad::SceneMesh>* meshes)
{
    meshes_ = meshes;
    for (View& view : docks_) {
        if (view.render != nullptr) {
            view.render->setContext(contextFor());
        }
        // A plan view draws each mesh's footprint, so it needs them too.
        if (view.plan != nullptr) {
            view.plan->setMeshes(meshes);
        }
    }
}

void ViewWorkspace::setSceneOptions(const katana::cad::SceneOptions& options)
{
    options_ = options;
    // Never carries a view's layers: those are set per widget from its state.
    options_.layers = nullptr;
    for (View& view : docks_) {
        if (view.render != nullptr) {
            view.render->setContext(contextFor());
        }
    }
}

bool ViewWorkspace::showSection(katana::cad::Section section)
{
    ViewState& state = ensureView(ViewKind::Section);
    SectionViewWidget* view = sectionView(state.id);
    if (view == nullptr) {
        return false;
    }
    view->setSection(std::move(section));
    return true;
}

void ViewWorkspace::wireTools(ViewportWidget& plan, ViewId id)
{
    plan.onActiveToolChanged = [this](const std::string& toolId) {
        if (!toolId.empty()) {
            lastToolId_ = toolId;
        }
        if (onActiveToolChanged) {
            onActiveToolChanged(toolId);
        }
    };
    // Through startTool, not the view's own: a view starting a tool itself
    // would leave one running in another view, and then the workspace's
    // activeToolId and typeIntoTool would find whichever view comes first.
    // The view Enter was pressed in becomes the active one, so it is where
    // the tool runs.
    plan.onRepeatTool = [this, id] {
        if (lastToolId_.empty()) {
            return;
        }
        activate(id);
        const std::string toolId = lastToolId_; // startTool's hooks rewrite it
        if (const Status started = startTool(toolId); !started && onError) {
            onError(QString::fromStdString(started.error().describe()));
        }
    };
    plan.onToolMessage = [this](const QString& message) {
        if (onToolMessage) {
            onToolMessage(message);
        } else if (onStatus) {
            onStatus(message);
        }
    };
    plan.onTextTyped = [this](const QString& text) {
        if (onTextTyped) {
            onTextTyped(text);
        }
    };
}

Status ViewWorkspace::startTool(std::string_view id)
{
    ViewportWidget* target = activePlanView();
    if (target == nullptr) {
        return makeError(ErrorCode::InvalidState, "there is no plan view to draw in",
                         std::string(id));
    }
    if (katana::cad::toolCatalog().find(id) == nullptr) {
        return makeError(ErrorCode::NotFound, "there is no tool '" + std::string(id) + "'");
    }
    // One tool at a time in the workspace: a second running in another view
    // would hold picks the user can no longer see is pending.
    for (ViewportWidget* plan : planViews()) {
        if (plan != target && plan->toolActive()) {
            plan->setTool(Tool::Select);
        }
    }
    return target->startTool(id);
}

std::string ViewWorkspace::activeToolId() const
{
    for (ViewportWidget* plan : planViews()) {
        if (plan->toolActive()) {
            return plan->activeToolId();
        }
    }
    return {};
}

bool ViewWorkspace::typeIntoTool(const QString& text)
{
    for (ViewportWidget* plan : planViews()) {
        if (plan->toolActive()) {
            return plan->typeIntoTool(text);
        }
    }
    return false;
}

void ViewWorkspace::stopTool()
{
    for (ViewportWidget* plan : planViews()) {
        if (plan->toolActive()) {
            plan->setTool(Tool::Select);
        }
    }
}

bool ViewWorkspace::pressEnter()
{
    ViewportWidget* target = nullptr;
    for (ViewportWidget* plan : planViews()) {
        if (plan->toolActive()) {
            target = plan;
            break;
        }
    }
    if (target == nullptr) {
        target = activePlanView();
    }
    if (target == nullptr) {
        return false;
    }
    // Delivered as the key itself, so the command line's Enter and the
    // drawing's are one path and cannot come to mean different things.
    QKeyEvent press(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
    QCoreApplication::sendEvent(target, &press);
    return true;
}

void ViewWorkspace::setGridVisible(bool visible)
{
    gridVisible_ = visible;
    options_.drawGrid = visible;
    for (View& view : docks_) {
        if (view.plan != nullptr) {
            view.plan->setGridVisible(visible);
        }
        if (view.render != nullptr) {
            view.render->setContext(contextFor());
        }
    }
}

void ViewWorkspace::setSnapEnabled(bool enabled)
{
    snapEnabled_ = enabled;
    for (ViewportWidget* plan : planViews()) {
        plan->setSnapEnabled(enabled);
    }
}

void ViewWorkspace::setSnapModes(katana::cad::SnapModes modes)
{
    snapModes_ = modes;
    for (ViewportWidget* plan : planViews()) {
        plan->setSnapModes(modes);
    }
}

void ViewWorkspace::cancel()
{
    // Esc ends a running tool (or takes back what was typed for it) before
    // anything else, and only the views busy with one hear it: another view
    // clearing the selection at the same key press would take away the
    // selection the tool was started on, where AutoCAD's second Esc does.
    std::vector<ViewportWidget*> busy;
    for (ViewportWidget* plan : planViews()) {
        if (plan->toolActive() || !plan->typedInput().isEmpty()) {
            busy.push_back(plan);
        }
    }
    for (ViewportWidget* plan : busy.empty() ? planViews() : busy) {
        plan->cancel();
    }
}

void ViewWorkspace::resetInteraction()
{
    for (ViewportWidget* plan : planViews()) {
        plan->resetInteraction();
    }
}

void ViewWorkspace::zoomExtents()
{
    if (const ViewState* state = views_.active()) {
        (void)zoomExtents(state->id);
    }
}

Status ViewWorkspace::zoomExtents(ViewId id)
{
    const View* view = find(id);
    if (view == nullptr) {
        return makeError(ErrorCode::NotFound, "no such view", std::to_string(id));
    }
    if (view->plan != nullptr) {
        view->plan->zoomExtents();
    } else if (view->render != nullptr) {
        view->render->zoomExtents();
    } else if (view->section != nullptr) {
        view->section->zoomExtents();
    }
    return {};
}

void ViewWorkspace::zoomExtentsAll()
{
    for (const View& view : docks_) {
        (void)zoomExtents(view.id);
    }
}

void ViewWorkspace::zoomTo(const katana::geometry::Box2& bounds)
{
    for (ViewportWidget* plan : planViews()) {
        plan->zoomTo(bounds);
    }
}

void ViewWorkspace::invalidateReferenceCache()
{
    for (View& view : docks_) {
        if (view.plan != nullptr) {
            view.plan->invalidateReferenceCache();
        }
        if (view.render != nullptr) {
            view.render->invalidateScene();
        }
        // Called whenever a reference layer is added or removed, which is
        // what changes how many of a view's hidden references still exist.
        updateLayersButton(view);
    }
}

void ViewWorkspace::refreshAll()
{
    for (View& view : docks_) {
        if (view.render != nullptr) {
            view.render->invalidateScene();
        } else if (QWidget* widget = view.widget()) {
            widget->update();
        }
    }
}

void ViewWorkspace::repaintViews()
{
    for (View& view : docks_) {
        if (QWidget* widget = view.widget()) {
            widget->update();
        }
    }
}

void ViewWorkspace::viewLayersChanged(ViewId id)
{
    View* view = find(id);
    if (view == nullptr) {
        return;
    }
    if (view->render != nullptr) {
        view->render->invalidateScene();
    } else if (view->plan != nullptr) {
        view->plan->invalidateReferenceCache();
        view->plan->update();
    } else if (view->section != nullptr) {
        view->section->update();
    }
    updateLayersButton(*view);
}

void ViewWorkspace::pruneViewLayers()
{
    for (ViewState* state : views_.views()) {
        if (state->layers.pruneMissing(document_.model().layers) > 0) {
            viewLayersChanged(state->id);
        }
    }
}

} // namespace katana::qt
