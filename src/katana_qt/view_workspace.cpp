#include "view_workspace.hpp"

#include <algorithm>

#include <QCloseEvent>
#include <QTimer>

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
    setDockOptions(QMainWindow::AnimatedDocks | QMainWindow::AllowNestedDocks |
                   QMainWindow::AllowTabbedDocks | QMainWindow::GroupedDragging);
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
        plan->setTool(tool_);
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
        plan->onToolChanged = [this](Tool tool) {
            // A view that dropped back to Select on its own (a right-click
            // with nothing picked) takes every other plan view with it, so the
            // views never disagree about the tool the toolbar shows.
            tool_ = tool;
            for (ViewportWidget* other : planViews()) {
                if (other->tool() != tool) {
                    other->setTool(tool);
                }
            }
            if (onToolChanged) {
                onToolChanged(tool);
            }
        };
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
    if (const ViewState* state = views_.find(view.id)) {
        view.dock->setWindowTitle(QString::fromStdString(katana::cad::ViewSet::title(*state)));
    }
}

ViewState& ViewWorkspace::openView(ViewKind kind, bool activateIt)
{
    ViewState& state = views_.add(kind);
    const ViewId previous = views_.activeId();

    View view;
    view.id = state.id;
    view.dock = new ViewDock(state.id, this);
    view.dock->onCloseRequested = [this, id = state.id] { (void)closeView(id); };
    docks_.push_back(view);
    View& added = docks_.back();
    buildContent(added, state);

    // Beside the active view when it is docked here, so a new view shares the
    // space the user was looking at rather than squeezing in at an edge.
    const View* beside = previous != state.id ? find(previous) : nullptr;
    if (beside != nullptr && !beside->dock->isFloating() && beside->dock->isVisible()) {
        splitDockWidget(beside->dock, added.dock, Qt::Horizontal);
    } else {
        addDockWidget(kViewArea, added.dock);
    }

    if (activateIt) {
        activate(state.id);
    }
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
    // The dock and its widget go first: the widget holds a reference to the
    // state, which must outlive it.
    removeDockWidget(dock);
    delete dock;
    (void)views_.remove(id);
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
    if (onActiveChanged) {
        onActiveChanged();
    }
}

ViewState& ViewWorkspace::ensureView(ViewKind kind)
{
    if (ViewState* state = views_.mostRecent(kind)) {
        if (const View* view = find(state->id)) {
            view->dock->show();
            view->dock->raise();
        }
        return *state;
    }
    return openView(kind, false);
}

void ViewWorkspace::arrange(katana::cad::LayoutKind kind)
{
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

void ViewWorkspace::setTool(Tool tool)
{
    tool_ = tool;
    for (ViewportWidget* plan : planViews()) {
        plan->setTool(tool);
    }
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
    for (ViewportWidget* plan : planViews()) {
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
    const ViewState* state = views_.active();
    const View* view = state != nullptr ? find(state->id) : nullptr;
    if (view == nullptr) {
        return;
    }
    if (view->plan != nullptr) {
        view->plan->zoomExtents();
    } else if (view->render != nullptr) {
        view->render->zoomExtents();
    } else if (view->section != nullptr) {
        view->section->zoomExtents();
    }
}

void ViewWorkspace::zoomExtentsAll()
{
    for (View& view : docks_) {
        if (view.plan != nullptr) {
            view.plan->zoomExtents();
        } else if (view.render != nullptr) {
            view.render->zoomExtents();
        } else if (view.section != nullptr) {
            view.section->zoomExtents();
        }
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
