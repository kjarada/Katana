#include "viewport_widget.hpp"

#include "theme.hpp"
#include "view_focus.hpp"

#include "katana/cad/spatial_query.hpp"
#include "katana/cad/dashing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/cad/hatching.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/entity/display.hpp"

#include <algorithm>
#include <optional>
#include <charconv>
#include <cmath>
#include <cstdio>
#include <limits>

#include <QPdfWriter>
#include <QPageSize>
#include <QLineF>
#include <QMarginsF>
#include <QContextMenuEvent>
#include <QCursor>
#include <QFont>
#include <QFontMetrics>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPolygonF>
#include <QWheelEvent>

#include "katana/cad/selection.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/cad/style_resolver.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace cmd = katana::commands;
using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

namespace {

const QColor kBackground(0x1e, 0x23, 0x29);
const QColor kGridMinor(0x2a, 0x31, 0x39);
const QColor kGridMajor(0x38, 0x42, 0x4d);
const QColor kAxis(0x5a, 0x68, 0x75);
const QColor kSelection(0xff, 0x9f, 0x1c);
const QColor kPreview(0x4c, 0xc9, 0xf0);
const QColor kSnapMarker(0xf7, 0xd0, 0x3c);

constexpr double kPickAperturePixels = 8.0;
constexpr double kSnapAperturePixels = 12.0;
constexpr double kPointMarkerPixels = 4.0;
constexpr double kWheelZoomStep = 1.2;
// Drags shorter than this are clicks, not selection boxes.
constexpr double kDragThresholdPixels = 4.0;
// The margin Zoom Extents leaves round the drawing, as a fraction of the view
// on each side: enough that a line along the edge of the drawing is not lost
// under the edge of the view.
constexpr double kFrameMargin = 0.08;
// The zoom of a view that has never been framed, in pixels per model unit:
// ten pixels a metre shows an empty drawing as a few tens of metres of grid,
// a sensible first look for a site drawn in metres.
constexpr double kInitialScale = 10.0;

// The smallest a plan view may be made. It used to be 480 x 320, which
// stopped a dock being shrunk below that and made its neighbours overlap it
// (the UX audit of 2026-09-23). Measured in the headless screenshot's
// 1360 x 860 window with the Layers and Properties panels open: the whole
// central area is 642 x 594 pixels, so 480 x 320 would not let two views sit
// side by side at all, and each view of a 2 x 2 arrangement gets about
// 321 x 273 once its title bar is taken off - less on a 1366 x 768 laptop.
// 160 x 120 fits that quarter with room to spare and still leaves a usable
// drawing: at the grid's minimum spacing (cad::gridSpacing, 12 px) it is 13
// by 10 grid cells, twenty pick apertures across, and room for a snap marker
// and its label.
constexpr int kMinimumWidth = 160;
constexpr int kMinimumHeight = 120;

QColor toQColor(const katana::entity::Color& color)
{
    return QColor(color.r, color.g, color.b, color.a);
}

// Whether a mesh is drawn at all - in plan as its footprint, and so counted
// in what Zoom Extents frames. The 3D scene applies the same rule to it.
bool isShown(const katana::cad::SceneMesh& item)
{
    return item.visible && item.mesh != nullptr && !item.mesh->empty() &&
           item.style != katana::cad::SurfaceStyle::Hidden;
}

} // namespace

const char* toString(Tool tool)
{
    switch (tool) {
    case Tool::Select:
        return "Select";
    case Tool::Point:
        return "Point";
    case Tool::Line:
        return "Line";
    case Tool::Polyline:
        return "Polyline";
    case Tool::Rectangle:
        return "Rectangle";
    case Tool::Circle:
        return "Circle";
    case Tool::Arc:
        return "Arc";
    case Tool::Move:
        return "Move";
    case Tool::Copy:
        return "Copy";
    }
    return "Unknown";
}

const char* toolId(Tool tool)
{
    // The catalogue tools that do what the eight did: Arc is the three-point
    // arc, as the old tool drew, and Circle is centre and radius.
    switch (tool) {
    case Tool::Select:
        return "";
    case Tool::Point:
        return "draw.point";
    case Tool::Line:
        return "draw.line";
    case Tool::Polyline:
        return "draw.polyline";
    case Tool::Rectangle:
        return "draw.rectangle";
    case Tool::Circle:
        return "draw.circle";
    case Tool::Arc:
        return "draw.arc";
    case Tool::Move:
        return "modify.move";
    case Tool::Copy:
        return "modify.copy";
    }
    return "";
}

std::optional<Tool> legacyTool(std::string_view id)
{
    for (const Tool tool : {Tool::Select, Tool::Point, Tool::Line, Tool::Polyline, Tool::Rectangle,
                            Tool::Circle, Tool::Arc, Tool::Move, Tool::Copy}) {
        if (id == toolId(tool)) {
            return tool;
        }
    }
    return std::nullopt;
}

ViewportWidget::ViewportWidget(cad::Document& document, cad::ViewState& state, QWidget* parent)
    : QWidget(parent), document_(document), state_(state), tools_(document)
{
    setMinimumSize(kMinimumWidth, kMinimumHeight);
    setMouseTracking(true);
    setFocusPolicy(Qt::StrongFocus);
    setCursor(Qt::CrossCursor);
    // Only a view that has never been framed: one rebuilt after a change of
    // kind keeps the pan and zoom it had.
    if (!state_.planFramed) {
        state_.plan.scale = kInitialScale;
    }
    documentListener_ = document_.addListener([this] { update(); });
    wireToolHost();
    activateOnFocus(*this, [this] {
        if (onActivated) {
            onActivated();
        }
    });
}

QPointF ViewportWidget::toScreen(const Point2& world) const
{
    const Point2 p = state_.plan.worldToScreen(world);
    return QPointF(p.x, p.y);
}

ViewportWidget::Point2 ViewportWidget::toWorld(const QPointF& screen) const
{
    return state_.plan.screenToWorld(Point2(screen.x(), screen.y()));
}

// ---- public controls --------------------------------------------------------------------

void ViewportWidget::setTool(Tool tool)
{
    if (tool == Tool::Select) {
        typed_.clear();
        boxStart_.reset();
        if (tools_.active()) {
            tools_.cancel(); // its onFinished reports the change
        } else if (onToolChanged) {
            // Reported even when nothing was running, as it always was: the
            // window checks its Select button from this at start-up.
            onToolChanged(Tool::Select);
        }
        updatePrompt();
        update();
        return;
    }
    // Every id in toolId is in the catalogue (a test asserts it), so this
    // cannot fail short of a catalogue that lost a tool, and then says so.
    const auto started = startTool(toolId(tool));
    if (!started && onError) {
        onError(QString::fromStdString(started.error().describe()));
    }
}

Tool ViewportWidget::tool() const { return legacyTool(tools_.activeId()).value_or(Tool::Select); }

katana::core::Status ViewportWidget::startTool(std::string_view id)
{
    boxStart_.reset();
    typed_.clear();
    tools_.setPickTolerance(pickTolerance());
    return tools_.start(id);
}

bool ViewportWidget::typeIntoTool(const QString& text)
{
    if (!tools_.active()) {
        return false;
    }
    typed_.clear();
    (void)tools_.typed(text.toStdString());
    update();
    return true;
}

double ViewportWidget::pickTolerance() const
{
    return state_.plan.pixelsToWorld(kPickAperturePixels);
}

void ViewportWidget::wireToolHost()
{
    tools_.onPrompt = [this](const std::string&) { updatePrompt(); };
    tools_.onMessage = [this](const std::string& message) {
        if (onToolMessage) {
            onToolMessage(QString::fromStdString(message));
        }
    };
    tools_.onRejected = [this](const std::string& message) {
        if (onError) {
            onError(QString::fromStdString(message));
        }
    };
    tools_.onStarted = [this](const std::string& id) {
        lastToolId_ = id;
        if (onActiveToolChanged) {
            onActiveToolChanged(id);
        }
        if (const auto legacy = legacyTool(id); legacy && onToolChanged) {
            onToolChanged(*legacy);
        }
        update();
    };
    tools_.onFinished = [this](const std::string&) {
        typed_.clear();
        if (onActiveToolChanged) {
            onActiveToolChanged({});
        }
        if (onToolChanged) {
            onToolChanged(Tool::Select);
        }
        update();
    };
}

Box2 ViewportWidget::drawnBounds() const
{
    // What THIS view draws, through the one visibility rule with this view's
    // hidden layers: the entities' own bounds counted every entity, so a
    // layer hidden here - or a hidden stray far away - still pulled the frame
    // out to it.
    Box2 bounds = cad::drawnExtent(document_.model(), state_.layers);
    // Zoom Extents means everything the user can see, so imported imagery and
    // point clouds count. A drawing that is empty except for an orthophoto
    // would otherwise fit an empty box and leave the photo off screen. Layer
    // by layer rather than ReferenceData::visibleBounds, because a layer
    // hidden in this view is not seen here.
    if (reference_ != nullptr) {
        for (const katana::interop::RasterOverlay& raster : reference_->rasters()) {
            if (raster.visible && !state_.hiddenReferences.contains(raster.id)) {
                bounds.expand(raster.worldBounds());
            }
        }
        for (const katana::interop::PointCloudLayer& cloud : reference_->pointClouds()) {
            if (cloud.visible && !state_.hiddenReferences.contains(cloud.id)) {
                bounds.expand(cloud.worldBounds());
            }
        }
    }
    // A mesh is drawn in plan as its footprint, so it is seen and counts. A
    // drawing that is nothing but meshes framed an empty box before.
    if (meshes_ != nullptr) {
        for (const katana::cad::SceneMesh& item : *meshes_) {
            if (isShown(item)) {
                const katana::math::AABB space = item.mesh->bounds();
                if (!space.empty()) {
                    bounds.expand(Point2(space.min.x, space.min.y));
                    bounds.expand(Point2(space.max.x, space.max.y));
                }
            }
        }
    }
    // Alignments are drawn but are not entities, so they are in no entity
    // bound. Found by looking at a screenshot: the sample's access road ran off
    // the bottom of a view that claimed to show everything.
    for (const katana::entity::Alignment& alignment : document_.model().alignments.all()) {
        if (const auto solved = katana::geometry::solveAlignment(alignment.horizontal)) {
            // A metre is fine enough for a bounding box; the curve cannot
            // stray further than that from its chords.
            for (const Point2& vertex : solved->toPolyline(1.0).vertices) {
                bounds.expand(vertex);
            }
        }
    }
    return bounds;
}

void ViewportWidget::zoomExtents()
{
    // An empty box is fitted too: ViewTransform::fit then goes back to the
    // origin at one pixel a unit, which is what Zoom Extents on nothing has
    // always done.
    frame(drawnBounds());
}

void ViewportWidget::zoomTo(const Box2& bounds)
{
    if (bounds.empty()) {
        return;
    }
    frame(bounds);
}

void ViewportWidget::frame(const Box2& bounds)
{
    state_.plan.resize(width(), height());
    state_.plan.fit(bounds, kFrameMargin);
    state_.planFramed = true;
    framedBox_ = bounds;
    update();
}

void ViewportWidget::frameOnFirstPaint()
{
    // At the first paint rather than the first resize, because a paint is at
    // the size the view is seen at - a dock's first resize can be a
    // provisional one from before the workspace laid it out.
    state_.planFramed = true;
    const Box2 bounds = drawnBounds();
    if (!bounds.empty()) {
        state_.plan.fit(bounds, kFrameMargin);
        framedBox_ = bounds;
    }
}

void ViewportWidget::setReferenceData(katana::interop::ReferenceData* reference)
{
    // Not named `data`: QWidget already has a member of that name, and -Wshadow
    // is an error here.
    reference_ = reference;
    invalidateReferenceCache();
}

void ViewportWidget::invalidateReferenceCache()
{
    rasterCache_.clear();
    cloudCache_.clear();
    update();
}

void ViewportWidget::setGridVisible(bool visible)
{
    gridVisible_ = visible;
    update();
}

void ViewportWidget::setSnapEnabled(bool enabled)
{
    snapEnabled_ = enabled;
    activeSnap_.reset();
    update();
}

void ViewportWidget::setSnapModes(cad::SnapModes modes)
{
    snapModes_ = modes;
}

void ViewportWidget::cancel()
{
    if (!typed_.isEmpty()) {
        // As in a command line: the first Esc takes back what was typed.
        typed_.clear();
    } else if (tools_.active()) {
        boxStart_.reset(); // a selection box the tool was being given
        tools_.cancel();   // its onFinished resets the prompt and repaints
        return;
    } else if (boxStart_) {
        boxStart_.reset();
    } else if (!document_.selection().empty()) {
        document_.selection().clear();
        document_.notifySelectionChanged();
    }
    updatePrompt();
    update();
}

void ViewportWidget::resetInteraction()
{
    typed_.clear();
    boxStart_.reset();
    activeSnap_.reset();
    tools_.reset();
    updatePrompt();
    update();
}

void ViewportWidget::updatePrompt()
{
    if (!onPrompt) {
        return;
    }
    if (tools_.active()) {
        onPrompt(QString("%1: %2").arg(QString::fromStdString(tools_.info()->name),
                                      QString::fromStdString(tools_.prompt())));
        return;
    }
    onPrompt("Select: click an entity, drag right for a window, drag left for crossing");
}

void ViewportWidget::run(cmd::CommandPtr command)
{
    const auto status = document_.execute(std::move(command));
    if (!status && onError) {
        onError(QString::fromStdString(status.error().describe()));
    }
}

// ---- interaction ------------------------------------------------------------------------

void ViewportWidget::updateCursor(const QPointF& screen)
{
    const Point2 raw = toWorld(screen);
    cursorWorld_ = raw;
    activeSnap_.reset();

    // Snapping serves a tool that wants a point: a pick of an entity or a
    // selection is made where the cursor really is.
    if (snapEnabled_ && tools_.expects() == cad::ToolInput::Point) {
        cad::SnapRequest request;
        request.cursor = raw;
        request.aperture = state_.plan.pixelsToWorld(kSnapAperturePixels);
        request.modes = snapModes_;
        request.from = tools_.lastPoint();
        request.gridSpacing = gridVisible_ ? cad::gridSpacing(state_.plan.scale) : 0.0;
        request.view = &state_.layers;
        // Through the Document's spatial index (PLAN.MD Phase 18). Measured in
        // Release on 100 000 entities: 4404 us per mouse move scanning,
        // 88 us indexed. The answer is identical either way - asserted by
        // IndexedQueries - so this is purely the cost.
        activeSnap_ = cad::snap(document_.model(), request, &document_.spatialIndex());
        if (activeSnap_) {
            cursorWorld_ = activeSnap_->point;
        }
    }
    if (onCursorMoved) {
        onCursorMoved(cursorWorld_, activeSnap_);
    }
}

std::optional<katana::entity::EntityId> ViewportWidget::entityAt(const QPointF& screen) const
{
    cad::SelectionFilter filter;
    filter.view = &state_.layers;
    return cad::pickEntity(document_.model(), toWorld(screen), pickTolerance(), filter,
                           &document_.spatialIndex());
}

void ViewportWidget::toolClick(const QPointF& screen)
{
    // The aperture at this zoom, for a tool that finds geometry near a point
    // itself; a tool reads it when it (re)starts.
    tools_.setPickTolerance(pickTolerance());
    switch (tools_.expects().value_or(cad::ToolInput::Point)) {
    case cad::ToolInput::Entity: {
        // Where the cursor really is, not a snapped point: the pick is of
        // what is under it. Nothing there is still a click, and the tool
        // either takes it as a point or says what it wants instead.
        const Point2 at = toWorld(screen);
        if (const auto picked = entityAt(screen)) {
            (void)tools_.entity(*picked, at);
        } else {
            (void)tools_.point(at);
        }
        break;
    }
    case cad::ToolInput::Point:
    case cad::ToolInput::Value:
        updateCursor(screen);
        (void)tools_.point(cursorWorld_);
        break;
    case cad::ToolInput::Selection:
        break; // selected at the release, as the Select tool does
    }
    update();
}

void ViewportWidget::toolEnter()
{
    if (!typed_.isEmpty()) {
        const std::string text = typed_.toStdString();
        typed_.clear();
        (void)tools_.typed(text);
    } else {
        (void)tools_.enter();
    }
    updatePrompt();
    update();
}

void ViewportWidget::selectAt(const QPointF& screen, Qt::KeyboardModifiers modifiers)
{
    cad::SelectionFilter filter;
    filter.view = &state_.layers;
    const auto picked = cad::pickEntity(document_.model(), toWorld(screen),
                                        state_.plan.pixelsToWorld(kPickAperturePixels), filter,
                                        &document_.spatialIndex());
    cad::SelectionSet& selection = document_.selection();
    if (modifiers & Qt::ControlModifier) {
        if (picked) {
            selection.toggle(*picked);
        }
    } else if (modifiers & Qt::ShiftModifier) {
        if (picked) {
            selection.add(*picked);
        }
    } else {
        selection.clear();
        if (picked) {
            selection.add(*picked);
        }
    }
    document_.notifySelectionChanged();
}

void ViewportWidget::selectInBox(const QPointF& from, const QPointF& to,
                                 Qt::KeyboardModifiers modifiers)
{
    Box2 box;
    box.expand(toWorld(from));
    box.expand(toWorld(to));
    // Dragging left to right selects what is fully inside; right to left also
    // takes whatever the box touches.
    const auto mode = to.x() >= from.x() ? cad::BoxSelectionMode::Window
                                         : cad::BoxSelectionMode::Crossing;
    cad::SelectionFilter filter;
    filter.view = &state_.layers;
    const auto picked = cad::pickInBox(document_.model(), box, mode, filter,
                                       &document_.spatialIndex());
    cad::SelectionSet& selection = document_.selection();
    if (!(modifiers & (Qt::ShiftModifier | Qt::ControlModifier))) {
        selection.clear();
    }
    for (const auto id : picked) {
        selection.add(id);
    }
    document_.notifySelectionChanged();
}

void ViewportWidget::mousePressEvent(QMouseEvent* event)
{
    if (onActivated) {
        onActivated();
    }
    setFocus();
    lastMouse_ = event->position();
    if (event->button() == Qt::MiddleButton) {
        panning_ = true;
        setCursor(Qt::ClosedHandCursor);
        return;
    }
    if (event->button() == Qt::RightButton) {
        if (tools_.active()) {
            // Enter, as in AutoCAD with its shortcut menu off: it finishes a
            // chain, ends a selection or takes the prompt's default.
            boxStart_.reset();
            toolEnter();
        } else if (!boxStart_ && onContextMenu) {
            // Nothing to finish or cancel, and cancel() here would only have
            // cleared the selection the menu is about to act on.
            onContextMenu(event->globalPosition().toPoint());
        } else {
            cancel();
        }
        return;
    }
    if (event->button() != Qt::LeftButton) {
        return;
    }
    // Selecting, with no tool or for a tool that asks for a selection: a
    // click or a box, settled at the release.
    if (!tools_.active() || tools_.expects() == cad::ToolInput::Selection) {
        boxStart_ = event->position();
        boxEnd_ = event->position();
        return;
    }
    toolClick(event->position());
}

void ViewportWidget::mouseMoveEvent(QMouseEvent* event)
{
    if (panning_) {
        const QPointF delta = event->position() - lastMouse_;
        state_.plan.panByPixels(delta.x(), delta.y());
        framedBox_.reset(); // the user's view now, kept on a resize
    } else if (boxStart_) {
        boxEnd_ = event->position();
    }
    lastMouse_ = event->position();
    updateCursor(event->position());
    update();
}

void ViewportWidget::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() == Qt::MiddleButton) {
        panning_ = false;
        setCursor(Qt::CrossCursor);
        return;
    }
    if (event->button() == Qt::LeftButton && boxStart_) {
        const QPointF from = *boxStart_;
        const QPointF to = event->position();
        boxStart_.reset();
        const bool dragged = std::abs(to.x() - from.x()) > kDragThresholdPixels ||
                             std::abs(to.y() - from.y()) > kDragThresholdPixels;
        Qt::KeyboardModifiers modifiers = event->modifiers();
        if (tools_.expects() == cad::ToolInput::Selection) {
            // A tool's "Select entities" GATHERS, as AutoCAD's does: each
            // click or box adds to what is picked, and Shift or Ctrl takes a
            // picked entity back out. Replacing the selection at every
            // plain click, as the Select tool does, would leave only the
            // last of several cutting edges picked.
            modifiers = (modifiers & (Qt::ShiftModifier | Qt::ControlModifier))
                            ? Qt::KeyboardModifiers(Qt::ControlModifier)
                            : Qt::KeyboardModifiers(Qt::ShiftModifier);
        }
        if (dragged) {
            selectInBox(from, to, modifiers);
        } else {
            selectAt(to, modifiers);
        }
        update();
    }
}

void ViewportWidget::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() == Qt::MiddleButton) {
        zoomExtents();
        return;
    }
    // Qt delivers a double click as press, release, DOUBLECLICK, release - the
    // SECOND press arrives here and never reaches mousePressEvent. Swallowing it
    // means a user clicking quickly while drawing silently loses that vertex, so
    // a drawing tool has to consume it exactly as it would an ordinary press.
    // Select is left alone: there a double click is not a second pick.
    if (tools_.active()) {
        mousePressEvent(event);
    }
}

void ViewportWidget::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches != 0.0) {
        state_.plan.zoomAt(Point2(event->position().x(), event->position().y()),
                           std::pow(kWheelZoomStep, notches));
        framedBox_.reset(); // the user's view now, kept on a resize
        updateCursor(event->position());
        update();
    }
    event->accept();
}

namespace {

// Text a key press types, as opposed to a shortcut: every character printable
// and no Ctrl or Alt held. Ctrl AND Alt together are let through, because
// that is how Windows reports AltGr, which types @, { and the euro sign on
// most European keyboards - refusing it would make a coordinate or a layer
// name untypable there.
bool isTypedText(const QKeyEvent& event)
{
    const QString text = event.text();
    if (text.isEmpty()) {
        return false;
    }
    const Qt::KeyboardModifiers chord =
        event.modifiers() & (Qt::ControlModifier | Qt::AltModifier | Qt::MetaModifier);
    const bool altGr = chord == (Qt::ControlModifier | Qt::AltModifier);
    if (chord != Qt::NoModifier && !altGr) {
        return false;
    }
    return std::ranges::all_of(text, [](QChar c) { return c.isPrint(); });
}

} // namespace

bool ViewportWidget::event(QEvent* event)
{
    // Ctrl+Z while a tool runs is the TOOL's: it steps back inside the tool -
    // the last vertex of a chain, the last pick - as U does, and never undoes
    // a command underneath a tool still holding points. Claimed here, where
    // Qt asks whether the window's Undo shortcut may have the key, and acted
    // on at the key press (keyPressEvent), because Qt may ask more than once
    // for one press.
    if (event->type() == QEvent::ShortcutOverride && tools_.active() &&
        static_cast<QKeyEvent*>(event)->matches(QKeySequence::Undo)) {
        event->accept();
        return true;
    }
    return QWidget::event(event);
}

void ViewportWidget::keyPressEvent(QKeyEvent* event)
{
    if (tools_.active()) {
        // While a tool runs, what is typed is ITS input - a coordinate, a
        // distance, an option letter - kept here and shown after the prompt
        // until Enter sends it, rather than going to the window as a command.
        switch (event->key()) {
        case Qt::Key_Escape:
            cancel();
            return;
        case Qt::Key_Return:
        case Qt::Key_Enter:
            toolEnter();
            return;
        case Qt::Key_Backspace:
            typed_.chop(1);
            updatePrompt();
            update();
            return;
        case Qt::Key_Space:
            // Space is Enter, as in AutoCAD, except inside something typed
            // for a tool that wants text, where it is a space.
            if (typed_.isEmpty() || tools_.expects() != cad::ToolInput::Value) {
                toolEnter();
                return;
            }
            break;
        case Qt::Key_Z:
            if (event->matches(QKeySequence::Undo)) {
                // A tool with nothing to step back says so (onError); the
                // drawing's Undo is Esc and then Ctrl+Z.
                (void)tools_.undo();
                updatePrompt();
                update();
                return;
            }
            break;
        default:
            break;
        }
        if (isTypedText(*event)) {
            typed_ += event->text();
            updatePrompt();
            update();
            event->accept();
            return;
        }
        QWidget::keyPressEvent(event);
        return;
    }
    switch (event->key()) {
    case Qt::Key_Escape:
        cancel();
        return;
    case Qt::Key_Return:
    case Qt::Key_Enter:
    case Qt::Key_Space:
        // Enter or Space at no prompt repeats the last tool, as AutoCAD
        // repeats the last command: drawing a run of circles is a click on
        // Circle and then Enter between them.
        if (!lastToolId_.empty()) {
            const auto repeated = startTool(lastToolId_);
            if (!repeated && onError) {
                onError(QString::fromStdString(repeated.error().describe()));
            }
        }
        return;
    case Qt::Key_Delete:
        if (!document_.selection().empty()) {
            run(cmd::deleteEntities(document_.selection().ids()));
        }
        return;
    default:
        break;
    }
    // Type anywhere: the text goes to the window, which hands it to the
    // command line, instead of being dropped here.
    if (onTextTyped && isTypedText(*event)) {
        onTextTyped(event->text());
        event->accept();
        return;
    }
    QWidget::keyPressEvent(event);
}

void ViewportWidget::contextMenuEvent(QContextMenuEvent* event)
{
    // A right-click was dealt with at the press (a menu, or finishing or
    // cancelling a tool); what Qt sends after it is swallowed here so that it
    // cannot reach the dock or the window behind the view. The menu key has
    // no press, so it opens the menu here, at the cursor when the cursor is
    // over the view and in the middle of it otherwise.
    event->accept();
    if (event->reason() != QContextMenuEvent::Keyboard || !onContextMenu || tools_.active() ||
        boxStart_) {
        return;
    }
    const QPoint local = mapFromGlobal(QCursor::pos());
    onContextMenu(mapToGlobal(rect().contains(local) ? local : rect().center()));
}

void ViewportWidget::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    state_.plan.resize(width(), height());
    // A view still showing what it was framed to goes on showing it at the
    // new size. One the user has panned or zoomed keeps their centre and
    // scale: resizing a dock must not undo their zoom.
    if (framedBox_.has_value()) {
        state_.plan.fit(*framedBox_, kFrameMargin);
    }
}

// ---- painting ---------------------------------------------------------------------------

void ViewportWidget::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    painter.fillRect(rect(), kBackground);
    state_.plan.resize(width(), height());
    if (!state_.planFramed) {
        frameOnFirstPaint();
    }

    // Imagery sits beneath everything: it is a backdrop, and the grid has to
    // stay legible over it. Point clouds sit above the grid but below the
    // drawing, so drawn geometry is never obscured by survey returns.
    drawRasters(painter);
    if (gridVisible_) {
        drawGrid(painter);
    }
    drawPointClouds(painter);
    painter.setRenderHint(QPainter::Antialiasing, true);
    // Beneath the drawing, like a surface: a mesh is context for what is
    // drawn over it, not a thing to be picked in plan.
    drawMeshFootprints(painter);
    drawEntities(painter);
    drawAlignments(painter);
    drawPreview(painter);
    if (drawingIsEmpty() && !tools_.active()) {
        drawEmptyHint(painter);
    }

    if (boxStart_) {
        const bool window = boxEnd_.x() >= boxStart_->x();
        QColor fill = window ? QColor(0x4c, 0xc9, 0xf0, 40) : QColor(0x7b, 0xd3, 0x89, 40);
        painter.setPen(QPen(fill.lighter(160), 1, window ? Qt::SolidLine : Qt::DashLine));
        painter.setBrush(fill);
        painter.drawRect(QRectF(*boxStart_, boxEnd_).normalized());
        painter.setBrush(Qt::NoBrush);
    }
    drawSnapMarker(painter);
    drawPrompt(painter);
}

bool ViewportWidget::drawingIsEmpty() const
{
    const auto& model = document_.model();
    return model.entities.empty() && model.alignments.empty() &&
           (reference_ == nullptr || reference_->empty()) &&
           (meshes_ == nullptr || meshes_->empty());
}

// A blank grid gives a new user nothing to go on, so an empty drawing says
// what can be done with it. Only for a drawing with NOTHING in it: a drawing
// whose layers are all hidden is not empty, and saying so would be wrong.
void ViewportWidget::drawEmptyHint(QPainter& painter) const
{
    QFont headline = font();
    headline.setPointSizeF(headline.pointSizeF() * 1.4);
    headline.setBold(true);
    const QFont body = font();
    const QString title = QStringLiteral("Nothing drawn yet");
    const QString detail = QStringLiteral(
        "Pick a drawing tool, or type a command such as LINE, CIRCLE or HELP "
        "in the command line.\n"
        "Bring data in with File > Import, or from the GIS menu.");

    const int margin = 24;
    const QRect area = rect().adjusted(margin, margin, -margin, -margin);
    const QFontMetrics titleMetrics(headline);
    const QFontMetrics bodyMetrics(body);
    // About seventy characters of the interface font: a line that can be
    // read at a glance rather than one stretched across a wide view.
    const int textWidth = std::min(area.width(), 460);
    const QRect bodyBounds = bodyMetrics.boundingRect(
        QRect(0, 0, textWidth, area.height()), Qt::AlignHCenter | Qt::TextWordWrap, detail);
    const int gap = 8;
    const int blockHeight = titleMetrics.height() + gap + bodyBounds.height();
    // A view too small to hold the hint shows the grid alone: a hint cut off
    // at the edges says less than none.
    if (textWidth < titleMetrics.horizontalAdvance(title) || blockHeight > area.height()) {
        return;
    }
    const int top = area.center().y() - blockHeight / 2;
    const int left = area.center().x() - textWidth / 2;
    const int used = std::max(bodyBounds.width(), titleMetrics.horizontalAdvance(title));

    painter.save();
    painter.setRenderHint(QPainter::TextAntialiasing, true);
    // A backing of the drawing's own ground, so the grid's axes, which cross
    // right where the hint sits, do not strike through the words.
    QColor backing = kBackground;
    backing.setAlpha(225);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawRoundedRect(
        QRect(area.center().x() - used / 2 - 16, top - 12, used + 32, blockHeight + 24), 6, 6);
    painter.setBrush(Qt::NoBrush);
    // The chrome's muted text, so the hint reads as the application talking
    // and not as something drawn.
    painter.setPen(theme::textMuted());
    painter.setFont(headline);
    painter.drawText(QRect(left, top, textWidth, titleMetrics.height()), Qt::AlignHCenter,
                     title);
    painter.setFont(body);
    painter.drawText(QRect(left, top + titleMetrics.height() + gap, textWidth, bodyBounds.height()),
                     Qt::AlignHCenter | Qt::TextWordWrap, detail);
    painter.restore();
}

void ViewportWidget::drawRasters(QPainter& painter) const
{
    if (reference_ == nullptr) {
        return;
    }
    for (const katana::interop::RasterOverlay& raster : reference_->rasters()) {
        // Hidden in the Reference Data panel, or in this view only.
        if (!raster.visible || state_.hiddenReferences.contains(raster.id) ||
            raster.width <= 0 || raster.height <= 0) {
            continue;
        }

        // Cache the QImage: rebuilding it from the RGBA bytes every frame would
        // copy tens of megabytes per repaint.
        auto cached = std::find_if(
            rasterCache_.begin(), rasterCache_.end(),
            [&raster](const RasterCache& entry) { return entry.id == raster.id; });
        if (cached == rasterCache_.end()) {
            QImage image(reinterpret_cast<const uchar*>(raster.rgba.data()), raster.width,
                         raster.height, raster.width * 4, QImage::Format_RGBA8888);
            // copy(): the QImage above only borrows the vector's buffer, and the
            // cache must outlive this loop iteration.
            rasterCache_.push_back(RasterCache{raster.id, image.copy()});
            // Drawn in this same paint. It used to `continue` and be drawn "on
            // the next pass" - which nothing asked for, so an imported image
            // stayed invisible until the mouse happened to move over the view.
            cached = std::prev(rasterCache_.end());
        }

        // Pixel -> world is the geotransform; world -> screen is the view. The
        // composition is itself affine, so it is handed to QPainter as one
        // transform rather than resampling the image here.
        //
        //   world.x = g0 + px*g1 + py*g2       screen.x = w/2 + (world.x - cx)*s
        //   world.y = g3 + px*g4 + py*g5       screen.y = h/2 - (world.y - cy)*s
        //
        // so screen.x = [w/2 + (g0-cx)s] + px*(g1 s) + py*(g2 s)
        //    screen.y = [h/2 - (g3-cy)s] + px*(-g4 s) + py*(-g5 s)
        const auto& g = raster.geotransform;
        const double s = state_.plan.scale;
        const double dx = 0.5 * width() + (g[0] - state_.plan.center.x) * s;
        const double dy = 0.5 * height() - (g[3] - state_.plan.center.y) * s;
        const QTransform transform(g[1] * s, -g[4] * s, g[2] * s, -g[5] * s, dx, dy);

        painter.save();
        painter.setOpacity(std::clamp(raster.opacity, 0.0, 1.0));
        painter.setTransform(transform);
        // Smooth only when magnifying past 1:1; downsampling a huge image with
        // smoothing on is slow and makes little visible difference.
        painter.setRenderHint(QPainter::SmoothPixmapTransform,
                              std::abs(g[1] * s) > 1.0);
        painter.drawImage(QPointF(0.0, 0.0), cached->image);
        painter.restore();
    }
}

void ViewportWidget::drawPointClouds(QPainter& painter) const
{
    if (reference_ == nullptr || width() <= 0 || height() <= 0) {
        return;
    }

    for (const katana::interop::PointCloudLayer& cloud : reference_->pointClouds()) {
        // Hidden in the Reference Data panel, or in this view only.
        if (!cloud.visible || state_.hiddenReferences.contains(cloud.id) ||
            cloud.points.empty()) {
            continue;
        }

        // Per-point colour depends only on the layer and its mode, so it is
        // computed once and cached; only the projection is redone per frame.
        auto cached = std::find_if(cloudCache_.begin(), cloudCache_.end(),
                                   [&cloud](const CloudCache& entry) {
                                       return entry.id == cloud.id &&
                                              entry.mode == cloud.colorMode;
                                   });
        if (cached == cloudCache_.end()) {
            // Drop any stale entry for this layer whose mode has changed.
            cloudCache_.erase(std::remove_if(cloudCache_.begin(), cloudCache_.end(),
                                             [&cloud](const CloudCache& entry) {
                                                 return entry.id == cloud.id;
                                             }),
                              cloudCache_.end());

            double minimum = 0.0;
            double maximum = 1.0;
            if (cloud.colorMode == katana::interop::PointColorMode::Intensity) {
                minimum = std::numeric_limits<double>::max();
                maximum = std::numeric_limits<double>::lowest();
                for (const auto& point : cloud.points) {
                    minimum = std::min(minimum, point.intensity);
                    maximum = std::max(maximum, point.intensity);
                }
            } else {
                minimum = cloud.bounds.minZ;
                maximum = cloud.bounds.maxZ;
            }

            CloudCache entry;
            entry.id = cloud.id;
            entry.mode = cloud.colorMode;
            entry.colors.reserve(cloud.points.size());
            for (const auto& point : cloud.points) {
                const katana::interop::Rgb rgb =
                    katana::interop::colorForPoint(point, cloud.colorMode, minimum, maximum);
                entry.colors.push_back(qRgb(rgb.r, rgb.g, rgb.b));
            }
            cloudCache_.push_back(std::move(entry));
            cached = std::prev(cloudCache_.end());
        }

        // Splat into an image rather than calling QPainter per point: a
        // QPainter::drawPoint costs microseconds, which at two million points is
        // seconds per frame. Writing pixels directly is a handful of
        // instructions each and keeps panning interactive.
        QImage layer(width(), height(), QImage::Format_ARGB32_Premultiplied);
        layer.fill(::Qt::transparent);
        auto* bits = reinterpret_cast<QRgb*>(layer.bits());
        const int stride = static_cast<int>(layer.bytesPerLine() / sizeof(QRgb));

        const double s = state_.plan.scale;
        const double halfWidth = 0.5 * width();
        const double halfHeight = 0.5 * height();
        const int radius = std::max(0, static_cast<int>(cloud.pointSize) - 1);

        for (std::size_t i = 0; i < cloud.points.size(); ++i) {
            const auto& point = cloud.points[i];
            const double sx = halfWidth + (point.x - state_.plan.center.x) * s;
            const double sy = halfHeight - (point.y - state_.plan.center.y) * s;
            // Reject before the cast: converting a coordinate far outside int
            // range is undefined behaviour, and panning a UTM-scale cloud when
            // zoomed in produces exactly such values.
            if (!(sx > -1.0e6 && sx < 1.0e6 && sy > -1.0e6 && sy < 1.0e6)) {
                continue;
            }
            const int px = static_cast<int>(sx);
            const int py = static_cast<int>(sy);
            if (px < 0 || py < 0 || px >= width() || py >= height()) {
                continue;
            }
            const QRgb color = cached->colors[i] | 0xff000000u;
            if (radius == 0) {
                bits[py * stride + px] = color;
                continue;
            }
            for (int oy = -radius; oy <= radius; ++oy) {
                const int y = py + oy;
                if (y < 0 || y >= height()) {
                    continue;
                }
                for (int ox = -radius; ox <= radius; ++ox) {
                    const int x = px + ox;
                    if (x >= 0 && x < width()) {
                        bits[y * stride + x] = color;
                    }
                }
            }
        }
        painter.drawImage(0, 0, layer);
    }
}

void ViewportWidget::drawGrid(QPainter& painter) const
{
    const double spacing = cad::gridSpacing(state_.plan.scale);
    const Box2 visible = state_.plan.visibleWorldBounds();
    const auto firstIndex = [&](double lo) { return static_cast<long long>(std::floor(lo / spacing)); };
    const auto lastIndex = [&](double hi) { return static_cast<long long>(std::ceil(hi / spacing)); };

    // Every fifth line is a major line so that distances can be read off.
    for (long long i = firstIndex(visible.min.x); i <= lastIndex(visible.max.x); ++i) {
        painter.setPen(QPen(i % 5 == 0 ? kGridMajor : kGridMinor, 1));
        const double x = toScreen(Point2(static_cast<double>(i) * spacing, 0.0)).x();
        painter.drawLine(QPointF(x, 0.0), QPointF(x, height()));
    }
    for (long long i = firstIndex(visible.min.y); i <= lastIndex(visible.max.y); ++i) {
        painter.setPen(QPen(i % 5 == 0 ? kGridMajor : kGridMinor, 1));
        const double y = toScreen(Point2(0.0, static_cast<double>(i) * spacing)).y();
        painter.drawLine(QPointF(0.0, y), QPointF(width(), y));
    }
    const QPointF origin = toScreen(Point2(0.0, 0.0));
    painter.setPen(QPen(kAxis, 1));
    painter.drawLine(QPointF(origin.x(), 0.0), QPointF(origin.x(), height()));
    painter.drawLine(QPointF(0.0, origin.y()), QPointF(width(), origin.y()));
}

void ViewportWidget::drawEntities(QPainter& painter) const
{
    const auto& model = document_.model();
    const Box2 visible = state_.plan.visibleWorldBounds();
    const cad::SelectionSet& selection = document_.selection();
    const bool plotting = paperPixelsPerMillimetre_ > 0.0;
    const double paper = paperScale();

    // How far each style's symbol reaches from the point it is put at, at
    // this frame's scale. A symbol is culled by what it DRAWS, not by its
    // insertion point: 12d's SSM1 draws 434 m from its point and the plot
    // stamps up to 388 m, and culling on the point dropped every one whose
    // point was just off screen while its strokes were in view. The index is
    // asked for the view grown by the furthest reach, and each entity then by
    // its own style's.
    std::map<std::string, double, std::less<>> symbolReach;
    double furthestReach = 0.0;
    model.styles.forEach([&](const katana::entity::Style& style) {
        if (style.symbol.empty()) {
            return;
        }
        const Box2 box = cad::drawnExtent(cad::pointSymbolDrawing(
            definitions_, document_.styleLibrary(), document_.libraryGeneration(), style.symbol,
            Point2(0.0, 0.0), style.symbolSize, 0.0, paper, plainMarkHalfWidth()));
        if (box.empty()) {
            return;
        }
        const double reach = std::max({std::abs(box.min.x), std::abs(box.max.x),
                                       std::abs(box.min.y), std::abs(box.max.y)});
        symbolReach.emplace(style.name, reach);
        furthestReach = std::max(furthestReach, reach);
    });

    // Through the spatial index (PLAN.MD Phase 18). This runs on EVERY repaint
    // - every pan, every zoom - not just on a click, so it is the scan that
    // mattered most. Measured in Release at 500 000 entities zoomed to 1% of
    // the extent: 22.3 ms scanning, 0.090 ms indexed. forEachCandidate falls
    // back to the ordered scan for a zoomed-out repaint, where asking the
    // index for everything would be slower than walking the model once.
    // A dash pattern is a function of the linetype, the pen width and the
    // VIEW SCALE. The scale is fixed for a frame and changes between them,
    // so the cache lives exactly one frame.
    dashCache_.clear();
    lastDrawnEntities_ = 0;
    std::vector<katana::geometry::SpatialId> scratch;
    cad::detail::forEachCandidate(
        model, &document_.spatialIndex(), visible.inflated(furthestReach), scratch,
        [&](const Entity& entity) {
        // The layer is resolved ONCE and the visibility rule is asked about
        // that, rather than looking it up again inside isDrawn.
        const katana::entity::ResolvedLayer layer = model.layers.resolve(entity.layer);
        // This box test is NOT the one forEachCandidate already did:
        // queryExtents is deliberately wider than the geometry (an arc offers
        // its centre for snapping), so this is the tighter, drawing-specific
        // filter and removing it would paint entities that are off screen.
        // Except for a dimension, whose DRAWING - label, arrows, extension
        // overshoot - reaches beyond its geometry's box, which holds only the
        // measured points and the dimension line: culling on that box dropped
        // a dimension whose label alone was on screen (audit QT-25). Its
        // query extent is exactly the drawing's box. A style's symbol widens
        // the box by its reach, for the same reason.
        const bool dimension =
            std::holds_alternative<katana::entity::DimensionGeometry>(entity.geometry);
        Box2 drawn = dimension ? cad::detail::queryExtents(model, entity)
                               : katana::entity::boundingBox(entity.geometry);
        if (const auto reach = symbolReach.find(entity.style); reach != symbolReach.end()) {
            drawn = drawn.inflated(reach->second);
        }
        if (!cad::isDrawn(layer, entity, state_.layers) || !drawn.intersects(visible)) {
            return;
        }
        ++lastDrawnEntities_;
        // Through the one resolution chain, so this agrees with the 3D view
        // and so that a named style can finally change how an entity looks -
        // Style::color was stored and validated and read by nothing.
        const auto display = katana::entity::resolveDisplay(model, entity);
        // One answer to what the linetype NAME draws (decisions D2 and D8): a
        // library linestyle's own strokes, a model linetype's dashes, or a
        // plain line - never a symbol laid along the line as a pattern.
        const cad::ResolvedLinetype pattern = cad::resolveLinePattern(
            model, document_.styleLibrary(), display.linetype, display.symbol);
        // The selection and the fading of a locked layer are screen furniture:
        // they say what the user is working on, and a plot of a drawing
        // printed them in orange dashes and half tone (audit QT-26).
        const bool selected = !plotting && selection.contains(entity.id);
        QPen entityPen; // the entity's pen WITHOUT a model linetype's dashes
        if (selected) {
            entityPen = QPen(kSelection, 2, Qt::DashLine);
            painter.setPen(entityPen);
        } else {
            // On paper, white and near-white print black (D7): white is a new
            // layer's colour and a third of the reference mapfile's, and it
            // would vanish into the sheet.
            QColor color = toQColor(plotting ? cad::paperColour(display.color, plotSettings_)
                                             : display.color);
            if (!plotting && layer.locked) {
                color.setAlpha(110); // locked layers, and their children, read as background
            }
            // On screen every line is a 1.5 px hairline: a screen has no
            // paper for a line weight to be millimetres of. On a plot the
            // width is Layer::lineWeight - "millimetres on paper" - which
            // means what it says for the first time (PLAN.MD Phase 22).
            const double penWidthPixels = plotting
                                              ? display.lineWeight * paperPixelsPerMillimetre_
                                              : 1.5;
            entityPen = QPen(color, penWidthPixels);
            QPen pen = entityPen;
            // Dashes are MODEL lengths: a 0.5 m dash stays half a metre of
            // ground at every zoom, so the pixel pattern is recomputed from
            // the view scale each frame. Qt's array is in units of PEN WIDTH,
            // not pixels, which is why the width is passed in rather than
            // assumed - at 1.5 px a pattern that forgot it would be half again
            // too long.
            // The pattern depends only on the linetype, the view scale and
            // the pen width, and the view scale is fixed for a whole frame.
            // Computing it per entity rebuilt the same handful of patterns
            // tens of thousands of times a frame and allocated twice for each.
            // Only a MODEL linetype dashes the pen: a library linestyle of the
            // same name wins, and its strokes are drawn undashed (D2).
            if (pattern.kind == cad::LinetypeKind::ModelLinetype) {
                const auto key = std::make_pair(display.linetype, penWidthPixels);
                auto cached = dashCache_.find(key);
                if (cached == dashCache_.end()) {
                    cad::DashOptions dash;
                    dash.viewScale = state_.plan.scale;
                    const auto dashes = cad::qtDashPattern(*pattern.linetype, dash, penWidthPixels);
                    cached = dashCache_.emplace(key, QList<qreal>(dashes.begin(), dashes.end()))
                                 .first;
                }
                if (!cached->second.isEmpty()) {
                    pen.setDashPattern(cached->second);
                }
            }
            painter.setPen(pen);
        }
        // A 12d definition is painted in the entity's colour and width with
        // a FLAT cap, so a 3 mm dash plots 3 mm rather than 3 mm and a pen
        // width (QPen's square cap); the painter draws its dots round.
        StylePaintTarget target;
        target.view = state_.plan;
        target.entityPen = entityPen;
        target.entityPen.setCapStyle(Qt::FlatCap);
        target.paper = plotting ? &plotSettings_ : nullptr;
        target.entityPenOnly = selected;
        // Resolved once above and reused: resolveHatchPattern used to do a
        // second full resolveDisplay of its own, and the dimension style was
        // looked up for every entity although only a dimension can use it.
        hatch_ = cad::resolveHatchPattern(model, display);
        if (std::holds_alternative<katana::entity::DimensionGeometry>(entity.geometry)) {
            dimensionStyle_ = cad::resolveDimensionStyle(model, entity);
        }
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry);
            point != nullptr && !display.symbol.empty()) {
            drawSymbol(painter, target, display.symbol, point->position, display.symbolSize);
            return;
        }
        // A 12d linestyle IS the line, gaps and all: "move 0 0 / draw 3 0 /
        // move 5 0" is a three-unit dash followed by a two-unit gap, and a
        // fence style carries the fence as well as its ticks. So it REPLACES
        // the plain line rather than being drawn over it. Drawing both filled
        // in every gap, which made every linestyle look continuous. A
        // pattern too fine to see or too long to lay is the plain line, and
        // so is one that is not laid for any other reason: never nothing.
        bool drawnByStyle = false;
        if (pattern.kind == cad::LinetypeKind::LibraryDefinition) {
            if (const auto flat = definitions_.find(document_.styleLibrary(),
                                                    document_.libraryGeneration(),
                                                    pattern.definition->name);
                flat != nullptr) {
                drawnByStyle = drawLineStyle(painter, target, *flat, entity.geometry);
            }
        }
        // A hatch is painted inside drawGeometry, so an entity carrying one
        // is drawn anyway and puts up with a doubled outline. A 12da never
        // brings a hatch; this is for a drawing given one in Katana.
        if (!drawnByStyle || hatch_ != nullptr) {
            drawGeometry(painter, entity.geometry);
        }
        // A line whose style names a symbol carries it at EVERY vertex, as a
        // 12d string does (D8) and as the 12da import intends: a fence line's
        // posts, a string of drill holes. The line above is drawn as well.
        if (!display.symbol.empty()) {
            for (const Point2& vertex : cad::symbolVertices(entity.geometry)) {
                drawSymbol(painter, target, display.symbol, vertex, display.symbolSize);
            }
        }
    });
}

void ViewportWidget::setMeshes(const std::vector<katana::cad::SceneMesh>* meshes)
{
    meshes_ = meshes;
    update();
}

// A mesh in plan is its FOOTPRINT - the hull of its vertices - and not its
// triangles: 1 453 meshes of 90 656 triangles arrive from one real archive,
// and drawing those in plan would bury the drawing they are context for. The
// 3D view is where a mesh is looked at.
void ViewportWidget::drawMeshFootprints(QPainter& painter) const
{
    if (meshes_ == nullptr) {
        return;
    }
    for (const katana::cad::SceneMesh& item : *meshes_) {
        if (!isShown(item)) {
            continue;
        }
        const auto hull = item.mesh->planHull();
        if (hull.size() < 2) {
            continue;
        }
        const QColor colour(katana::render::redOf(item.flatColor),
                            katana::render::greenOf(item.flatColor),
                            katana::render::blueOf(item.flatColor));
        QColor outline = colour;
        outline.setAlpha(190);
        painter.setPen(QPen(outline, 1, Qt::DashLine));
        QColor fill = colour;
        fill.setAlpha(40);
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(hull.size()) + 1);
        for (const auto& vertex : hull) {
            polygon << toScreen(vertex);
        }
        // Two points are a wall seen from above: a line, with nothing to fill.
        if (hull.size() == 2) {
            painter.drawPolyline(polygon);
            continue;
        }
        painter.setBrush(fill);
        painter.drawPolygon(polygon);
        painter.setBrush(Qt::NoBrush);
    }
}

double ViewportWidget::paperScale() const
{
    // Model units to one plot millimetre, which is what a `paperstyle` is
    // measured in. Dividing by the view scale is what makes such a mark keep
    // its size on the PAGE as you zoom, which is the whole point of one.
    //
    // With no plot scale set, a millimetre is a millimetre OF SCREEN. It used
    // to be one pixel, which made every paper linestyle about four times too
    // small: the ticks of a fence style came out a pixel tall and vanished
    // into the line they sit on, so the feature looked broken when it was
    // only invisible.
    const double pixelsPerMillimetre = paperPixelsPerMillimetre_ > 0.0
                                           ? paperPixelsPerMillimetre_
                                           : std::max(1.0, logicalDpiX() / 25.4);
    return pixelsPerMillimetre / std::max(state_.plan.scale, 1e-12);
}

double ViewportWidget::plainMarkHalfWidth() const
{
    // The plain point mark's size, in model units at the current scale, so a
    // built-in symbol with no size of its own stays a mark and not a blob.
    return kPointMarkerPixels / std::max(state_.plan.scale, 1e-12);
}

// A linestyle runs along whatever plan shape the entity has. An arc and a
// circle are chorded first, because a pattern is laid by distance along a
// path and a path is what a polyline is.
bool ViewportWidget::drawLineStyle(QPainter& painter, const StylePaintTarget& target,
                                   const cad::FlatDefinition& definition,
                                   const katana::entity::Geometry& geometry) const
{
    bool drew = false;
    // Only the repeats that can reach the view are laid, each exactly where
    // it falls on the whole line; a pattern finer than two pixels or longer
    // than the budget is not laid at all, and the caller draws the plain line
    // (audit CAD-04: the tail of a long line used to vanish silently).
    cad::LinestyleOptions options;
    options.paperScale = paperScale();
    options.viewScale = state_.plan.scale;
    options.visible = state_.plan.visibleWorldBounds();
    // A quarter of a pixel, the same accuracy drawGeometry chords to, so a
    // pattern laid along a curve follows the curve that was drawn.
    const double chordTolerance = 0.25 / std::max(state_.plan.scale, 1e-12);
    const auto run = [&](const katana::geometry::Polyline2& shape) {
        if (shape.vertices.size() < 2) {
            return;
        }
        const cad::LinestyleLayout laid = cad::styleDrawing(definition, shape, options);
        if (laid.outcome != cad::LinestyleLayout::Outcome::Laid) {
            return; // too fine, too long or degenerate: the caller draws the plain line
        }
        paintStyleDrawing(painter, laid.drawing, target);
        drew = true;
    };
    std::visit(
        [&](const auto& shape) {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, katana::geometry::Segment2>) {
                run(katana::geometry::Polyline2{{shape.start, shape.end}, false});
            } else if constexpr (std::is_same_v<T, katana::geometry::Polyline2>) {
                run(shape);
            } else if constexpr (std::is_same_v<T, katana::geometry::Arc2>) {
                run(katana::geometry::Polyline2{
                    katana::geometry::chordArc(shape, chordTolerance), false});
            } else if constexpr (std::is_same_v<T, katana::geometry::Circle2>) {
                run(katana::geometry::Polyline2{
                    katana::geometry::chordCircle(shape, chordTolerance), true});
            }
            // A point, a text and a mesh have no line to lay a pattern along.
        },
        geometry);
    return drew;
}

void ViewportWidget::drawSymbol(QPainter& painter, const StylePaintTarget& target,
                                const std::string& symbol, const Point2& centre,
                                double size) const
{
    // Through the one resolver the previews use: a loaded 12d definition
    // first, the sixteen built-in shapes after (PLAN.MD 20.3). Rotation is 0
    // because nothing in the model carries one yet.
    const cad::StyleDrawing drawing = cad::pointSymbolDrawing(
        definitions_, document_.styleLibrary(), document_.libraryGeneration(), symbol, centre,
        size, 0.0, paperScale(), plainMarkHalfWidth());
    if (!cad::drawnExtent(drawing).intersects(state_.plan.visibleWorldBounds())) {
        return; // culled by what it draws, not by where it stands
    }
    if (cad::belowSymbolDetail(drawing, state_.plan.scale)) {
        // Under three pixels a symbol is a smudge: a dot in its pen says a
        // point is there, for one draw call instead of every stroke.
        QPen dot = target.entityPen;
        dot.setCapStyle(Qt::RoundCap);
        dot.setWidthF(std::max(dot.widthF(), 2.0));
        const QPen previous = painter.pen();
        painter.setPen(dot);
        painter.drawPoint(toScreen(centre));
        painter.setPen(previous);
        return;
    }
    paintStyleDrawing(painter, drawing, target);
}

void ViewportWidget::drawGeometry(QPainter& painter,
                                  const katana::entity::Geometry& geometry) const
{
    // Arcs are tessellated in model space so that very large radii, where only a
    // sliver is on screen, never hand QPainter coordinates in the millions.
    const auto drawArcPath = [&](const Arc2& arc) {
        const double radiusPixels = arc.radius * state_.plan.scale;
        // Chord count for a sagitta under a quarter pixel, within sane bounds.
        const double stepAngle = radiusPixels > 1.0
                                     ? 2.0 * std::acos(std::max(0.0, 1.0 - 0.25 / radiusPixels))
                                     : katana::math::kPi;
        const int segments = static_cast<int>(
            std::clamp(std::ceil(std::abs(arc.sweep) / std::max(stepAngle, 1e-4)), 8.0, 2048.0));
        QPolygonF polygon;
        polygon.reserve(segments + 1);
        for (int i = 0; i <= segments; ++i) {
            polygon << toScreen(arc.pointAt(static_cast<double>(i) / segments));
        }
        painter.drawPolyline(polygon);
    };

    struct Visitor {
        const ViewportWidget& widget;
        QPainter& painter;
        const decltype(drawArcPath)& arcPath;

        void operator()(const katana::entity::PointGeometry& g) const
        {
            const QPointF p = widget.toScreen(g.position);
            const double r = kPointMarkerPixels;
            painter.drawLine(p + QPointF(-r, 0), p + QPointF(r, 0));
            painter.drawLine(p + QPointF(0, -r), p + QPointF(0, r));
        }
        void operator()(const Segment2& g) const
        {
            painter.drawLine(widget.toScreen(g.start), widget.toScreen(g.end));
        }
        void operator()(const Arc2& g) const { arcPath(g); }
        void operator()(const Circle2& g) const
        {
            arcPath(Arc2{g.center, g.radius, 0.0, katana::math::kTwoPi});
        }
        void operator()(const Polyline2& g) const
        {
            QPolygonF polygon;
            polygon.reserve(static_cast<int>(g.vertices.size()) + 1);
            for (const auto& vertex : g.vertices) {
                polygon << widget.toScreen(vertex);
            }
            if (g.closed && !g.vertices.empty()) {
                // The fill goes down before the boundary, so the outline stays
                // crisp over its own hatching instead of being half covered.
                widget.drawHatch(painter, g, polygon);
                polygon << widget.toScreen(g.vertices.front());
            }
            painter.drawPolyline(polygon);
        }
        void operator()(const katana::entity::TextGeometry& g) const
        {
            widget.drawText(painter, g.position, g.text, g.height, g.rotation);
        }
        void operator()(const katana::entity::DimensionGeometry& g) const
        {
            // Through the shared builder, so this draws exactly what the 3D
            // view draws and exactly what the cull box covers.
            //
            // Everything is in MODEL units now. The previous version drew a
            // fixed 5-pixel tick and a 12-pixel label, which looks right on
            // screen and plots at whatever size the paper happens to give it -
            // a dimension is part of the drawing, not an overlay on it. It also
            // formatted the number with QString::number, which is LOCALE
            // DEPENDENT and would put a comma in "1,5" on a European machine.
            const auto drawing = cad::buildDimension(g, widget.dimensionStyle_);
            if (drawing.empty()) {
                return;
            }
            const auto line = [this](const Segment2& segment) {
                painter.drawLine(widget.toScreen(segment.start), widget.toScreen(segment.end));
            };
            for (const Segment2& segment : drawing.extensionLines) {
                line(segment);
            }
            line(drawing.dimensionLine);
            for (const Segment2& stroke : drawing.arrowStrokes) {
                line(stroke);
            }
            for (const std::vector<Point2>& fill : drawing.arrowFills) {
                QPolygonF polygon;
                polygon.reserve(static_cast<int>(fill.size()));
                for (const Point2& point : fill) {
                    polygon << widget.toScreen(point);
                }
                const QBrush previous = painter.brush();
                painter.setBrush(painter.pen().color());
                painter.drawPolygon(polygon);
                painter.setBrush(previous);
            }
            widget.drawText(painter, drawing.textAnchor, drawing.text, drawing.textHeight,
                            drawing.textRotation);
        }
    };
    std::visit(Visitor{*this, painter, drawArcPath}, geometry);
}

namespace {

// Chainage in the civil convention, kilometres + metres: 1234.5 reads as
// "1+234.50". Formatted with to_chars rather than snprintf, because snprintf
// obeys the C locale and would print "1+234,50" on a machine set to one that
// uses a decimal comma - the same reason the dimension formatter avoids it.
std::string formatStation(double station)
{
    const double magnitude = std::abs(station);
    const auto kilometres = static_cast<long long>(magnitude / 1000.0);
    const double metres = magnitude - static_cast<double>(kilometres) * 1000.0;
    char buffer[32];
    const auto result =
        std::to_chars(buffer, buffer + sizeof buffer, metres, std::chars_format::fixed, 2);
    std::string metresText(buffer, result.ptr);
    while (metresText.size() < 6) { // "000.00"
        metresText.insert(metresText.begin(), '0');
    }
    return (station < 0.0 ? "-" : "") + std::to_string(kilometres) + "+" + metresText;
}

} // namespace

void ViewportWidget::drawAlignments(QPainter& painter) const
{
    const auto& model = document_.model();
    if (model.alignments.empty() || !(state_.plan.scale > 0.0)) {
        return;
    }
    // Half a pixel: finer cannot be seen, coarser shows facets on tight curves.
    const double tolerance = 0.5 / state_.plan.scale;
    const Box2 visible = state_.plan.visibleWorldBounds();
    const QColor kAlignment(0xff, 0xb7, 0x4d); // amber: an overlay, not drawing content
    const double tick = 6.0 / state_.plan.scale;     // screen-constant, like the snap marker
    const double height = 11.0 / state_.plan.scale;

    for (const katana::entity::Alignment& alignment : model.alignments.all()) {
        // Solved per repaint. A document has a handful of alignments and the
        // solve is a few spiral end-points; caching it would need invalidation
        // on every edit for no measurable gain. Measure before changing this.
        const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
        if (!solved) {
            continue; // the model refused it on the way in; nothing to draw
        }
        const Polyline2 line = solved->toPolyline(tolerance);
        if (line.vertices.size() < 2) {
            continue;
        }
        Box2 box;
        for (const Point2& vertex : line.vertices) {
            box.expand(vertex);
        }
        if (!box.inflated(tick * 4.0).intersects(visible)) {
            continue;
        }

        QPolygonF polygon;
        polygon.reserve(static_cast<int>(line.vertices.size()));
        for (const Point2& vertex : line.vertices) {
            polygon << toScreen(vertex);
        }
        painter.setPen(QPen(kAlignment, 2.0));
        painter.drawPolyline(polygon);

        painter.setPen(QPen(kAlignment, 1.0));
        // Key stations bunch up - a 10 m spiral puts TS and SC ten metres
        // apart - and their labels then print over one another into a smear
        // nobody can read. A label is skipped when it would land within its
        // own length of the last one drawn; the TICK is always drawn, because
        // the tick is the information and the label only names it.
        std::optional<QPointF> lastLabel;
        const double minimumLabelGapPixels = 70.0; // about one "0+000.00" at this text size
        for (const double station : solved->keyStations()) {
            const auto left = solved->pointAtStationOffset(station, tick);
            const auto right = solved->pointAtStationOffset(station, -tick);
            const auto direction = solved->directionAtStation(station);
            const auto label = solved->pointAtStationOffset(station, tick * 1.6);
            if (!left || !right || !direction || !label) {
                continue;
            }
            painter.drawLine(toScreen(*left), toScreen(*right));
            const QPointF at = toScreen(*label);
            if (lastLabel.has_value() &&
                QLineF(*lastLabel, at).length() < minimumLabelGapPixels) {
                continue;
            }
            lastLabel = at;
            drawText(painter, *label, formatStation(station), height, *direction);
        }
        if (const auto start = solved->pointAtStationOffset(solved->startStation(), -tick * 3.0)) {
            const auto direction = solved->directionAtStation(solved->startStation());
            drawText(painter, *start, alignment.name, height * 1.3, direction.value_or(0.0));
        }
    }
}

katana::core::Status ViewportWidget::plotToPdf(const QString& path,
                                               const cad::PlotSettings& settings)
{
    auto sheet = cad::sheetFor(settings);
    if (!sheet) {
        return sheet.error();
    }
    QPdfWriter writer(path);
    writer.setResolution(static_cast<int>(settings.dpi));
    const cad::PaperDimensions paper = cad::paperDimensions(settings.paper, settings.landscape);
    writer.setPageSize(QPageSize(QSizeF(paper.widthMm, paper.heightMm), QPageSize::Millimeter));
    // The sheet transform owns the margins; the writer's would shift the page.
    writer.setPageMargins(QMarginsF(0.0, 0.0, 0.0, 0.0));
    QPainter painter(&writer);
    if (!painter.isActive()) {
        return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                       "could not open the PDF for writing", path.toStdString());
    }

    // The same drawing members as paintEvent, through the sheet instead of
    // the screen, with line weights in paper millimetres - then the screen
    // view is put back exactly as it was. Nothing is painted or processed in
    // between, so no one sees the view's state holding the sheet.
    const cad::ViewTransform screen = state_.plan;
    state_.plan = sheet->view;
    paperPixelsPerMillimetre_ = sheet->pixelsPerMillimetre;
    plotSettings_ = settings;
    painter.setRenderHint(QPainter::Antialiasing, true);
    drawEntities(painter);
    drawAlignments(painter);
    painter.end();
    state_.plan = screen;
    paperPixelsPerMillimetre_ = 0.0;
    return {};
}

void ViewportWidget::drawHatch(QPainter& painter, const katana::geometry::Polyline2& boundary,
                                 const QPolygonF& screen) const
{
    if (hatch_ == nullptr) {
        return;
    }
    cad::HatchOptions options;
    options.viewScale = state_.plan.scale;
    const QColor color = painter.pen().color();

    switch (cad::hatchDrawing(*hatch_, options)) {
    case cad::HatchDrawing::None:
        return;
    case cad::HatchDrawing::Solid: {
        // Drawn at partial opacity rather than flat: a solid fill in the
        // entity's own colour hides the drawing underneath it, and at this zoom
        // the user is looking at the layout, not at the fill.
        QColor fill = color;
        fill.setAlpha(90);
        painter.fillPath([&] {
            QPainterPath path;
            path.addPolygon(screen);
            path.closeSubpath();
            return path;
        }(), fill);
        return;
    }
    case cad::HatchDrawing::Lines:
        break;
    }

    // Hatch lines are always solid and hairline, whatever the boundary is
    // drawn with: a dashed hatch of a dashed boundary is unreadable, and no CAD
    // package draws one.
    const QPen previous = painter.pen();
    painter.setPen(QPen(color, 0));
    for (const Segment2& line : cad::hatchSegments(boundary, *hatch_)) {
        painter.drawLine(toScreen(line.start), toScreen(line.end));
    }
    painter.setPen(previous);
}

void ViewportWidget::drawText(QPainter& painter, const Point2& position, const std::string& text,
                              double height, double rotation) const
{
    const double pixels = height * state_.plan.scale;
    const QPointF anchor = toScreen(position);
    if (pixels < 3.0) {
        // Too small to read: a stroke along the baseline keeps it discoverable.
        const double width = 0.6 * pixels * static_cast<double>(text.size());
        painter.drawLine(anchor, anchor + QPointF(std::cos(rotation), -std::sin(rotation)) * width);
        return;
    }
    QFont font("Segoe UI");
    font.setPixelSize(static_cast<int>(std::min(pixels, 2000.0)));
    painter.save();
    painter.setFont(font);
    painter.translate(anchor);
    painter.rotate(-rotation * katana::math::kRadToDeg); // screen y points down
    painter.drawText(QPointF(0.0, 0.0), QString::fromStdString(text));
    painter.restore();
}

void ViewportWidget::drawPreview(QPainter& painter) const
{
    lastPreviewCount_ = 0;
    if (!tools_.active()) {
        return;
    }
    const cad::ToolFeedback feedback = tools_.feedback(cursorWorld_);
    // A shape in the preview is drawn as the geometry it will become, so a
    // dimension or a text previews at its real size. Its style is what new
    // work gets (the current layer and style), and it is never hatched: a
    // closed outline previews as its outline.
    hatch_ = nullptr;
    katana::entity::Entity drawn;
    drawn.layer = document_.currentAttributes().layer;
    drawn.style = document_.currentAttributes().style;
    painter.setPen(QPen(kPreview, 1, Qt::DashLine));
    painter.setBrush(Qt::NoBrush);
    for (const katana::entity::Geometry& shape : feedback.shapes) {
        if (std::holds_alternative<katana::entity::DimensionGeometry>(shape)) {
            drawn.geometry = shape;
            dimensionStyle_ = cad::resolveDimensionStyle(document_.model(), drawn);
        }
        drawGeometry(painter, shape);
    }
    // Markers: a small open square, the grip AutoCAD draws at a base point.
    painter.setPen(QPen(kPreview, 1.5));
    const double r = 3.5;
    for (const Point2& marker : feedback.markers) {
        const QPointF p = toScreen(marker);
        painter.drawRect(QRectF(p.x() - r, p.y() - r, 2 * r, 2 * r));
    }
    lastPreviewCount_ = feedback.shapes.size() + feedback.markers.size();
}

void ViewportWidget::drawPrompt(QPainter& painter) const
{
    if (!tools_.active()) {
        return;
    }
    // The prompt in the view as well as in the window's command line: the
    // eye is on the drawing, and a floating view may be far from the window.
    // What has been typed follows it with a caret, as it will be sent.
    const QString text = QString("%1: %2  %3_").arg(QString::fromStdString(tools_.info()->name),
                                                    QString::fromStdString(tools_.prompt()),
                                                    typed_);
    QFont font("Segoe UI");
    font.setPixelSize(12);
    painter.setFont(font);
    const QFontMetrics metrics(font);
    const int pad = 4;
    const int height = metrics.height() + 2 * pad;
    const QRect band(0, this->height() - height, width(), height);
    painter.fillRect(band, QColor(0x12, 0x16, 0x1a, 220));
    painter.setPen(kPreview);
    painter.drawText(band.adjusted(pad + 2, 0, -pad, 0), Qt::AlignVCenter | Qt::AlignLeft,
                     metrics.elidedText(text, Qt::ElideLeft, band.width() - 2 * pad - 2));
}

void ViewportWidget::drawSnapMarker(QPainter& painter) const
{
    if (!activeSnap_ || activeSnap_->mode == cad::SnapMode::Grid) {
        return;
    }
    const QPointF p = toScreen(activeSnap_->point);
    const double r = 6.0;
    painter.setPen(QPen(kSnapMarker, 2));
    painter.setBrush(Qt::NoBrush);
    switch (activeSnap_->mode) {
    case cad::SnapMode::Endpoint:
        painter.drawRect(QRectF(p.x() - r, p.y() - r, 2 * r, 2 * r));
        break;
    case cad::SnapMode::Midpoint: {
        QPolygonF triangle;
        triangle << p + QPointF(0, -r) << p + QPointF(r, r) << p + QPointF(-r, r);
        painter.drawPolygon(triangle);
        break;
    }
    case cad::SnapMode::Center:
        painter.drawEllipse(p, r, r);
        break;
    case cad::SnapMode::Intersection:
        painter.drawLine(p + QPointF(-r, -r), p + QPointF(r, r));
        painter.drawLine(p + QPointF(-r, r), p + QPointF(r, -r));
        break;
    default: { // perpendicular, tangent, nearest: a diamond
        QPolygonF diamond;
        diamond << p + QPointF(0, -r) << p + QPointF(r, 0) << p + QPointF(0, r) << p + QPointF(-r, 0);
        painter.drawPolygon(diamond);
        break;
    }
    }
    painter.setFont(QFont("Segoe UI", 8));
    painter.drawText(p + QPointF(r + 4, -r - 2), cad::toString(activeSnap_->mode));
}

} // namespace katana::qt
