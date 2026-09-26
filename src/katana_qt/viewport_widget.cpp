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
#include <cstring>
#include <limits>

#include <QPdfWriter>
#include <QPageSize>
#include <QLineF>
#include <QMarginsF>
#include <QContextMenuEvent>
#include <QElapsedTimer>
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
const QColor kPreview(0x4c, 0xc9, 0xf0);
const QColor kSnapMarker(0xf7, 0xd0, 0x3c);

constexpr double kPickAperturePixels = 8.0;
constexpr double kSnapAperturePixels = 12.0;
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
    documentListener_ = document_.addListener([this] {
        ++notifications_; // the kept drawing is stale (drawingKey)
        update();
    });
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
    sendTyped(text.toStdString());
    updatePrompt();
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
    // What THIS view draws, through the one function a plot fits to as well.
    return planDrawnBounds(paintSource(), state_.layers, state_.hiddenReferences);
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
    paintCache_.invalidateReferences();
    ++referenceRevision_;
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
    tools_.abandon(); // its onFinished reports that the tool ended
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
        // With what the snap found, so a point of an entity is handed on as
        // one (an annotation made from it follows the entity).
        (void)tools_.point(cursorWorld_, activeSnap_);
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
        sendTyped(text);
    } else {
        (void)tools_.enter();
    }
    updatePrompt();
    update();
}

void ViewportWidget::sendTyped(const std::string& text)
{
    const bool selecting = tools_.expects() == cad::ToolInput::Selection;
    const QString word = QString::fromStdString(text).trimmed();
    if (selecting && (word.compare("U", Qt::CaseInsensitive) == 0 ||
                      word.compare("Undo", Qt::CaseInsensitive) == 0)) {
        stepBack(); // the tool's own U would skip the clicks it never saw
        return;
    }
    const auto outcome = tools_.typed(text);
    if (selecting && outcome == tools::ToolHost::Outcome::Continue &&
        tools_.expects() == cad::ToolInput::Selection) {
        // Taken by the tool at its selection step (All): the tool holds it,
        // so Ctrl+Z hands it back to the tool, in its turn.
        selectionSteps().emplace_back(std::nullopt);
    }
}

void ViewportWidget::stepBack()
{
    auto& steps = selectionSteps();
    if (tools_.expects() == cad::ToolInput::Selection && !steps.empty()) {
        std::optional<std::vector<katana::entity::EntityId>> step = std::move(steps.back());
        steps.pop_back();
        if (step) {
            cad::SelectionSet& selection = document_.selection();
            selection.set(std::move(*step));
            // An entity deleted since (by a panel, not this tool) stays gone.
            (void)selection.prune(document_.model().entities);
            document_.notifySelectionChanged();
            updatePrompt();
            update();
            return;
        }
    }
    // A tool with nothing to step back says so (onRejected, then onError).
    (void)tools_.undo();
    updatePrompt();
    update();
}

std::vector<std::optional<std::vector<katana::entity::EntityId>>>&
ViewportWidget::selectionSteps()
{
    if (selectionStepsOf_ != tools_.generation()) {
        selectionSteps_.clear();
        selectionStepsOf_ = tools_.generation();
    }
    return selectionSteps_;
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

void ViewportWidget::selectForMenu(const QPointF& screen)
{
    // A right-click on an entity with nothing selected means "this one", as
    // a left click would have; with a selection, the click may miss what the
    // menu is for, so the selection stands.
    if (!document_.selection().empty()) {
        return;
    }
    if (const auto picked = entityAt(screen)) {
        document_.selection().add(*picked);
        document_.notifySelectionChanged();
        update();
    }
}

std::vector<katana::entity::EntityId> ViewportWidget::pickedInBox(const QPointF& from,
                                                                  const QPointF& to) const
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
    return cad::pickInBox(document_.model(), box, mode, filter, &document_.spatialIndex());
}

void ViewportWidget::selectInBox(const QPointF& from, const QPointF& to,
                                 Qt::KeyboardModifiers modifiers)
{
    const auto picked = pickedInBox(from, to);
    cad::SelectionSet& selection = document_.selection();
    if (!(modifiers & (Qt::ShiftModifier | Qt::ControlModifier))) {
        selection.clear();
    }
    for (const auto id : picked) {
        selection.add(id);
    }
    document_.notifySelectionChanged();
}

void ViewportWidget::gatherForTool(const QPointF& from, const QPointF& to, bool dragged,
                                   bool takeOut)
{
    std::vector<katana::entity::EntityId> picked;
    if (dragged) {
        picked = pickedInBox(from, to);
    } else if (const auto id = entityAt(to)) {
        picked.push_back(*id);
    }
    cad::SelectionSet& selection = document_.selection();
    std::vector<katana::entity::EntityId> before = selection.ids();
    for (const auto id : picked) {
        if (takeOut) {
            selection.remove(id);
        } else {
            selection.add(id);
        }
    }
    // All adds or all removes, so the count says whether anything changed.
    if (selection.size() == before.size()) {
        return;
    }
    selectionSteps().emplace_back(std::move(before));
    document_.notifySelectionChanged();
    // The prompt counts what is picked (Trim's "<use 2 edges>"), and the
    // command line hears it only through onPrompt.
    updatePrompt();
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
            selectForMenu(event->position());
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
        const Qt::KeyboardModifiers modifiers = event->modifiers();
        if (tools_.expects() == cad::ToolInput::Selection) {
            gatherForTool(from, to, dragged,
                          (modifiers & (Qt::ShiftModifier | Qt::ControlModifier)) != 0);
        } else if (dragged) {
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
    // With no tool, the first click has selected what is under the cursor and
    // the second asks to edit it; empty space asks for nothing.
    if (tools_.active()) {
        mousePressEvent(event);
        return;
    }
    if (event->button() == Qt::LeftButton && onEntityDoubleClicked) {
        if (const auto id = entityAt(event->position())) {
            onEntityDoubleClicked(*id);
        }
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
                // The drawing's Undo is Esc and then Ctrl+Z.
                stepBack();
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
        if (onRepeatTool) {
            onRepeatTool();
        } else if (!lastToolId_.empty()) {
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
    if (rect().contains(local)) {
        selectForMenu(QPointF(local));
    }
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

PlanSource ViewportWidget::paintSource() const
{
    PlanSource source = planSourceOf(document_);
    source.reference = reference_;
    source.meshes = meshes_;
    return source;
}

PlanFrame ViewportWidget::paintFrame() const
{
    PlanFrame frame;
    frame.transform = state_.plan;
    frame.layers = &state_.layers;
    frame.hiddenReferences = &state_.hiddenReferences;
    return frame;
}

PlanPaintOptions ViewportWidget::screenOptions() const
{
    PlanPaintOptions options;
    options.medium = PlanMedium::Screen;
    // A paper linestyle's millimetre on screen is a millimetre OF SCREEN. It
    // used to be one pixel, which made every paper linestyle about four
    // times too small: the ticks of a fence style came out a pixel tall and
    // vanished into the line they sit on.
    options.pixelsPerMillimetre = std::max(1.0, logicalDpiX() / 25.4);
    options.grid = gridVisible_;
    options.thinLines = thinScreenLines();
    options.symbolSprites = true;
    // Paper-sized annotation at the document's annotation scale (ANNOSCALE,
    // docs/annotation.md); a change of it is a command, so the kept drawing
    // is repainted by the model revision it moves.
    options.annotationScale = document_.annotationScale();
    return options;
}

namespace {

// Whether plan views draw their lines as a cosmetic pixel: the View menu's
// "Thin screen lines (faster)", on unless the user turns it off.
bool thinScreenLinesSetting = true;

// FNV-1a, for the fingerprints of what the drawing depends on without the
// document saying so.
struct Fingerprint {
    std::uint64_t value = 1469598103934665603ULL;
    void add(std::uint64_t word)
    {
        for (int byte = 0; byte < 8; ++byte) {
            value ^= (word >> (8 * byte)) & 0xffU;
            value *= 1099511628211ULL;
        }
    }
    void add(double number)
    {
        std::uint64_t bits = 0;
        static_assert(sizeof bits == sizeof number);
        std::memcpy(&bits, &number, sizeof bits);
        add(bits);
    }
};

} // namespace

void ViewportWidget::setThinScreenLines(bool thin)
{
    thinScreenLinesSetting = thin;
}

bool ViewportWidget::thinScreenLines()
{
    return thinScreenLinesSetting;
}

ViewportWidget::DrawingKey ViewportWidget::drawingKey(double deviceRatio) const
{
    DrawingKey key;
    key.centreX = state_.plan.center.x;
    key.centreY = state_.plan.center.y;
    key.scale = state_.plan.scale;
    key.width = state_.plan.widthPixels;
    key.height = state_.plan.heightPixels;
    key.deviceRatio = deviceRatio;
    key.notifications = notifications_;
    key.modelRevision = document_.modelRevision();
    key.libraryGeneration = document_.libraryGeneration();
    // The selection is announced (Document::notifySelectionChanged), but a
    // caller that changes it and only asks for a repaint must still see it
    // drawn: a selection is the ids, in order, and hashing even a whole
    // drawing's is a fraction of a millisecond.
    Fingerprint selection;
    for (const katana::entity::EntityId id : document_.selection().ids()) {
        selection.add(static_cast<std::uint64_t>(id));
    }
    key.selection = selection.value;
    Fingerprint references;
    references.add(referenceRevision_);
    if (reference_ != nullptr) {
        // The panel shows, hides and fades a layer without a notification.
        for (const katana::interop::RasterOverlay& raster : reference_->rasters()) {
            references.add(static_cast<std::uint64_t>(raster.id));
            references.add(static_cast<std::uint64_t>(raster.visible));
            references.add(raster.opacity);
        }
        for (const katana::interop::PointCloudLayer& cloud : reference_->pointClouds()) {
            references.add(static_cast<std::uint64_t>(cloud.id));
            references.add(static_cast<std::uint64_t>(cloud.visible));
            references.add(static_cast<std::uint64_t>(cloud.colorMode));
            references.add(static_cast<double>(cloud.pointSize));
            references.add(static_cast<std::uint64_t>(cloud.points.size()));
        }
    }
    key.references = references.value;
    Fingerprint meshes;
    if (meshes_ != nullptr) {
        // The window's vector, which grows and restyles in place.
        meshes.add(static_cast<std::uint64_t>(meshes_->size()));
        for (const katana::cad::SceneMesh& mesh : *meshes_) {
            meshes.add(static_cast<std::uint64_t>(reinterpret_cast<std::uintptr_t>(mesh.mesh)));
            meshes.add(static_cast<std::uint64_t>(mesh.visible));
            meshes.add(static_cast<std::uint64_t>(mesh.style));
            meshes.add(static_cast<std::uint64_t>(mesh.flatColor));
        }
    }
    key.meshes = meshes.value;
    key.layers = state_.layers;
    key.hiddenReferences = state_.hiddenReferences;
    key.grid = gridVisible_;
    key.thinLines = thinScreenLines();
    return key;
}

void ViewportWidget::paintEvent(QPaintEvent*)
{
    QElapsedTimer frameTimer;
    frameTimer.start();
    QPainter painter(this);
    state_.plan.resize(width(), height());
    if (!state_.planFramed) {
        frameOnFirstPaint();
    }

    // The drawing, through the one plan painter the plot uses too, into an
    // image kept until something it depends on changes. The image is the
    // widget's size in DEVICE pixels, so on a scaled display it is as sharp
    // as painting the widget directly, and laying it down is a copy.
    const double deviceRatio = devicePixelRatioF();
    DrawingKey key = drawingKey(deviceRatio);
    const bool stale = !drawingKey_.has_value() || *drawingKey_ != key || drawing_.isNull();
    if (stale) {
        QElapsedTimer drawingTimer;
        drawingTimer.start();
        const QSize pixels(std::max(1, static_cast<int>(std::ceil(width() * deviceRatio))),
                           std::max(1, static_cast<int>(std::ceil(height() * deviceRatio))));
        if (drawing_.size() != pixels) {
            drawing_ = QImage(pixels, QImage::Format_ARGB32_Premultiplied);
        }
        drawing_.setDevicePixelRatio(deviceRatio);
        QPainter layer(&drawing_);
        layer.fillRect(rect(), kBackground);
        const PlanPaintStats stats =
            paintPlan(layer, paintSource(), paintFrame(), screenOptions(), paintCache_);
        layer.end();
        lastDrawnEntities_ = stats.entitiesDrawn;
        drawingKey_ = std::move(key);
        ++drawingPaints_;
        lastDrawingMs_ = static_cast<double>(drawingTimer.nsecsElapsed()) / 1.0e6;
    }
    painter.drawImage(QPointF(0.0, 0.0), drawing_);

    // The view's own furniture over the drawing: what a tool is making, the
    // hint for an empty drawing, the selection box, the snap and the prompt.
    painter.setRenderHint(QPainter::Antialiasing, true);
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

    lastFrameMs_ = static_cast<double>(frameTimer.nsecsElapsed()) / 1.0e6;
    if (onFrameStats) {
        // The drawing's own time, and this frame's when it only laid the
        // kept drawing down: a mouse move reads "kept", a pan the drawing.
        onFrameStats(stale ? QString("Plan  %1 drawn  %2 ms")
                                 .arg(lastDrawnEntities_)
                                 .arg(lastDrawingMs_, 0, 'f', 1)
                           : QString("Plan  %1 drawn  %2 ms (kept, %3 ms)")
                                 .arg(lastDrawnEntities_)
                                 .arg(lastDrawingMs_, 0, 'f', 1)
                                 .arg(lastFrameMs_, 0, 'f', 1));
    }
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

void ViewportWidget::setMeshes(const std::vector<katana::cad::SceneMesh>* meshes)
{
    meshes_ = meshes;
    update();
}

katana::core::Status ViewportWidget::plotToPdf(const QString& path,
                                               const cad::PlotSettings& settings)
{
    // Titled with the project's name when it has one, so a sheet opened on
    // its own says which drawing it came from.
    QString title;
    if (const auto directory = document_.projectDirectory()) {
        title = QString::fromStdWString(directory->filename().wstring());
    }
    return plotPlanToPdf(path, settings, paintSource(), state_.layers, state_.hiddenReferences,
                         plotCache_, title);
}

QImage ViewportWidget::renderToImage(const QSize& size, const QColor& background,
                                     bool asPlotted) const
{
    QImage image(size, QImage::Format_ARGB32_Premultiplied);
    image.fill(background);
    PlanFrame frame = paintFrame();
    PlanPaintOptions options = screenOptions();
    // The grid helps a person place a point; in a picture of the drawing it
    // is a mesh of lines over it.
    options.grid = false;
    // Before the first paint the view has no size of its own, and the image
    // is drawn at the view's scale as it stands.
    if (frame.transform.widthPixels > 1.0 && frame.transform.heightPixels > 1.0) {
        const double enlarge = std::min(size.width() / frame.transform.widthPixels,
                                        size.height() / frame.transform.heightPixels);
        frame.transform.scale *= enlarge;
        options.pixelsPerMillimetre *= enlarge;
    }
    frame.transform.resize(size.width(), size.height());
    // As a plot draws on paper, at the image's own millimetre: the paper
    // colour rule needs the plot's settings, the defaults of which are a
    // colour plot with white pens printing black.
    const cad::PlotSettings plotted;
    if (asPlotted) {
        options.medium = PlanMedium::Paper;
        options.plot = &plotted;
        options.symbolSprites = false;
    }
    // A cache of its own: the view's kept drawing and its cache stay as the
    // screen left them.
    PlanPaintCache cache;
    QPainter painter(&image);
    (void)paintPlan(painter, paintSource(), frame, options, cache);
    return image;
}

katana::core::Result<cad::PlotSettings> ViewportWidget::fittedPlot(cad::PlotSettings settings) const
{
    // What this view draws - its layers, reference layers, meshes and
    // alignments - and not the spatial index's bounds, which never shrink,
    // count every arc's whole circle and hidden layers, and miss alignments
    // (audit QT-12, GEO-01).
    const Box2 extent = drawnBounds();
    auto fitted = cad::fitScale(extent, settings);
    if (!fitted) {
        return fitted.error();
    }
    settings.scaleDenominator = *fitted;
    settings.center =
        Point2(0.5 * (extent.min.x + extent.max.x), 0.5 * (extent.min.y + extent.max.y));
    return settings;
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
    katana::entity::Entity drawn;
    drawn.layer = document_.currentAttributes().layer;
    drawn.style = document_.currentAttributes().style;
    painter.setPen(QPen(kPreview, 1, Qt::DashLine));
    painter.setBrush(Qt::NoBrush);
    const PlanFrame frame = paintFrame();
    const PlanPaintOptions options = screenOptions();
    katana::entity::DimensionStyle dimensionStyle{};
    for (const katana::entity::Geometry& shape : feedback.shapes) {
        if (std::holds_alternative<katana::entity::DimensionGeometry>(shape)) {
            drawn.geometry = shape;
            dimensionStyle = cad::resolveDimensionStyle(document_.model(), drawn);
        }
        paintPlanGeometry(painter, frame, options, paintCache_, shape, dimensionStyle);
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
    const QFont font = theme::overlayFont(12);
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
    painter.setFont(theme::overlayFont(11));
    painter.drawText(p + QPointF(r + 4, -r - 2), cad::toString(activeSnap_->mode));
}

} // namespace katana::qt
