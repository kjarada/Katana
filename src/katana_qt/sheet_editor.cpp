#include "sheet_editor.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <initializer_list>
#include <set>
#include <span>
#include <utility>
#include <vector>

#include <QAbstractItemView>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QContextMenuEvent>
#include <QDate>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDockWidget>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QInputDialog>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QRegularExpression>
#include <QScrollArea>
#include <QSpinBox>
#include <QStatusBar>
#include <QTabWidget>
#include <QTableWidget>
#include <QToolBar>
#include <QToolButton>
#include <QVBoxLayout>
#include <QWheelEvent>

#include "icons.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/math/numerics.hpp"
#include "plotting/grid_properties.hpp"
#include "plotting/legend_properties.hpp"
#include "plotting/plot_dialog.hpp"
#include "plotting/sheet_arrange.hpp"
#include "plotting/sheet_checks.hpp"
#include "plotting/sheet_list_widget.hpp"
#include "plotting/sheet_rulers.hpp"
#include "plotting/sheet_tables.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::core::Status;
using katana::geometry::Box2;
using katana::geometry::Point2;
using plotting::Sheet;
using plotting::SheetSet;
using plotting::TilingPreset;
using plotting::Viewport;
using plotting::ViewportKind;

namespace {

constexpr double kMinimumZoom = 0.3; // logical pixels a millimetre
constexpr double kMaximumZoom = 60.0;
constexpr double kHandlePixels = 7.0;
constexpr double kSnapPixels = 8.0;
constexpr double kMinimumViewportMm = 10.0;
// How far a press must go before it is a drag, not a click: a hand's tremor
// on a click must not move a viewport.
constexpr double kDragStartPixels = 3.0;

const QColor kDesk(88, 91, 99);
const QColor kSelection(0, 120, 215);
const QColor kGuide(230, 60, 160);
const QColor kCrossing(0, 150, 70);

constexpr std::array kKinds{ViewportKind::Plan,          ViewportKind::LongSection,
                            ViewportKind::CrossSections, ViewportKind::Model3D,
                            ViewportKind::KeyPlan,       ViewportKind::Legend,
                            ViewportKind::Notes,         ViewportKind::Image,
                            ViewportKind::SheetIndex,    ViewportKind::Revisions};

QString kindName(ViewportKind kind)
{
    switch (kind) {
    case ViewportKind::Plan: return QStringLiteral("Plan");
    case ViewportKind::LongSection: return QStringLiteral("Long section");
    case ViewportKind::CrossSections: return QStringLiteral("Cross sections");
    case ViewportKind::Model3D: return QStringLiteral("3D snapshot");
    case ViewportKind::Legend: return QStringLiteral("Legend");
    case ViewportKind::Notes: return QStringLiteral("Notes");
    case ViewportKind::Image: return QStringLiteral("Image");
    case ViewportKind::KeyPlan: return QStringLiteral("Key plan");
    case ViewportKind::SheetIndex: return QStringLiteral("Drawing register");
    case ViewportKind::Revisions: return QStringLiteral("Revision table");
    }
    return {};
}

constexpr std::array kPapers{katana::cad::PaperSize::A0, katana::cad::PaperSize::A1,
                             katana::cad::PaperSize::A2, katana::cad::PaperSize::A3,
                             katana::cad::PaperSize::A4};

QString paperName(katana::cad::PaperSize paper)
{
    return QString("A%1").arg(static_cast<int>(paper));
}

QString scaleLabel(double scale)
{
    return QString("1:%1").arg(QString::number(scale, 'f', scale == std::floor(scale) ? 0 : 2));
}

// "1:500", "500" or "auto" (nothing).
std::optional<double> parseScale(const QString& text)
{
    QString t = text.trimmed();
    if (t.startsWith(QStringLiteral("1:"))) {
        t = t.mid(2);
    }
    bool ok = false;
    const double value = t.toDouble(&ok);
    if (!ok || !(value > 0.0) || !std::isfinite(value)) {
        return std::nullopt;
    }
    return value;
}

QComboBox* scaleBox(QWidget* parent, bool withAuto)
{
    auto* box = new QComboBox(parent);
    box->setEditable(true);
    if (withAuto) {
        box->addItem(QStringLiteral("Auto"));
    }
    for (const double scale : katana::cad::kSheetScales) {
        box->addItem(scaleLabel(scale));
    }
    return box;
}

QDoubleSpinBox* spin(QWidget* parent, double minimum, double maximum, double value, int decimals,
                     const QString& suffix = QString())
{
    auto* box = new QDoubleSpinBox(parent);
    box->setRange(minimum, maximum);
    box->setDecimals(decimals);
    box->setValue(value);
    box->setSuffix(suffix);
    box->setKeyboardTracking(false);
    return box;
}

Point2 rotated(const Point2& v, double radians)
{
    const double c = std::cos(radians);
    const double s = std::sin(radians);
    return Point2(c * v.x - s * v.y, s * v.x + c * v.y);
}

// Where a rectangle of paper `rect` falls free on the sheet: the tiling area
// when the sheet has nothing on it, else a panel of `size` in its middle.
Box2 freePlace(const Sheet& sheet, double width, double height)
{
    const Box2 area = plotting::tilingArea(sheet);
    if (sheet.viewports.empty()) {
        return area;
    }
    width = std::min(width, area.width());
    height = std::min(height, area.height());
    const Point2 c = area.center();
    return Box2(Point2(c.x - width / 2.0, c.y - height / 2.0),
                Point2(c.x + width / 2.0, c.y + height / 2.0));
}

std::vector<std::string> alignmentNames(const katana::cad::Document& document)
{
    return document.model().alignments.names();
}

// A number as a verb line takes it: to a millionth, in its shortest form,
// as the replies write numbers - "24" and "24.5", never "24.500000".
QString verbNumber(double value)
{
    QString text = QString::number(std::round(value * 1e6) / 1e6, 'f', 6);
    while (text.endsWith('0')) {
        text.chop(1);
    }
    if (text.endsWith('.')) {
        text.chop(1);
    }
    return text == QStringLiteral("-0") ? QStringLiteral("0") : text;
}

// A value in double quotes, as a verb line groups words: a name or a path
// may hold spaces. The command line has no way to type a double quote in a
// value, so the dialogs never offer one.
QString quoted(const QString& text)
{
    return '"' + text + '"';
}

// "20, 40 60;80" as stations= takes it: "20,40,60,80". Empty when a part is
// not a number, which the caller says rather than run.
std::optional<QString> stationList(const QString& typed)
{
    QStringList numbers;
    for (const QString& part : typed.split(QRegularExpression("[,;\\s]+"), Qt::SkipEmptyParts)) {
        bool ok = false;
        const double value = part.toDouble(&ok);
        if (!ok || !std::isfinite(value)) {
            return std::nullopt;
        }
        numbers << verbNumber(value);
    }
    return numbers.join(',');
}

// The first line of a reply, for the status bar.
QString firstLine(const QString& text)
{
    return text.section('\n', 0, 0);
}

} // namespace

plotting::SheetVerbContext sheetVerbContextFor(const katana::cad::Document& document,
                                               const std::function<SheetSource()>& source)
{
    plotting::SheetVerbContext context;
    const SheetSource drawn = source();
    // GENERATE lays out what Generate Sheets always covered: everything the
    // plan view draws, and the visible surfaces for the sections.
    context.drawingExtent = planDrawnBounds(drawn.plan, {}, {});
    for (const auto& surface : drawn.surfaces) {
        if (surface.visible && surface.surface != nullptr) {
            context.surfaces.push_back({surface.name, surface.surface});
        }
    }
    // The rest ask for the source again when they are called: a verb may
    // change the set between being given the context and using it.
    context.check = [&document, source](std::span<const std::size_t> sheets) {
        return checkSheetsFor(document.sheetSet(), source(), sheets);
    };
    context.content = [&document, source](const plotting::Viewport& viewport) {
        return viewportContent(viewport, source(), document.sheetSet());
    };
    context.fitSection = [source](const plotting::Viewport& viewport) {
        SheetPaintCache cache;
        return resolveSectionViewport(viewport, source(), cache);
    };
    context.drawn = [source](const plotting::SheetSet& set) {
        return resolvedSheetSet(set, source());
    };
    return context;
}

// ============================================================================
// SheetCanvas
// ============================================================================

SheetCanvas::SheetCanvas(katana::cad::Document& document, SheetEditor& editor, QWidget* parent)
    : QWidget(parent), document_(document), editor_(editor)
{
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setAttribute(Qt::WA_OpaquePaintEvent, true);
    setMinimumSize(200, 150);
}

const Sheet* SheetCanvas::currentSheet() const
{
    const SheetSet& set = document_.sheetSet();
    return sheet_ < set.sheets.size() ? &set.sheets[sheet_] : nullptr;
}

double SheetCanvas::paperHeight() const
{
    const Sheet* sheet = currentSheet();
    return sheet != nullptr ? katana::cad::paperDimensions(sheet->paper, sheet->landscape).heightMm
                            : 297.0;
}

const Viewport* SheetCanvas::viewport(const std::string& id) const
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return nullptr;
    }
    for (const Viewport& v : sheet->viewports) {
        if (v.id == id) {
            return &v;
        }
    }
    return nullptr;
}

void SheetCanvas::setSheet(std::size_t index)
{
    if (index != sheet_) {
        sheet_ = index;
        selected_.clear();
        selection_.clear();
        fitted_ = false;
        invalidate();
        selectionChanged();
    }
}

void SheetCanvas::select(std::string viewportId)
{
    if (!viewportId.empty() && viewport(viewportId) == nullptr) {
        viewportId.clear();
    }
    std::vector<std::string> selection;
    if (!viewportId.empty()) {
        selection.push_back(viewportId);
    }
    if (viewportId == selected_ && selection == selection_) {
        return;
    }
    selected_ = std::move(viewportId);
    selection_ = std::move(selection);
    selectionChanged();
}

void SheetCanvas::invalidate()
{
    ++version_;
    pruneSelection();
    update();
}

void SheetCanvas::fitPage()
{
    const Sheet* sheet = currentSheet();
    const auto paper = sheet != nullptr
                           ? katana::cad::paperDimensions(sheet->paper, sheet->landscape)
                           : katana::cad::paperDimensions(katana::cad::PaperSize::A3, true);
    const double margin = 24.0;
    // The paper fitted to what the rulers leave.
    const double inset = rulerPixels();
    const double w = width() - inset;
    const double h = height() - inset;
    zoom_ = std::clamp(std::min((w - 2.0 * margin) / paper.widthMm,
                                (h - 2.0 * margin) / paper.heightMm),
                       kMinimumZoom, kMaximumZoom);
    origin_ = QPointF(inset + (w - paper.widthMm * zoom_) / 2.0,
                      inset + (h - paper.heightMm * zoom_) / 2.0);
    fitted_ = true;
    update();
}

void SheetCanvas::zoomTo(const Box2& box)
{
    if (box.empty() || currentSheet() == nullptr) {
        fitPage();
        return;
    }
    const double margin = 24.0;
    const double inset = rulerPixels();
    const double w = width() - inset;
    const double h = height() - inset;
    zoom_ = std::clamp(std::min((w - 2.0 * margin) / std::max(box.width(), 1.0),
                                (h - 2.0 * margin) / std::max(box.height(), 1.0)),
                       kMinimumZoom, kMaximumZoom);
    // The box's centre in the middle of what the rulers leave.
    const Point2 c = box.center();
    origin_ = QPointF(inset + w / 2.0 - c.x * zoom_,
                      inset + h / 2.0 - (paperHeight() - c.y) * zoom_);
    fitted_ = true;
    update();
}

QPointF SheetCanvas::paperToWidget(const Point2& paper) const
{
    return origin_ + QPointF(paper.x * zoom_, (paperHeight() - paper.y) * zoom_);
}

Point2 SheetCanvas::widgetToPaper(const QPointF& widget) const
{
    const QPointF d = widget - origin_;
    return Point2(d.x() / zoom_, paperHeight() - d.y() / zoom_);
}

QRectF SheetCanvas::widgetRect(const Box2& paper) const
{
    return QRectF(paperToWidget(Point2(paper.min.x, paper.max.y)),
                  paperToWidget(Point2(paper.max.x, paper.min.y)));
}

std::string SheetCanvas::viewportAt(const QPointF& widget) const
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return {};
    }
    const Point2 p = widgetToPaper(widget);
    // Front to back: the one drawn last is the one on top.
    for (auto it = sheet->viewports.rbegin(); it != sheet->viewports.rend(); ++it) {
        if (!it->rect.empty() && it->rect.contains(p)) {
            return it->id;
        }
    }
    return {};
}

std::optional<int> SheetCanvas::handleAt(const QPointF& widget) const
{
    // Handles only on a viewport selected alone: a group is moved, not sized.
    const Viewport* v = viewport(selected_);
    if (v == nullptr || v->locked || selection_.size() != 1) {
        return std::nullopt;
    }
    const QRectF r = widgetRect(v->rect);
    const std::array<QPointF, 8> handles{r.topLeft(),     QPointF(r.center().x(), r.top()),
                                         r.topRight(),    QPointF(r.right(), r.center().y()),
                                         r.bottomRight(), QPointF(r.center().x(), r.bottom()),
                                         r.bottomLeft(),  QPointF(r.left(), r.center().y())};
    for (int i = 0; i < 8; ++i) {
        if (std::abs(widget.x() - handles[i].x()) <= kHandlePixels &&
            std::abs(widget.y() - handles[i].y()) <= kHandlePixels) {
            return i;
        }
    }
    return std::nullopt;
}

void SheetCanvas::render()
{
    const double ratio = devicePixelRatioF() > 0.0 ? devicePixelRatioF() : 1.0;
    image_ = QImage(std::max(static_cast<int>(width() * ratio), 1),
                    std::max(static_cast<int>(height() * ratio), 1),
                    QImage::Format_ARGB32_Premultiplied);
    image_.setDevicePixelRatio(ratio);
    image_.fill(kDesk);
    imageOrigin_ = origin_;
    imageZoom_ = zoom_;
    imageVersion_ = version_;
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        stats_ = {};
        return;
    }
    ++renders_;
    QPainter painter(&image_);
    const auto paper = katana::cad::paperDimensions(sheet->paper, sheet->landscape);
    // The paper's shadow on the desk.
    painter.fillRect(QRectF(origin_ + QPointF(4.0, 4.0),
                            QSizeF(paper.widthMm * zoom_, paper.heightMm * zoom_)),
                     QColor(0, 0, 0, 70));
    painter.scale(1.0 / ratio, 1.0 / ratio);
    SheetPaintOptions options;
    options.pixelsPerMillimetre = zoom_ * ratio;
    options.origin = origin_ * ratio;
    options.construction = true;
    options.slotHints = true;
    options.rasterDpiCap = 110.0;
    const SheetSource source = editor_.source();
    QApplication::setOverrideCursor(Qt::WaitCursor);
    stats_ = paintSheet(painter, document_.sheetSet(), sheet_, source, options, cache_);
    QApplication::restoreOverrideCursor();
}

void SheetCanvas::paintEvent(QPaintEvent* /*event*/)
{
    if (!fitted_ && currentSheet() != nullptr) {
        fitPage();
    }
    const bool panning = drag_ == Drag::Pan;
    if (!panning && (imageVersion_ != version_ || imageZoom_ != zoom_ ||
                     imageOrigin_ != origin_ ||
                     image_.size() != (size() * devicePixelRatioF()))) {
        render();
    }
    QPainter painter(this);
    painter.fillRect(rect(), kDesk);
    // While panning, the last paint moves with the hand and is painted afresh
    // when it lets go.
    painter.drawImage(origin_ - imageOrigin_, image_);

    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        painter.setPen(Qt::white);
        painter.drawText(rect(), Qt::AlignCenter,
                         QStringLiteral("No sheets yet.\nUse Generate Sheets or New Sheet."));
        return;
    }
    painter.setRenderHint(QPainter::Antialiasing, true);
    const auto paper = katana::cad::paperDimensions(sheet->paper, sheet->landscape);
    RulerView ruler;
    ruler.pixelsPerMillimetre = zoom_;
    ruler.paperOrigin = origin_;
    ruler.paperWidthMm = paper.widthMm;
    ruler.paperHeightMm = paper.heightMm;

    // The drawing dragged inside its viewport: the paint shifted, clipped to it.
    if (drag_ == Drag::PanView && pressMoved_) {
        if (const Viewport* v = viewport(panViewId_)) {
            const QRectF r = widgetRect(v->rect);
            painter.save();
            painter.setClipRect(r);
            painter.fillRect(r, Qt::white);
            painter.drawImage(origin_ - imageOrigin_ +
                                  QPointF(viewShift_.x * zoom_, -viewShift_.y * zoom_),
                              image_);
            painter.restore();
        }
    }

    // The paper grid, faint, while a drag snaps to it.
    if (snapGrid_) {
        paintPaperGrid(painter, ruler, gridMm_);
        painter.setRenderHint(QPainter::Antialiasing, true);
    }

    // The drawing area, faint, so an empty sheet shows where views go.
    painter.setPen(QPen(QColor(0, 120, 215, 60), 1.0, Qt::DashLine));
    painter.setBrush(Qt::NoBrush);
    painter.drawRect(widgetRect(plotting::drawingArea(*sheet)));

    // What is being moved or sized, its own paint going with it.
    paintDraggedContent(painter);

    // Every selected viewport outlined where it is now; a group's bounds.
    Box2 bounds;
    for (const std::string& id : selection_) {
        const Box2 box = liveRect(id);
        bounds.expand(box);
        if (id != selected_ && !box.empty()) {
            painter.setPen(QPen(kSelection, 1.5));
            painter.drawRect(widgetRect(box));
        }
    }
    ruler.highlight = bounds;
    if (selection_.size() > 1 && !bounds.empty()) {
        painter.setPen(QPen(kSelection, 1.0, Qt::DashLine));
        painter.drawRect(widgetRect(bounds).adjusted(-4.0, -4.0, 4.0, 4.0));
    }

    // The primary, its handles, and the rectangle being dragged.
    if (const Viewport* v = viewport(selected_)) {
        const bool dragging = pressMoved_ && ((drag_ == Drag::Move && !startRects_.empty()) ||
                                              drag_ == Drag::Resize);
        const Box2 box = dragging && selection_.size() > 1 ? bounds : liveRect(selected_);
        const QRectF r = widgetRect(liveRect(selected_));
        painter.setPen(QPen(kSelection, 2.0));
        painter.drawRect(r);
        if (!v->locked && selection_.size() == 1) {
            painter.setBrush(Qt::white);
            painter.setPen(QPen(kSelection, 1.2));
            for (const QPointF& at :
                 {r.topLeft(), QPointF(r.center().x(), r.top()), r.topRight(),
                  QPointF(r.right(), r.center().y()), r.bottomRight(),
                  QPointF(r.center().x(), r.bottom()), r.bottomLeft(),
                  QPointF(r.left(), r.center().y())}) {
                painter.drawRect(QRectF(at - QPointF(4.0, 4.0), QSizeF(8.0, 8.0)));
            }
            painter.setBrush(Qt::NoBrush);
        }
        if (dragging) {
            painter.setPen(QPen(kGuide, 1.0, Qt::DashLine));
            if (guideX_) {
                const double x = paperToWidget(Point2(*guideX_, 0.0)).x();
                painter.drawLine(QPointF(x, 0.0), QPointF(x, height()));
            }
            if (guideY_) {
                const double y = paperToWidget(Point2(0.0, *guideY_)).y();
                painter.drawLine(QPointF(0.0, y), QPointF(width(), y));
            }
            // Where it is and how big, as the status line would say it.
            const QRectF around = widgetRect(box);
            const QString label = QString("%1, %2   %3 x %4 mm")
                                      .arg(box.min.x, 0, 'f', 1)
                                      .arg(box.min.y, 0, 'f', 1)
                                      .arg(box.width(), 0, 'f', 1)
                                      .arg(box.height(), 0, 'f', 1);
            const QRectF plate = painter.fontMetrics()
                                     .boundingRect(label)
                                     .toRectF()
                                     .translated(around.bottomLeft() + QPointF(4.0, 24.0))
                                     .adjusted(-4.0, -2.0, 4.0, 2.0);
            painter.fillRect(plate, QColor(30, 32, 38, 200));
            painter.setPen(Qt::white);
            painter.drawText(around.bottomLeft() + QPointF(4.0, 24.0), label);
        }
    }

    // The rubber band: a window solid and blue, a crossing dashed and green,
    // as CAD programs draw them.
    if (drag_ == Drag::Band && pressMoved_) {
        const bool crossing = bandCorner_.x < pressPaper_.x;
        const QColor ink = crossing ? kCrossing : kSelection;
        QColor fill = ink;
        fill.setAlpha(28);
        const Box2 band(Point2(std::min(pressPaper_.x, bandCorner_.x), std::min(pressPaper_.y, bandCorner_.y)),
                        Point2(std::max(pressPaper_.x, bandCorner_.x), std::max(pressPaper_.y, bandCorner_.y)));
        painter.setPen(QPen(ink, 1.0, crossing ? Qt::DashLine : Qt::SolidLine));
        painter.setBrush(fill);
        painter.drawRect(widgetRect(band));
        painter.setBrush(Qt::NoBrush);
    }

    if (rulers_) {
        ruler.cursor = cursorPaper_;
        paintSheetRulers(painter, size(), ruler);
    }
}

void SheetCanvas::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    if (!fitted_ || drag_ == Drag::None) {
        fitted_ = false;
    }
}

void SheetCanvas::mousePressEvent(QMouseEvent* event)
{
    setFocus(Qt::MouseFocusReason);
    const QPointF at = event->position();
    pressWidget_ = at;
    pressPaper_ = widgetToPaper(at);
    guideX_.reset();
    guideY_.reset();
    pressMoved_ = false;
    narrowOnClick_ = false;
    pressHit_.clear();
    startRects_.clear();
    liveDelta_ = Point2();
    // The middle button, or the left with Space held, drags the paper about.
    if (event->button() == Qt::MiddleButton ||
        (event->button() == Qt::LeftButton && spaceHeld_)) {
        drag_ = Drag::Pan;
        setCursor(Qt::ClosedHandCursor);
        return;
    }
    if (event->button() != Qt::LeftButton) {
        return;
    }
    // The rulers are not paper.
    if (at.x() < rulerPixels() || at.y() < rulerPixels()) {
        return;
    }
    const bool shift = (event->modifiers() & Qt::ShiftModifier) != 0;
    const bool control = (event->modifiers() & Qt::ControlModifier) != 0;
    if (!shift && !control) {
        if (auto handle = handleAt(at)) {
            drag_ = Drag::Resize;
            handle_ = *handle;
            startRect_ = liveRect_ = viewport(selected_)->rect;
            return;
        }
    }
    const std::string hit = viewportAt(at);
    const Viewport* v = viewport(hit);
    if (v == nullptr) {
        // Empty paper: a rubber band, adding to the selection with Ctrl or
        // Shift, else replacing it when it is let go - so Escape part way
        // leaves the selection as it was.
        bandAdds_ = shift || control;
        drag_ = Drag::Band;
        bandCorner_ = pressPaper_;
        return;
    }
    if (shift && (v->kind == ViewportKind::Plan || v->kind == ViewportKind::KeyPlan)) {
        // Shift-drag pans the drawing inside a plan; a Shift-click without a
        // drag toggles the plan in the selection (commitDrag).
        drag_ = Drag::PanView;
        panViewId_ = hit;
        viewShift_ = Point2();
        startCentre_ = resolvedPlan(*v).centre;
        setCursor(Qt::SizeAllCursor);
        return;
    }
    if (shift || control) {
        toggleSelected(hit);
        return;
    }
    if (isSelected(hit)) {
        // One of a group: it becomes the primary and the group moves with it;
        // let go without moving, it is selected alone.
        narrowOnClick_ = selection_.size() > 1;
        setSelection(selection_, hit);
    } else {
        select(hit);
    }
    pressHit_ = hit;
    const Sheet* sheet = currentSheet();
    startBounds_ = Box2();
    for (const Viewport& each : sheet->viewports) {
        if (isSelected(each.id) && !each.locked && !each.rect.empty()) {
            startRects_.emplace(each.id, each.rect);
            startBounds_.expand(each.rect);
        }
    }
    // A press on a locked member of a group still waits for the release: let
    // go without moving, it is selected alone; dragged, nothing moves.
    if (!startRects_.empty() || narrowOnClick_) {
        drag_ = Drag::Move;
        startRect_ = liveRect_ = startBounds_;
    }
}

void SheetCanvas::updateDrag(const QPointF& widget, Qt::KeyboardModifiers modifiers)
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return;
    }
    // A press is a click until the cursor has gone a few pixels from it.
    if (!pressMoved_) {
        const QPointF gone = widget - pressWidget_;
        if (std::hypot(gone.x(), gone.y()) < kDragStartPixels) {
            return;
        }
        pressMoved_ = true;
        narrowOnClick_ = false;
        if (drag_ == Drag::PanView) {
            setCursor(Qt::SizeAllCursor);
        }
    }
    const Point2 now = widgetToPaper(widget);
    const Point2 delta = now - pressPaper_;
    // Alt: no snapping at all, to the edges or to the grid.
    const bool snap = (modifiers & Qt::AltModifier) == 0;
    const double tolerance = snap ? kSnapPixels / zoom_ : 0.0;
    const double grid = snap && snapGrid_ ? gridMm_ : 0.0;
    const Box2 area = plotting::drawingArea(*sheet);
    // What the moving viewports snap to: every placed viewport not moving.
    std::vector<Box2> others;
    for (const Viewport& v : sheet->viewports) {
        const bool moving = drag_ == Drag::Move ? startRects_.contains(v.id) : v.id == selected_;
        if (!moving && !v.rect.empty()) {
            others.push_back(v.rect);
        }
    }
    guideX_.reset();
    guideY_.reset();
    if (drag_ == Drag::Move && startRects_.empty()) {
        // Only locked viewports under the hand: nothing to move.
    } else if (drag_ == Drag::Move) {
        // The group's bounds snap, and every member moves as they do.
        Box2 moved(startBounds_.min + delta, startBounds_.max + delta);
        if (snap) {
            const plotting::SnapResult snapped =
                plotting::snapMovingRect(moved, area, others, tolerance, grid);
            moved = snapped.rect;
            guideX_ = snapped.guideX;
            guideY_ = snapped.guideY;
        }
        liveRect_ = moved;
        liveDelta_ = moved.min - startBounds_.min;
    } else if (drag_ == Drag::Resize) {
        Box2 r = startRect_;
        // Which edges this handle moves: 0 TL, 1 T, 2 TR, 3 R, 4 BR, 5 B, 6 BL, 7 L.
        const bool left = handle_ == 0 || handle_ == 6 || handle_ == 7;
        const bool right = handle_ == 2 || handle_ == 3 || handle_ == 4;
        const bool top = handle_ == 0 || handle_ == 1 || handle_ == 2;
        const bool bottom = handle_ == 4 || handle_ == 5 || handle_ == 6;
        std::vector<double> xs{area.min.x, area.max.x};
        std::vector<double> ys{area.min.y, area.max.y};
        for (const Box2& o : others) {
            xs.insert(xs.end(), {o.min.x, o.max.x});
            ys.insert(ys.end(), {o.min.y, o.max.y});
        }
        // An edge snaps to another edge in reach, else to the grid.
        const auto snapTo = [&](double value, const std::vector<double>& targets,
                                std::optional<double>& guide) {
            const plotting::EdgeSnap snapped = plotting::snapEdge(value, targets, tolerance, grid);
            if (snapped.guide) {
                guide = snapped.guide;
            }
            return snapped.value;
        };
        if (left) {
            r.min.x = std::min(snapTo(startRect_.min.x + delta.x, xs, guideX_),
                               r.max.x - kMinimumViewportMm);
        }
        if (right) {
            r.max.x = std::max(snapTo(startRect_.max.x + delta.x, xs, guideX_),
                               r.min.x + kMinimumViewportMm);
        }
        if (bottom) {
            r.min.y = std::min(snapTo(startRect_.min.y + delta.y, ys, guideY_),
                               r.max.y - kMinimumViewportMm);
        }
        if (top) {
            r.max.y = std::max(snapTo(startRect_.max.y + delta.y, ys, guideY_),
                               r.min.y + kMinimumViewportMm);
        }
        liveRect_ = r;
    } else if (drag_ == Drag::PanView) {
        viewShift_ = delta;
    } else if (drag_ == Drag::Band) {
        bandCorner_ = now;
    }
    update();
}

void SheetCanvas::mouseMoveEvent(QMouseEvent* event)
{
    const QPointF at = event->position();
    updateReadout(at);
    switch (drag_) {
    case Drag::Pan:
        origin_ += at - pressWidget_;
        pressWidget_ = at;
        update();
        return;
    case Drag::Move:
    case Drag::Resize:
    case Drag::PanView:
    case Drag::Band:
        updateDrag(at, event->modifiers());
        return;
    case Drag::None:
        break;
    }
    // The cursor says what a press would do.
    if (spaceHeld_) {
        setCursor(Qt::OpenHandCursor);
    } else if (auto handle = handleAt(at)) {
        static constexpr std::array<Qt::CursorShape, 8> kCursors{
            Qt::SizeFDiagCursor, Qt::SizeVerCursor, Qt::SizeBDiagCursor, Qt::SizeHorCursor,
            Qt::SizeFDiagCursor, Qt::SizeVerCursor, Qt::SizeBDiagCursor, Qt::SizeHorCursor};
        setCursor(kCursors[*handle]);
    } else if (!viewportAt(at).empty()) {
        setCursor(Qt::SizeAllCursor);
    } else {
        setCursor(Qt::ArrowCursor);
    }
}

void SheetCanvas::commitDrag()
{
    const Drag drag = drag_;
    const bool moved = pressMoved_;
    drag_ = Drag::None;
    pressMoved_ = false;
    guideX_.reset();
    guideY_.reset();
    unsetCursor();
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        update();
        return;
    }
    Status status;
    if (drag == Drag::Band) {
        if (!moved) {
            // A click on empty paper: nothing selected, unless it was to add.
            if (!bandAdds_) {
                select({});
            }
        } else {
            const Box2 band(Point2(std::min(pressPaper_.x, bandCorner_.x),
                                   std::min(pressPaper_.y, bandCorner_.y)),
                            Point2(std::max(pressPaper_.x, bandCorner_.x),
                                   std::max(pressPaper_.y, bandCorner_.y)));
            const auto mode = bandCorner_.x < pressPaper_.x ? plotting::BandMode::Crossing
                                                            : plotting::BandMode::Window;
            const std::vector<std::string> picked = plotting::viewportsInBand(*sheet, band, mode);
            if (bandAdds_) {
                std::vector<std::string> ids = selection_;
                ids.insert(ids.end(), picked.begin(), picked.end());
                setSelection(std::move(ids), selected_);
            } else {
                setSelection(picked);
            }
        }
    } else if (drag == Drag::PanView) {
        const std::string id = std::exchange(panViewId_, std::string{});
        const Point2 shift = std::exchange(viewShift_, Point2());
        const Viewport* v = viewport(id);
        if (v != nullptr && !moved) {
            toggleSelected(id); // a Shift-click, not a drag
        } else if (v != nullptr && (shift.x != 0.0 || shift.y != 0.0)) {
            const ResolvedViewport resolved = resolvedPlan(*v);
            // The drawing follows the hand, so the centre moves the other way.
            const Point2 centre =
                startCentre_ - rotated(shift, v->rotation) * (resolved.scale / 1000.0);
            const double scale = resolved.scale;
            status = plotting::editViewport(
                document_, id,
                [centre, scale](Viewport& edited) {
                    edited.centre = centre;
                    edited.scale = scale;
                    edited.autoCentre = false;
                    edited.autoScale = false;
                    return Status{};
                },
                "PAN_VIEWPORT");
        }
    } else if (drag == Drag::Move && !moved) {
        // A click on one of a group: that one alone.
        if (narrowOnClick_) {
            select(pressHit_);
        }
    } else if (drag == Drag::Move && startRects_.size() == 1) {
        // One viewport takes the snapped rectangle exactly.
        const auto& [id, start] = *startRects_.begin();
        const Box2 rect = liveRect_;
        if (!(rect == start)) {
            status = plotting::editViewport(
                document_, id,
                [rect](Viewport& edited) {
                    edited.rect = rect;
                    return Status{};
                },
                "MOVE_VIEWPORT");
        }
    } else if (drag == Drag::Move && (liveDelta_.x != 0.0 || liveDelta_.y != 0.0)) {
        // A group: every member by the same distance, one step.
        std::vector<std::string> ids;
        for (const auto& entry : startRects_) {
            ids.push_back(entry.first);
        }
        status = plotting::moveViewports(document_, sheet_, ids, liveDelta_, "MOVE_VIEWPORTS");
    } else if (drag == Drag::Resize && moved) {
        const Viewport* v = viewport(selected_);
        if (v != nullptr && !(liveRect_ == v->rect)) {
            const Box2 rect = liveRect_;
            status = plotting::editViewport(
                document_, selected_,
                [rect](Viewport& edited) {
                    edited.rect = rect;
                    return Status{};
                },
                "RESIZE_VIEWPORT");
        }
    }
    startRects_.clear();
    narrowOnClick_ = false;
    viewShift_ = Point2();
    // A refused edit leaves the viewport where it was; the paint shows that.
    (void)status;
    update();
}

void SheetCanvas::mouseReleaseEvent(QMouseEvent* /*event*/)
{
    if (drag_ == Drag::Pan) {
        drag_ = Drag::None;
        unsetCursor();
        update(); // paints afresh where it was dropped
        return;
    }
    if (drag_ != Drag::None) {
        commitDrag();
    }
}

void SheetCanvas::mouseDoubleClickEvent(QMouseEvent* event)
{
    const QPointF at = event->position();
    if (event->button() != Qt::LeftButton || at.x() < rulerPixels() || at.y() < rulerPixels()) {
        return;
    }
    // A viewport fills the window; the desk or empty paper fits the page.
    const std::string hit = viewportAt(at);
    if (const Viewport* v = viewport(hit)) {
        select(hit);
        zoomTo(v->rect);
    } else {
        fitPage();
    }
}

void SheetCanvas::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches == 0.0) {
        return;
    }
    const double before = zoom_;
    zoom_ = std::clamp(zoom_ * std::pow(1.2, notches), kMinimumZoom, kMaximumZoom);
    const QPointF at = event->position();
    origin_ = at - (at - origin_) * (zoom_ / before);
    fitted_ = true;
    update();
    event->accept();
}

void SheetCanvas::keyPressEvent(QKeyEvent* event)
{
    // Space held: a left drag pans the paper, as in drawing programs.
    if (event->key() == Qt::Key_Space) {
        if (!event->isAutoRepeat()) {
            spaceHeld_ = true;
            if (drag_ == Drag::None) {
                setCursor(Qt::OpenHandCursor);
            }
        }
        return;
    }
    if (event->key() == Qt::Key_Escape) {
        // A drag under way is abandoned, leaving everything where it was.
        if (drag_ != Drag::None) {
            cancelDrag();
        } else {
            select({});
        }
        return;
    }
    if (selection_.empty()) {
        QWidget::keyPressEvent(event);
        return;
    }
    if (event->key() == Qt::Key_Delete || event->key() == Qt::Key_Backspace) {
        (void)editor_.removeSelectedViewport();
        return;
    }
    const double step = (event->modifiers() & Qt::ShiftModifier) != 0 ? 10.0 : 1.0;
    Point2 nudge;
    switch (event->key()) {
    case Qt::Key_Left: nudge = Point2(-step, 0.0); break;
    case Qt::Key_Right: nudge = Point2(step, 0.0); break;
    case Qt::Key_Up: nudge = Point2(0.0, step); break;
    case Qt::Key_Down: nudge = Point2(0.0, -step); break;
    default: QWidget::keyPressEvent(event); return;
    }
    // Every selected viewport, one step; the locked ones stay.
    (void)editor_.nudgeSelection(nudge);
}

void SheetCanvas::contextMenuEvent(QContextMenuEvent* event)
{
    // On one of a group, the menu is for the group.
    const std::string hit = viewportAt(event->pos());
    if (!isSelected(hit)) {
        select(hit);
    }
    if (onContextMenu) {
        onContextMenu(event->globalPos(), hit);
    }
}

// ============================================================================
// SheetEditor
// ============================================================================

SheetEditor::SheetEditor(katana::cad::Document& document, SourceProvider source, QWidget* parent)
    : QMainWindow(parent, Qt::Window), document_(document), source_(std::move(source))
{
    setObjectName(QStringLiteral("sheetEditor"));
    setWindowTitle(QStringLiteral("Sheets"));
    resize(1400, 900);

    canvas_ = new SheetCanvas(document_, *this, this);
    canvas_->setObjectName(QStringLiteral("sheetCanvas"));
    setCentralWidget(canvas_);

    // The sheets.
    auto* listDock = new QDockWidget(QStringLiteral("Sheets"), this);
    listDock->setObjectName(QStringLiteral("sheetListDock"));
    listDock->setFeatures(QDockWidget::DockWidgetMovable);
    auto* listPanel = new QWidget(listDock);
    auto* listLayout = new QVBoxLayout(listPanel);
    listLayout->setContentsMargins(4, 4, 4, 4);
    list_ = new SheetListWidget(listPanel);
    list_->setObjectName(QStringLiteral("sheetList"));
    listLayout->addWidget(list_);
    auto* buttons = new QHBoxLayout();
    const auto button = [&](const QString& text, const QString& tip, auto&& action) {
        auto* b = new QToolButton(listPanel);
        b->setText(text);
        b->setToolTip(tip);
        connect(b, &QToolButton::clicked, this, action);
        buttons->addWidget(b);
        return b;
    };
    button(QStringLiteral("+"), QStringLiteral("A new blank sheet"), [this] { (void)addBlankSheet(); });
    button(QStringLiteral("Copy"), QStringLiteral("Duplicate this sheet"), [this] {
        if (auto s = plotting::duplicateSheet(document_, currentSheet()); !s) {
            report(QString::fromStdString(s.error().describe()), true);
        } else {
            setCurrentSheet(currentSheet() + 1);
        }
    });
    button(QStringLiteral("−"), QStringLiteral("Remove this sheet"), [this] {
        if (document_.sheetSet().sheets.empty()) {
            return;
        }
        (void)plotting::removeSheet(document_, currentSheet());
    });
    button(QStringLiteral("↑"), QStringLiteral("Move this sheet up"), [this] {
        const std::size_t at = currentSheet();
        if (at > 0 && plotting::moveSheet(document_, at, at - 1)) {
            setCurrentSheet(at - 1);
        }
    });
    button(QStringLiteral("↓"), QStringLiteral("Move this sheet down"), [this] {
        const std::size_t at = currentSheet();
        if (at + 1 < document_.sheetSet().sheets.size() &&
            plotting::moveSheet(document_, at, at + 1)) {
            setCurrentSheet(at + 1);
        }
    });
    buttons->addStretch(1);
    listLayout->addLayout(buttons);
    listDock->setWidget(listPanel);
    addDockWidget(Qt::LeftDockWidgetArea, listDock);
    connect(list_, &QListWidget::currentRowChanged, this, [this](int row) {
        if (!rebuilding_ && row >= 0) {
            canvas_->setSheet(static_cast<std::size_t>(row));
            rebuildProperties();
        }
    });
    connect(list_, &QListWidget::itemDoubleClicked, this, [this](QListWidgetItem*) {
        const std::size_t at = currentSheet();
        const SheetSet& set = document_.sheetSet();
        if (at >= set.sheets.size()) {
            return;
        }
        bool ok = false;
        const QString name = QInputDialog::getText(
            this, QStringLiteral("Rename Sheet"), QStringLiteral("Name:"), QLineEdit::Normal,
            QString::fromStdString(set.sheets[at].name), &ok);
        if (ok) {
            (void)plotting::editSheet(document_, at, [&name](Sheet& sheet) {
                sheet.name = name.toStdString();
                return Status{};
            }, "RENAME_SHEET");
        }
    });

    // Properties.
    auto* propertiesDock = new QDockWidget(QStringLiteral("Properties"), this);
    propertiesDock->setObjectName(QStringLiteral("sheetPropertiesDock"));
    propertiesDock->setFeatures(QDockWidget::DockWidgetMovable);
    properties_ = new QScrollArea(propertiesDock);
    properties_->setWidgetResizable(true);
    properties_->setMinimumWidth(300);
    propertiesDock->setWidget(properties_);
    addDockWidget(Qt::RightDockWidgetArea, propertiesDock);

    // The preflight checks, with the painter's knowledge of the drawing.
    checks_ = new SheetChecksDock(
        document_, [this] { return checkSheetsFor(document_.sheetSet(), this->source()); }, this);
    checks_->onActivated = [this](const plotting::Finding& finding) { showFinding(finding); };
    addDockWidget(Qt::BottomDockWidgetArea, checks_);

    status_ = new QLabel(this);
    statusBar()->addWidget(status_, 1);

    buildActions();
    setUpEditing();

    canvas_->onSelectionChanged = [this](const std::string&) {
        rebuildProperties();
        updateEditActions();
    };
    canvas_->onContextMenu = [this](const QPointF& global, const std::string& id) {
        showContextMenu(global, id);
    };

    listener_ = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.selectionOnly()) {
            return;
        }
        refresh();
    });
    refresh();
}

SheetEditor::~SheetEditor() = default;

std::size_t SheetEditor::currentSheet() const { return canvas_->sheet(); }

void SheetEditor::setCurrentSheet(std::size_t index)
{
    if (index < document_.sheetSet().sheets.size()) {
        list_->setCurrentRow(static_cast<int>(index));
        canvas_->setSheet(index);
        rebuildProperties();
    }
}

void SheetEditor::report(const QString& text, bool error)
{
    statusBar()->showMessage(text, 8000);
    if (onMessage) {
        onMessage(text, error);
    }
}

VerbOutcome SheetEditor::runLine(const QString& line)
{
    VerbOutcome outcome;
    if (run_) {
        // The window echoes the line and logs the reply itself.
        outcome = run_(line);
    } else {
        if (onMessage) {
            onMessage("> " + line, false);
        }
        const auto tokens = katana::cad::CommandInterpreter::tokenize(line.toStdString());
        const katana::core::Result<std::string> reply =
            tokens ? plotting::runSheetVerb(document_, *tokens,
                                            [this] { return sheetVerbContextFor(document_, source_); })
                   : katana::core::Result<std::string>(tokens.error());
        outcome.ok = reply.ok();
        (outcome.ok ? outcome.reply : outcome.error) =
            QString::fromStdString(outcome.ok ? *reply : reply.error().describe());
        if (onMessage) {
            onMessage(outcome.ok ? outcome.reply : outcome.error, !outcome.ok);
        }
    }
    statusBar()->showMessage(firstLine(outcome.ok ? outcome.reply : outcome.error), 8000);
    if (!outcome.ok) {
        // A refused line changed nothing, so nothing redraws the panel: the
        // field that was typed into shows the stored value again.
        rebuildProperties();
    }
    return outcome;
}

void SheetEditor::buildActions()
{
    QToolBar* bar = addToolBar(QStringLiteral("Sheets"));
    bar->setObjectName(QStringLiteral("sheetToolBar"));
    bar->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);

    auto* undo = new QAction(icon(Icon::Undo), QStringLiteral("Undo"), this);
    undo->setObjectName(QStringLiteral("sheetUndo"));
    undo->setShortcut(QKeySequence::Undo);
    connect(undo, &QAction::triggered, this, [this] { (void)document_.undo(); });
    auto* redo = new QAction(icon(Icon::Redo), QStringLiteral("Redo"), this);
    redo->setObjectName(QStringLiteral("sheetRedo"));
    redo->setShortcut(QKeySequence::Redo);
    connect(redo, &QAction::triggered, this, [this] { (void)document_.redo(); });
    addAction(undo);
    addAction(redo);

    QAction* generate = bar->addAction(icon(Icon::Properties), QStringLiteral("Generate Sheets..."));
    generate->setObjectName(QStringLiteral("sheetGenerate"));
    generate->setToolTip(QStringLiteral("Lay sheets out for the drawing, an alignment or its sections"));
    // Opened, not waited on: a headless session fills it by its object names.
    connect(generate, &QAction::triggered, this, [this] { (void)openGenerateDialog(); });

    QAction* blank = bar->addAction(icon(Icon::New), QStringLiteral("New Sheet"));
    blank->setObjectName(QStringLiteral("sheetNewSheet"));
    connect(blank, &QAction::triggered, this, [this] { (void)addBlankSheet(); });

    auto* addMenu = new QMenu(this);
    for (const ViewportKind kind : kKinds) {
        QAction* a = addMenu->addAction(kindName(kind));
        a->setObjectName(QStringLiteral("sheetAddView_") + QString::fromUtf8(plotting::toString(kind)));
        connect(a, &QAction::triggered, this, [this, kind] {
            if (auto s = addViewport(kind); !s) {
                report(QString::fromStdString(s.error().describe()), true);
            }
        });
    }
    auto* addButton = new QToolButton(bar);
    addButton->setObjectName(QStringLiteral("sheetAddViewButton"));
    addButton->setText(QStringLiteral("Add View"));
    addButton->setIcon(icon(Icon::Rectangle));
    addButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    addButton->setPopupMode(QToolButton::InstantPopup);
    addButton->setMenu(addMenu);
    bar->addWidget(addButton);

    auto* tileMenu = new QMenu(this);
    for (std::size_t i = 0; i < plotting::kTilingPresetCount; ++i) {
        const auto preset = static_cast<TilingPreset>(i);
        QAction* a = tileMenu->addAction(QString::fromUtf8(plotting::presetName(preset).data(),
                                                           static_cast<qsizetype>(plotting::presetName(preset).size())));
        a->setObjectName(QStringLiteral("sheetTile_%1").arg(i));
        connect(a, &QAction::triggered, this, [this, preset] {
            if (auto s = tile(preset); !s) {
                report(QString::fromStdString(s.error().describe()), true);
            }
        });
    }
    auto* tileButton = new QToolButton(bar);
    tileButton->setObjectName(QStringLiteral("sheetTileButton"));
    tileButton->setText(QStringLiteral("Tile"));
    tileButton->setIcon(icon(Icon::Grid));
    tileButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    tileButton->setPopupMode(QToolButton::InstantPopup);
    tileButton->setMenu(tileMenu);
    bar->addWidget(tileButton);
    bar->addWidget(arrangeToolButton(bar, document_, *this));

    bar->addSeparator();
    QAction* titleBlock = bar->addAction(icon(Icon::Properties), QStringLiteral("Title Block..."));
    titleBlock->setObjectName(QStringLiteral("sheetTitleBlock"));
    titleBlock->setToolTip(
        QStringLiteral("The organisation, project, sign-offs, notes, revisions and logo every sheet shares"));
    connect(titleBlock, &QAction::triggered, this, [this] { editTitleBlock(); });

    bar->addSeparator();
    QAction* fit = bar->addAction(icon(Icon::ZoomExtents), QStringLiteral("Fit Page"));
    fit->setObjectName(QStringLiteral("sheetFitPage"));
    fit->setShortcut(QKeySequence(Qt::Key_Home));
    connect(fit, &QAction::triggered, this, [this] { canvas_->fitPage(); });

    bar->addSeparator();
    QAction* check = bar->addAction(checkSheetsIcon(), QStringLiteral("Check Sheets"));
    check->setObjectName(QStringLiteral("sheetCheck"));
    check->setToolTip(QStringLiteral("Find what a plot would get wrong: views off the paper or over "
                                     "nothing, sections off their alignment, blanks in the title block"));
    connect(check, &QAction::triggered, this, [this] { (void)checkSheets(); });
    QAction* plotOne = bar->addAction(icon(Icon::Plot), QStringLiteral("Plot Sheet..."));
    plotOne->setObjectName(QStringLiteral("sheetPlotSheet"));
    connect(plotOne, &QAction::triggered, this, [this] { plotInteractive(false); });
    QAction* plotAll = bar->addAction(icon(Icon::Plot), QStringLiteral("Plot All..."));
    plotAll->setObjectName(QStringLiteral("sheetPlotAll"));
    plotAll->setShortcut(QKeySequence::Print);
    connect(plotAll, &QAction::triggered, this, [this] { plotInteractive(true); });
}

void SheetEditor::refresh()
{
    rebuildList();
    canvas_->invalidate();
    rebuildProperties();
    const SheetSet& set = document_.sheetSet();
    QString text = QString("%1 sheet%2").arg(set.sheets.size()).arg(set.sheets.size() == 1 ? "" : "s");
    if (const Status status = document_.sheetSetStatus(); !status) {
        text += "  -  " + QString::fromStdString(status.error().describe());
    }
    status_->setText(text);
    checks_->schedule();
}

void SheetEditor::rebuildList()
{
    rebuilding_ = true;
    const SheetSet& set = document_.sheetSet();
    const std::size_t keep = std::min(canvas_->sheet(), set.sheets.empty() ? 0 : set.sheets.size() - 1);
    list_->clear();
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        const Sheet& sheet = set.sheets[i];
        const std::string number =
            plotting::formatSheetNumber(set.numbering, i + 1, set.sheets.size(), set.defaults.setNumber);
        list_->addItem(QString("%1   %2\n%3 %4")
                           .arg(QString::fromStdString(number),
                                QString::fromStdString(sheet.name), paperName(sheet.paper),
                                sheet.landscape ? QStringLiteral("landscape")
                                                : QStringLiteral("portrait")));
    }
    if (!set.sheets.empty()) {
        list_->setCurrentRow(static_cast<int>(keep));
    }
    rebuilding_ = false;
    canvas_->setSheet(keep);
    refreshThumbnails();
}

void SheetEditor::rebuildProperties()
{
    auto* panel = new QWidget();
    auto* layout = new QVBoxLayout(panel);
    const SheetSet& set = document_.sheetSet();
    const std::size_t index = currentSheet();
    if (index >= set.sheets.size()) {
        layout->addWidget(new QLabel(QStringLiteral("No sheet."), panel));
        layout->addStretch(1);
        properties_->setWidget(panel);
        return;
    }
    const Sheet& sheet = set.sheets[index];
    const Viewport* selected = nullptr;
    for (const Viewport& v : sheet.viewports) {
        if (v.id == canvas_->selected()) {
            selected = &v;
        }
    }

    if (selected != nullptr) {
        const std::string id = selected->id;
        const Viewport& v = *selected;
        const auto edit = [this, id](auto&& change, const char* step = "EDIT_VIEWPORT") {
            if (auto s = plotting::editViewport(
                    document_, id,
                    [&change](Viewport& edited) {
                        change(edited);
                        return Status{};
                    },
                    step);
                !s) {
                report(QString::fromStdString(s.error().describe()), true);
            }
        };
        if (const std::size_t count = canvas_->selectedIds().size(); count > 1) {
            // Short lines, wrapped: the panel may be narrow.
            auto* group = new QLabel(QString("%1 views selected.\nThe properties are %2's.")
                                         .arg(count)
                                         .arg(QString::fromStdString(id)),
                                     panel);
            group->setObjectName(QStringLiteral("sheetSelectionNote"));
            group->setToolTip(QStringLiteral("A drag, the arrows, Delete, Cut, Copy and Duplicate act on "
                                             "every selected view"));
            group->setWordWrap(true);
            layout->addWidget(group);
        }
        auto* box = new QGroupBox(kindName(v.kind) + QString("  (%1)").arg(QString::fromStdString(v.id)), panel);
        auto* form = new QFormLayout(box);

        auto* title = new QLineEdit(QString::fromStdString(v.title), box);
        title->setPlaceholderText(QString::fromStdString(plotting::automaticTitle(v)));
        connect(title, &QLineEdit::editingFinished, this, [edit, title] {
            edit([t = title->text().toStdString()](Viewport& e) { e.title = t; });
        });
        form->addRow(QStringLiteral("Title"), title);

        const bool plan = v.kind == ViewportKind::Plan || v.kind == ViewportKind::KeyPlan;
        const bool section = v.kind == ViewportKind::LongSection || v.kind == ViewportKind::CrossSections;
        // An automatic section's exaggeration is the fitted one, shown as
        // such and kept when a fixed scale is chosen, so choosing one does
        // not redraw the section at the stale stored exaggeration.
        std::optional<double> fittedExaggeration;
        if (plan || section) {
            auto* scale = scaleBox(box, true);
            scale->setObjectName(QStringLiteral("sheetViewportScale"));
            if (plan && v.autoScale) {
                scale->setCurrentIndex(0);
                const ResolvedViewport resolved = resolvePlanViewport(v, source(), set, index);
                scale->setToolTip(QString("Automatic: now %1").arg(scaleLabel(resolved.scale)));
            } else if (v.autoScale) {
                // A section fits both its scale and its exaggeration.
                scale->setCurrentIndex(0);
                Viewport fitted = v;
                const plotting::SectionFit fit =
                    resolveSectionViewport(v, source(), canvas_->paintCache());
                fitted.scale = fit.scale;
                fitted.verticalExaggeration = fit.exaggeration;
                fittedExaggeration = fit.exaggeration;
                scale->setToolTip(
                    QString("Automatic: now %1").arg(QString::fromStdString(plotting::scaleText(fitted))));
            } else {
                scale->setCurrentText(scaleLabel(v.scale));
            }
            connect(scale, &QComboBox::textActivated, this,
                    [edit, scale, fittedExaggeration](const QString& text) {
                if (text == QStringLiteral("Auto")) {
                    edit([](Viewport& e) { e.autoScale = true; });
                } else if (auto value = parseScale(text)) {
                    edit([value, fittedExaggeration](Viewport& e) {
                        e.scale = *value;
                        if (e.autoScale && fittedExaggeration) {
                            e.verticalExaggeration = *fittedExaggeration;
                        }
                        e.autoScale = false;
                    });
                } else {
                    scale->setCurrentIndex(0);
                }
            });
            form->addRow(section ? QStringLiteral("Horizontal scale") : QStringLiteral("Scale"), scale);
        }
        if (plan || v.kind == ViewportKind::Model3D) {
            auto* rotation = spin(box, -360.0, 360.0, v.rotation * katana::math::kRadToDeg, 3,
                                  QStringLiteral(" °"));
            rotation->setToolTip(QStringLiteral("The world direction that runs left to right across the paper, "
                                                "counter-clockwise from east"));
            connect(rotation, &QDoubleSpinBox::valueChanged, this, [edit](double degrees) {
                edit([degrees](Viewport& e) { e.rotation = degrees * katana::math::kDegToRad; });
            });
            form->addRow(QStringLiteral("Rotation"), rotation);
        }
        if (plan || section) {
            auto* autoCentre = new QCheckBox(QStringLiteral("Centre on what it shows"), box);
            autoCentre->setChecked(v.autoCentre);
            connect(autoCentre, &QCheckBox::toggled, this, [edit](bool on) {
                edit([on](Viewport& e) { e.autoCentre = on; });
            });
            form->addRow(QString(), autoCentre);
            auto* cx = spin(box, -1e9, 1e9, v.centre.x, 3);
            auto* cy = spin(box, -1e9, 1e9, v.centre.y, 3);
            cx->setEnabled(!v.autoCentre);
            cy->setEnabled(!v.autoCentre);
            connect(cx, &QDoubleSpinBox::valueChanged, this,
                    [edit](double x) { edit([x](Viewport& e) { e.centre.x = x; }); });
            connect(cy, &QDoubleSpinBox::valueChanged, this,
                    [edit](double y) { edit([y](Viewport& e) { e.centre.y = y; }); });
            form->addRow(plan ? QStringLiteral("Centre E") : QStringLiteral("Centre (ch/offset)"), cx);
            form->addRow(plan ? QStringLiteral("Centre N") : QStringLiteral("Centre level"), cy);
        }
        if (section) {
            auto* ve = spin(box, 0.1, 100.0, fittedExaggeration.value_or(v.verticalExaggeration), 2,
                            QStringLiteral(" x"));
            ve->setObjectName(QStringLiteral("sheetViewportExaggeration"));
            ve->setEnabled(!v.autoScale);
            if (v.autoScale) {
                ve->setToolTip(QStringLiteral("Chosen with the automatic scale"));
            }
            connect(ve, &QDoubleSpinBox::valueChanged, this, [edit](double x) {
                edit([x](Viewport& e) { e.verticalExaggeration = x; });
            });
            form->addRow(QStringLiteral("Vertical exaggeration"), ve);
        }
        if (section || plan) {
            auto* alignment = new QComboBox(box);
            alignment->addItem(plan ? QStringLiteral("(the drawing)") : QStringLiteral("(none)"));
            for (const std::string& name : alignmentNames(document_)) {
                alignment->addItem(QString::fromStdString(name));
            }
            alignment->setCurrentText(QString::fromStdString(v.source.alignment));
            if (v.source.alignment.empty()) {
                alignment->setCurrentIndex(0);
            }
            connect(alignment, &QComboBox::currentIndexChanged, this, [edit, alignment](int i) {
                const std::string name = i == 0 ? std::string{} : alignment->currentText().toStdString();
                edit([name](Viewport& e) { e.source.alignment = name; });
            });
            form->addRow(QStringLiteral("Alignment"), alignment);
        }
        // The fields added for the verbs run the VIEW SET line a person would
        // type, queued: the line rebuilds this panel, and with it the field
        // that is still signalling.
        const auto viewSet = [this, id](const QString& options) {
            const QString line = QString("VIEW SET %1 %2").arg(QString::fromStdString(id), options);
            QMetaObject::invokeMethod(this, [this, line] { (void)runLine(line); }, Qt::QueuedConnection);
        };
        const bool crossSections = v.kind == ViewportKind::CrossSections;
        if (v.kind == ViewportKind::LongSection || crossSections ||
            (plan && !v.source.alignment.empty())) {
            auto* from = spin(box, -1e9, 1e9, v.source.chainageFrom, 3);
            auto* to = spin(box, -1e9, 1e9, v.source.chainageTo, 3);
            from->setObjectName(QStringLiteral("sheetChainageFrom"));
            to->setObjectName(QStringLiteral("sheetChainageTo"));
            from->setToolTip(crossSections
                                 ? QStringLiteral("The chainages cut every interval run from here")
                                 : QStringLiteral("From equal to To: the whole alignment"));
            to->setToolTip(crossSections ? QStringLiteral("...to here") : from->toolTip());
            // What the box shows, rounded: a box left as it was writes nothing.
            const double shownFrom = from->value();
            const double shownTo = to->value();
            connect(from, &QDoubleSpinBox::editingFinished, this, [viewSet, from, shownFrom] {
                if (from->value() != shownFrom) {
                    viewSet("from=" + verbNumber(from->value()));
                }
            });
            connect(to, &QDoubleSpinBox::editingFinished, this, [viewSet, to, shownTo] {
                if (to->value() != shownTo) {
                    viewSet("to=" + verbNumber(to->value()));
                }
            });
            form->addRow(QStringLiteral("Chainage from"), from);
            form->addRow(QStringLiteral("Chainage to"), to);
        }
        if (crossSections) {
            // Cut at the chainages listed, or every so many metres from one
            // chainage to another: the painter cuts the list when there is
            // one (plotting::viewportStations), so Every clears it, and cuts
            // an interval only over a range, so Every with none sets the
            // alignment's whole length.
            QStringList stations;
            for (const double s : v.source.stations) {
                stations << QString::number(s, 'f', 3);
            }
            const bool byInterval = v.source.stations.empty() && v.source.sectionInterval > 0.0;
            auto* atChainages = new QRadioButton(QStringLiteral("At chainages"), box);
            atChainages->setObjectName(QStringLiteral("sheetSectionsAtChainages"));
            atChainages->setChecked(!byInterval);
            auto* list = new QLineEdit(stations.join(QStringLiteral(", ")), box);
            list->setObjectName(QStringLiteral("sheetSectionStations"));
            list->setPlaceholderText(QStringLiteral("e.g. 20, 40, 60"));
            list->setEnabled(!byInterval);
            auto* every = new QRadioButton(QStringLiteral("Every"), box);
            every->setObjectName(QStringLiteral("sheetSectionsEvery"));
            every->setChecked(byInterval);
            auto* interval = spin(box, 0.5, 10000.0, byInterval ? v.source.sectionInterval : 20.0, 2,
                                  QStringLiteral(" m"));
            interval->setObjectName(QStringLiteral("sheetSectionInterval"));
            interval->setEnabled(byInterval);
            const auto listed = [this, viewSet, list] {
                const auto typed = stationList(list->text());
                if (!typed) {
                    report(QStringLiteral("Chainages are numbers separated by commas: ") + list->text(), true);
                    return;
                }
                viewSet("interval=0 stations=" + (typed->isEmpty() ? QStringLiteral("\"\"") : *typed));
            };
            connect(list, &QLineEdit::editingFinished, this, [listed, list, text = list->text()] {
                if (list->text() != text) {
                    listed();
                }
            });
            connect(atChainages, &QRadioButton::clicked, this, [listed, byInterval] {
                if (byInterval) {
                    listed();
                }
            });
            std::optional<std::pair<double, double>> whole;
            if (const auto* a = document_.model().alignments.find(v.source.alignment)) {
                if (auto solved = katana::geometry::solveAlignment(a->horizontal)) {
                    whole = std::pair{solved->startStation(), solved->endStation()};
                }
            }
            const bool ranged = v.source.chainageTo > v.source.chainageFrom;
            connect(every, &QRadioButton::clicked, this, [viewSet, interval, byInterval, ranged, whole] {
                if (byInterval) {
                    return;
                }
                QString options = "stations=\"\" interval=" + verbNumber(interval->value());
                if (!ranged && whole) {
                    options += " from=" + verbNumber(whole->first) + " to=" + verbNumber(whole->second);
                }
                viewSet(options);
            });
            const double shownInterval = interval->value();
            connect(interval, &QDoubleSpinBox::editingFinished, this, [viewSet, interval, shownInterval] {
                if (interval->value() != shownInterval) {
                    viewSet("interval=" + verbNumber(interval->value()));
                }
            });
            form->addRow(atChainages, list);
            form->addRow(every, interval);
            auto* half = spin(box, 0.5, 1000.0, v.source.sectionHalfWidth > 0.0 ? v.source.sectionHalfWidth : 20.0,
                              2, QStringLiteral(" m"));
            connect(half, &QDoubleSpinBox::valueChanged, this, [edit](double w) {
                edit([w](Viewport& e) { e.source.sectionHalfWidth = w; });
            });
            form->addRow(QStringLiteral("Half width"), half);
        }
        if (v.kind == ViewportKind::Model3D) {
            auto* tilt = spin(box, 1.0, 89.0, v.tiltDegrees, 1, QStringLiteral(" °"));
            connect(tilt, &QDoubleSpinBox::valueChanged, this,
                    [edit](double t) { edit([t](Viewport& e) { e.tiltDegrees = t; }); });
            form->addRow(QStringLiteral("Camera height angle"), tilt);
        }
        if (plan) {
            auto* north = new QCheckBox(QStringLiteral("North arrow"), box);
            north->setChecked(v.northArrow);
            connect(north, &QCheckBox::toggled, this,
                    [edit](bool on) { edit([on](Viewport& e) { e.northArrow = on; }); });
            auto* bar = new QCheckBox(QStringLiteral("Scale bar"), box);
            bar->setChecked(v.scaleBar);
            connect(bar, &QCheckBox::toggled, this,
                    [edit](bool on) { edit([on](Viewport& e) { e.scaleBar = on; }); });
            form->addRow(QString(), north);
            form->addRow(QString(), bar);
            addGridRows(*form, box, v,
                        plotting::automaticGridInterval(
                            resolvePlanViewport(v, source(), set, index).scale),
                        [this, id](plotting::GridStyle style, double interval) {
                            // Once the list or box that asked has finished
                            // signalling: the edit rebuilds this panel, them with it.
                            QMetaObject::invokeMethod(
                                this,
                                [this, id, style, interval] {
                                    if (auto s = plotting::setPlanGrid(document_, id, style, interval);
                                        !s) {
                                        report(QString::fromStdString(s.error().describe()), true);
                                    }
                                },
                                Qt::QueuedConnection);
                        });
        }
        // The painter leaves a section's hidden layers out as it does a
        // plan's (section_painter.cpp), so both offer the list.
        if (plan || section) {
            auto* layers = new QPushButton(
                QString("Hidden layers (%1)...").arg(v.hiddenLayers.size()), box);
            layers->setObjectName(QStringLiteral("sheetViewportHiddenLayers"));
            connect(layers, &QPushButton::clicked, this,
                    [this, id, hidden = v.hiddenLayers] { chooseHiddenLayers(id, hidden); });
            form->addRow(QString(), layers);
        }
        if (v.kind == ViewportKind::Notes) {
            auto* text = new QPlainTextEdit(QString::fromStdString(v.text), box);
            text->setMinimumHeight(140);
            auto* apply = new QPushButton(QStringLiteral("Apply text"), box);
            connect(apply, &QPushButton::clicked, this, [edit, text] {
                edit([t = text->toPlainText().toStdString()](Viewport& e) { e.text = t; });
            });
            form->addRow(QStringLiteral("Text"), text);
            form->addRow(QString(), apply);
        }
        if (v.kind == ViewportKind::Revisions) {
            auto* limit = new QSpinBox(box);
            limit->setObjectName(QStringLiteral("sheetRevisionLimit"));
            limit->setRange(0, 999);
            limit->setSpecialValueText(QStringLiteral("All"));
            limit->setValue(static_cast<int>(std::min<std::size_t>(v.revisionLimit, 999)));
            limit->setToolTip(QStringLiteral("Only the newest so many revisions; All shows every one"));
            // One step per value settled on, not per digit typed; queued, as
            // the edit rebuilds this panel and so deletes the spin box.
            limit->setKeyboardTracking(false);
            connect(
                limit, &QSpinBox::valueChanged, this,
                [edit](int n) {
                    edit([n](Viewport& e) { e.revisionLimit = static_cast<std::size_t>(n); },
                         "VIEWPORT_REVISION_LIMIT");
                },
                Qt::QueuedConnection);
            form->addRow(QStringLiteral("Newest revisions"), limit);
        }
        if (v.kind == ViewportKind::Image) {
            auto* row = new QWidget(box);
            auto* rowLayout = new QHBoxLayout(row);
            rowLayout->setContentsMargins(0, 0, 0, 0);
            auto* file = new QLineEdit(QString::fromStdString(v.text), row);
            file->setObjectName(QStringLiteral("sheetImageName"));
            file->setPlaceholderText(QStringLiteral("a file in the project's assets folder"));
            connect(file, &QLineEdit::editingFinished, this, [edit, file] {
                edit([t = file->text().toStdString()](Viewport& e) { e.text = t; });
            });
            // Another picture: copied into the project's assets as VIEW SET
            // file= copies it (importImageAsset, up to 32 MB), and named in
            // the same step.
            auto* browse = new QPushButton(QStringLiteral("Browse..."), row);
            browse->setObjectName(QStringLiteral("sheetImageBrowse"));
            browse->setToolTip(QStringLiteral("Choose the picture: it is copied into the project's assets"));
            connect(browse, &QPushButton::clicked, this, [this, id, viewSet] {
                const QString line = QString("VIEW SET %1 file=\"path\"").arg(QString::fromStdString(id));
                if (headless_) {
                    report("Browse opens no file dialog in a headless session: type " + line, true);
                    return;
                }
                const QString chosen = QFileDialog::getOpenFileName(
                    this, QStringLiteral("Picture for the View"),
                    QString::fromStdString(source().assets.string()),
                    QStringLiteral("Images (*.png *.jpg *.jpeg *.bmp *.gif)"));
                if (!chosen.isEmpty()) {
                    viewSet("file=" + quoted(chosen));
                }
            });
            rowLayout->addWidget(file, 1);
            rowLayout->addWidget(browse);
            form->addRow(QStringLiteral("Image"), row);
        }
        if (v.kind == ViewportKind::Legend) {
            addLegendProperties(*form, document_, set, index, v, source(),
                                [this](const QString& text, bool error) { report(text, error); });
        }
        auto* locked = new QCheckBox(QStringLiteral("Locked (tiling and dragging leave it)"), box);
        locked->setChecked(v.locked);
        connect(locked, &QCheckBox::toggled, this,
                [edit](bool on) { edit([on](Viewport& e) { e.locked = on; }); });
        form->addRow(QString(), locked);

        // Where the view is on the paper, in paper millimetres from its
        // lower-left corner: typed, as VIEW SET rect= takes it, and refused
        // as a typed rectangle is when it misses the paper.
        auto* place = new QGroupBox(QStringLiteral("On the paper"), panel);
        auto* placeForm = new QFormLayout(place);
        auto* rx = spin(place, -10000.0, 10000.0, v.rect.min.x, 1, QStringLiteral(" mm"));
        auto* ry = spin(place, -10000.0, 10000.0, v.rect.min.y, 1, QStringLiteral(" mm"));
        auto* rw = spin(place, 1.0, 10000.0, v.rect.width(), 1, QStringLiteral(" mm"));
        auto* rh = spin(place, 1.0, 10000.0, v.rect.height(), 1, QStringLiteral(" mm"));
        rx->setObjectName(QStringLiteral("sheetViewportX"));
        ry->setObjectName(QStringLiteral("sheetViewportY"));
        rw->setObjectName(QStringLiteral("sheetViewportW"));
        rh->setObjectName(QStringLiteral("sheetViewportH"));
        rx->setToolTip(QStringLiteral("The left edge, from the paper's left edge"));
        ry->setToolTip(QStringLiteral("The bottom edge, from the paper's bottom edge"));
        placeForm->addRow(QStringLiteral("Left"), rx);
        placeForm->addRow(QStringLiteral("Bottom"), ry);
        placeForm->addRow(QStringLiteral("Width"), rw);
        placeForm->addRow(QStringLiteral("Height"), rh);
        // A box left as it was keeps its exact value, not its rounding to
        // the tenth it shows; and nothing changed writes nothing.
        const std::array<double, 4> shown{rx->value(), ry->value(), rw->value(), rh->value()};
        const std::array<double, 4> exact{v.rect.min.x, v.rect.min.y, v.rect.width(), v.rect.height()};
        const auto placeView = [viewSet, rx, ry, rw, rh, shown, exact] {
            const std::array<double, 4> now{rx->value(), ry->value(), rw->value(), rh->value()};
            if (now == shown) {
                return;
            }
            std::array<double, 4> value{};
            for (std::size_t i = 0; i < value.size(); ++i) {
                value[i] = now[i] == shown[i] ? exact[i] : now[i];
            }
            viewSet(QString("rect=%1,%2,%3,%4")
                        .arg(verbNumber(value[0]), verbNumber(value[1]),
                             verbNumber(value[0] + value[2]), verbNumber(value[1] + value[3])));
        };
        for (QDoubleSpinBox* field : {rx, ry, rw, rh}) {
            connect(field, &QDoubleSpinBox::editingFinished, this, placeView);
        }
        layout->addWidget(box);
        layout->addWidget(place);

        for (const std::string& problem : canvas_->lastStats().problems) {
            if (problem.starts_with(id + ":")) {
                auto* warn = new QLabel(QString::fromStdString(problem.substr(id.size() + 2)), panel);
                warn->setWordWrap(true);
                warn->setStyleSheet(QStringLiteral("color: #d05050;"));
                layout->addWidget(warn);
            }
        }
    } else {
        // The sheet itself.
        const auto editThis = [this, index](auto&& change, const char* step = "EDIT_SHEET") {
            if (auto s = plotting::editSheet(
                    document_, index,
                    [&change](Sheet& edited) {
                        change(edited);
                        return Status{};
                    },
                    step);
                !s) {
                report(QString::fromStdString(s.error().describe()), true);
            }
        };
        auto* box = new QGroupBox(QStringLiteral("Sheet"), panel);
        auto* form = new QFormLayout(box);
        auto* name = new QLineEdit(QString::fromStdString(sheet.name), box);
        connect(name, &QLineEdit::editingFinished, this, [editThis, name] {
            editThis([n = name->text().toStdString()](Sheet& s) { s.name = n; }, "RENAME_SHEET");
        });
        form->addRow(QStringLiteral("Name"), name);
        auto* paper = new QComboBox(box);
        for (const auto p : kPapers) {
            paper->addItem(paperName(p));
        }
        paper->setCurrentIndex(static_cast<int>(sheet.paper));
        connect(paper, &QComboBox::currentIndexChanged, this, [editThis](int i) {
            editThis([i](Sheet& s) { s.paper = kPapers[static_cast<std::size_t>(i)]; });
        });
        form->addRow(QStringLiteral("Paper"), paper);
        auto* orientation = new QComboBox(box);
        orientation->addItems({QStringLiteral("Landscape"), QStringLiteral("Portrait")});
        orientation->setCurrentIndex(sheet.landscape ? 0 : 1);
        connect(orientation, &QComboBox::currentIndexChanged, this, [editThis](int i) {
            editThis([i](Sheet& s) { s.landscape = i == 0; });
        });
        form->addRow(QStringLiteral("Orientation"), orientation);
        auto* frame = new QCheckBox(QStringLiteral("Frame and title block"), box);
        frame->setChecked(!sheet.frame.empty());
        frame->setToolTip(QStringLiteral("A portrait sheet has no frame: the title block is laid out for landscape"));
        connect(frame, &QCheckBox::toggled, this, [editThis](bool on) {
            editThis([on](Sheet& s) { s.frame = on ? std::string(plotting::kBuiltInFrameId) : std::string{}; });
        });
        form->addRow(QString(), frame);
        auto* legend = new QCheckBox(QStringLiteral("Utility legend block"), box);
        legend->setChecked(sheet.frameLegend);
        connect(legend, &QCheckBox::toggled, this,
                [editThis](bool on) { editThis([on](Sheet& s) { s.frameLegend = on; }); });
        form->addRow(QString(), legend);
        form->addRow(QString(), choosePaperButton(box, document_, *this));
        layout->addWidget(box);

        // Title-block values: what each field prints, and what this sheet overrides.
        auto* fieldsBox = new QGroupBox(QStringLiteral("Title block on this sheet"), panel);
        auto* fieldsLayout = new QVBoxLayout(fieldsBox);
        auto* note = new QLabel(QStringLiteral("A value here is for this sheet only. Values every sheet "
                                               "shares are set with Title Block."),
                                fieldsBox);
        note->setWordWrap(true);
        fieldsLayout->addWidget(note);
        std::set<std::string> names;
        if (auto f = sheetFrame(sheet)) {
            for (const auto& text : f->texts) {
                names.insert(text.fields.begin(), text.fields.end());
            }
        }
        const SheetSource src = source();
        const auto resolved = plotting::resolveFields(set, index, src.fields);
        auto* table = new QTableWidget(static_cast<int>(names.size()), 2, fieldsBox);
        table->setObjectName(QStringLiteral("sheetFieldTable"));
        table->setHorizontalHeaderLabels({QStringLiteral("Field"), QStringLiteral("Prints")});
        table->horizontalHeader()->setStretchLastSection(true);
        table->verticalHeader()->setVisible(false);
        int row = 0;
        for (const std::string& field : names) {
            auto* key = new QTableWidgetItem(QString::fromStdString(field));
            key->setFlags(key->flags() & ~Qt::ItemIsEditable);
            table->setItem(row, 0, key);
            const auto it = resolved.find(field);
            auto* value = new QTableWidgetItem(
                QString::fromStdString(it != resolved.end() ? it->second : std::string{}));
            if (sheet.fields.contains(field)) {
                QFont bold = value->font();
                bold.setBold(true);
                value->setFont(bold);
            }
            table->setItem(row, 1, value);
            ++row;
        }
        table->resizeColumnToContents(0);
        table->setMinimumHeight(260);
        connect(table, &QTableWidget::itemChanged, this, [this, editThis, table, resolved](QTableWidgetItem* item) {
            if (rebuilding_ || item->column() != 1) {
                return;
            }
            const std::string field = table->item(item->row(), 0)->text().toStdString();
            const std::string value = item->text().toStdString();
            const auto it = resolved.find(field);
            if (it != resolved.end() && it->second == value) {
                return;
            }
            editThis([field, value](Sheet& s) {
                if (value.empty()) {
                    s.fields.erase(field);
                } else {
                    s.fields[field] = value;
                }
            }, "SHEET_FIELD");
        });
        fieldsLayout->addWidget(table);
        auto* clear = new QPushButton(QStringLiteral("Clear this sheet's values"), fieldsBox);
        clear->setEnabled(!sheet.fields.empty());
        connect(clear, &QPushButton::clicked, this,
                [editThis] { editThis([](Sheet& s) { s.fields.clear(); }, "SHEET_FIELD"); });
        fieldsLayout->addWidget(clear);
        layout->addWidget(fieldsBox);

        for (const std::string& problem : canvas_->lastStats().problems) {
            auto* warn = new QLabel(QString::fromStdString(problem), panel);
            warn->setWordWrap(true);
            warn->setStyleSheet(QStringLiteral("color: #d05050;"));
            layout->addWidget(warn);
        }
    }
    layout->addStretch(1);
    rebuilding_ = true;
    properties_->setWidget(panel);
    rebuilding_ = false;
}

void SheetEditor::chooseHiddenLayers(const std::string& id, const katana::cad::LayerOverrides& hidden)
{
    auto* dialog = new QDialog(this);
    dialog->setObjectName(QStringLiteral("sheetHiddenLayersDialog"));
    dialog->setWindowTitle(QStringLiteral("Layers Shown in This View"));
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    auto* l = new QVBoxLayout(dialog);
    auto* listWidget = new QListWidget(dialog);
    listWidget->setObjectName(QStringLiteral("sheetHiddenLayersList"));
    for (const auto& layer : document_.model().layers.all()) {
        auto* item = new QListWidgetItem(QString::fromStdString(layer.name), listWidget);
        item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
        item->setCheckState(hidden.hidesDirectly(layer.name) ? Qt::Unchecked : Qt::Checked);
    }
    l->addWidget(listWidget);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dialog);
    buttons->button(QDialogButtonBox::Ok)->setObjectName(QStringLiteral("sheetHiddenLayersOk"));
    connect(buttons, &QDialogButtonBox::accepted, dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    l->addWidget(buttons);
    // Only what the ticks changed, a layer an option: an entry the view
    // hides that is no layer of its own (a parent path) is left as it is.
    connect(dialog, &QDialog::accepted, this, [this, id, hidden, listWidget] {
        QStringList options;
        for (int i = 0; i < listWidget->count(); ++i) {
            const QString name = listWidget->item(i)->text();
            const bool hide = listWidget->item(i)->checkState() != Qt::Checked;
            if (hide != hidden.hidesDirectly(name.toStdString())) {
                options << (hide ? "hide=" : "show=") + quoted(name);
            }
        }
        if (!options.isEmpty()) {
            (void)runLine(QString("VIEW SET %1 %2").arg(QString::fromStdString(id), options.join(' ')));
        }
    });
    dialog->open();
}

void SheetEditor::showContextMenu(const QPointF& global, const std::string& viewportId)
{
    QMenu menu(this);
    QMenu* arrange = menu.addMenu(QStringLiteral("Arrange"));
    arrange->setObjectName(QStringLiteral("sheetContextArrangeMenu"));
    fillArrangeMenu(*arrange, document_, *this);
    menu.addSeparator();
    if (!viewportId.empty()) {
        const std::size_t index = currentSheet();
        QAction* front = menu.addAction(QStringLiteral("Bring to Front"), this, [this, index, viewportId] {
            (void)plotting::editSheet(document_, index, [&viewportId](Sheet& s) {
                auto it = std::find_if(s.viewports.begin(), s.viewports.end(),
                                       [&](const Viewport& v) { return v.id == viewportId; });
                if (it != s.viewports.end()) {
                    std::rotate(it, it + 1, s.viewports.end());
                }
                return Status{};
            }, "ORDER_VIEWPORT");
        });
        front->setObjectName(QStringLiteral("sheetBringToFront"));
        QAction* back = menu.addAction(QStringLiteral("Send to Back"), this, [this, index, viewportId] {
            (void)plotting::editSheet(document_, index, [&viewportId](Sheet& s) {
                auto it = std::find_if(s.viewports.begin(), s.viewports.end(),
                                       [&](const Viewport& v) { return v.id == viewportId; });
                if (it != s.viewports.end()) {
                    std::rotate(s.viewports.begin(), it, it + 1);
                }
                return Status{};
            }, "ORDER_VIEWPORT");
        });
        back->setObjectName(QStringLiteral("sheetSendToBack"));
        QAction* fill = menu.addAction(QStringLiteral("Fill the Drawing Area"), this, [this, viewportId] {
            const Sheet& sheet = document_.sheetSet().sheets[currentSheet()];
            const Box2 area = plotting::tilingArea(sheet);
            (void)plotting::editViewport(document_, viewportId, [area](Viewport& v) {
                v.rect = area;
                return Status{};
            }, "RESIZE_VIEWPORT");
        });
        fill->setObjectName(QStringLiteral("sheetFillDrawingArea"));
        menu.addSeparator();
        // The Edit and View menus' own actions, shortcuts and all.
        for (const char* name : {"sheetCut", "sheetCopy", "sheetDuplicateViews", "sheetZoomSelection"}) {
            if (QAction* a = findChild<QAction*>(QString::fromLatin1(name))) {
                menu.addAction(a);
            }
        }
        menu.addSeparator();
        if (QAction* a = findChild<QAction*>(QStringLiteral("sheetDeleteViews"))) {
            menu.addAction(a);
        }
    } else {
        QMenu* add = menu.addMenu(QStringLiteral("Add View"));
        for (const ViewportKind kind : kKinds) {
            add->addAction(kindName(kind), this, [this, kind] { (void)addViewport(kind); })
                ->setObjectName(QStringLiteral("sheetMenuAddView_") +
                                QString::fromUtf8(plotting::toString(kind)));
        }
        QMenu* tiles = menu.addMenu(QStringLiteral("Tile"));
        for (std::size_t i = 0; i < plotting::kTilingPresetCount; ++i) {
            const auto preset = static_cast<TilingPreset>(i);
            const auto name = plotting::presetName(preset);
            tiles->addAction(QString::fromUtf8(name.data(), static_cast<qsizetype>(name.size())), this,
                             [this, preset] { (void)tile(preset); })
                ->setObjectName(QStringLiteral("sheetMenuTile_%1").arg(i));
        }
        menu.addSeparator();
        for (const char* name : {"sheetPaste", "sheetSelectAll", "sheetFitPage"}) {
            if (QAction* a = findChild<QAction*>(QString::fromLatin1(name))) {
                menu.addAction(a);
            }
        }
    }
    menu.exec(global.toPoint());
}

Status SheetEditor::addBlankSheet()
{
    const SheetSet& set = document_.sheetSet();
    Sheet sheet = plotting::blankSheet({}, std::format("SHEET {}", set.sheets.size() + 1));
    sheet.id = plotting::nextSheetId(set);
    const std::size_t at = set.sheets.empty() ? 0 : currentSheet() + 1;
    if (auto s = plotting::addSheet(document_, std::move(sheet), at); !s) {
        return s;
    }
    setCurrentSheet(at);
    return {};
}

Status SheetEditor::addViewport(ViewportKind kind)
{
    if (document_.sheetSet().sheets.empty()) {
        if (auto s = addBlankSheet(); !s) {
            return s;
        }
    }
    const SheetSet& set = document_.sheetSet();
    const std::size_t index = currentSheet();
    const Sheet& sheet = set.sheets[index];

    Viewport v;
    v.id = plotting::nextViewportId(set);
    v.kind = kind;
    const std::vector<std::string> alignments = alignmentNames(document_);
    switch (kind) {
    case ViewportKind::Plan:
        v.rect = freePlace(sheet, 200.0, 140.0);
        v.autoScale = true;
        v.autoCentre = true;
        v.northArrow = true;
        v.scaleBar = true;
        break;
    case ViewportKind::KeyPlan: {
        v.rect = freePlace(sheet, 90.0, 70.0);
        v.autoScale = true;
        v.autoCentre = true;
        v.northArrow = true;
        // Its outlines are the sheets' plans as they are when it is drawn
        // (key_plan.hpp), numbered as the set numbers them then; nothing is
        // stored to go stale.
        break;
    }
    case ViewportKind::LongSection:
        v.rect = freePlace(sheet, 380.0, 110.0);
        v.scale = 500.0;
        v.verticalExaggeration = 10.0;
        v.autoScale = true; // the scale and exaggeration fitted to what it shows
        v.autoCentre = true;
        if (!alignments.empty()) {
            v.source.alignment = alignments.front();
        }
        break;
    case ViewportKind::CrossSections:
        v.rect = freePlace(sheet, 180.0, 110.0);
        v.scale = 200.0;
        v.autoScale = true;
        v.autoCentre = true;
        v.source.sectionHalfWidth = 20.0;
        if (!alignments.empty()) {
            v.source.alignment = alignments.front();
            if (const auto* a = document_.model().alignments.find(alignments.front())) {
                if (auto solved = katana::geometry::solveAlignment(a->horizontal)) {
                    v.source.stations = {(solved->startStation() + solved->endStation()) / 2.0};
                }
            }
        }
        break;
    case ViewportKind::Model3D:
        v.rect = freePlace(sheet, 160.0, 110.0);
        break;
    case ViewportKind::Legend:
        v.rect = freePlace(sheet, 80.0, 100.0);
        break;
    case ViewportKind::Notes:
        v.rect = freePlace(sheet, 90.0, 80.0);
        v.text = "1. ALL DIMENSIONS ARE IN METRES UNLESS NOTED OTHERWISE.";
        break;
    case ViewportKind::SheetIndex:
        v.rect = freePlace(sheet, 200.0, 150.0);
        break;
    case ViewportKind::Revisions:
        v.rect = freePlace(sheet, 130.0, 60.0);
        break;
    case ViewportKind::Image: {
        v.rect = freePlace(sheet, 100.0, 80.0);
        const SheetSource src = source();
        const QString chosen = QFileDialog::getOpenFileName(
            this, QStringLiteral("Image for the Sheet"), QString::fromStdString(src.assets.string()),
            QStringLiteral("Images (*.png *.jpg *.jpeg *.bmp *.gif)"));
        if (chosen.isEmpty()) {
            return {};
        }
        v.text = QFileInfo(chosen).fileName().toStdString();
        if (!src.assets.empty() &&
            QFileInfo(chosen).absolutePath() != QFileInfo(QString::fromStdString(src.assets.string())).absoluteFilePath()) {
            std::error_code error;
            std::filesystem::create_directories(src.assets, error);
            std::filesystem::copy_file(chosen.toStdWString(), src.assets / v.text,
                                       std::filesystem::copy_options::overwrite_existing, error);
            if (error) {
                return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                               "could not copy the image into the project", error.message());
            }
        }
        break;
    }
    }
    const std::string id = v.id;
    if (auto s = plotting::editSheet(document_, index, [&v](Sheet& edited) {
            edited.viewports.push_back(v);
            return Status{};
        }, "ADD_VIEWPORT");
        !s) {
        return s;
    }
    canvas_->select(id);
    return {};
}

Status SheetEditor::removeSelectedViewport()
{
    const std::vector<std::string> ids = canvas_->selectedIds();
    if (ids.empty()) {
        return {};
    }
    return plotting::removeViewports(document_, currentSheet(), ids,
                                     ids.size() == 1 ? "REMOVE_VIEWPORT" : "REMOVE_VIEWPORTS");
}

Status SheetEditor::tile(TilingPreset preset)
{
    if (document_.sheetSet().sheets.empty()) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidState, "there is no sheet to tile");
    }
    std::size_t placed = 0;
    auto s = plotting::editSheet(document_, currentSheet(), [&placed, preset](Sheet& sheet) {
        placed = plotting::tileViewports(sheet, preset);
        return Status{};
    }, "TILE_VIEWPORTS");
    if (s) {
        report(QString("Tiled %1 view%2.").arg(placed).arg(placed == 1 ? "" : "s"));
    }
    return s;
}

Status SheetEditor::plotToPdf(const QString& path, bool allSheets)
{
    const SheetSet& set = document_.sheetSet();
    if (!allSheets && currentSheet() >= set.sheets.size()) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument, "there is no sheet to plot");
    }
    // The checks first. What they find on these sheets is reported with the
    // plot, and the Checks dock is brought up; the plot still goes ahead -
    // an error there is paper wasted, not a file that cannot be written.
    const std::size_t errors = checkBeforePlot(allSheets);
    // One PDF in the set's plot style (plotting/plot_output.hpp).
    PlotRequest request = plotRequestFor(set.pageSetup, path,
                                         allSheets ? std::string() : std::to_string(currentSheet() + 1));
    request.format = PlotFormat::Pdf;
    request.title = QString::fromStdString(document_.metadata().name);
    SheetPaintCache cache;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const auto result = plotSheets(set, request, source(), cache);
    QApplication::restoreOverrideCursor();
    if (!result) {
        return result.error();
    }
    for (const std::string& problem : result->problems) {
        report(QString::fromStdString(problem), true);
    }
    QString done = result->summary(request);
    if (errors > 0) {
        done += QString(" The checks found %1 error%2 on %3: the Checks panel lists them.")
                    .arg(errors)
                    .arg(errors == 1 ? "" : "s")
                    .arg(allSheets ? QStringLiteral("the sheets") : QStringLiteral("it"));
    }
    report(done);
    return {};
}

std::size_t SheetEditor::checkBeforePlot(bool allSheets)
{
    const SheetSet& set = document_.sheetSet();
    std::vector<std::size_t> pages;
    if (!allSheets && currentSheet() < set.sheets.size()) {
        pages.push_back(currentSheet());
    }
    const std::size_t errors =
        plotting::summarize(plotting::findingsOnSheets(checks_->checkNow(), pages)).errors;
    if (errors > 0) {
        checks_->show();
        checks_->raise();
    }
    return errors;
}

void SheetEditor::plotInteractive(bool allSheets)
{
    // The checks first, as for any plot: the dock comes up over the dialog
    // when they find an error, so it is seen before the paper is.
    (void)checkBeforePlot(allSheets);
    // The Plot dialog, then the plot with its progress (plotting/plot_dialog.hpp).
    plotInteractively(this, document_.sheetSet(), currentSheet(), allSheets,
                      suggestedPlotFile(document_), QString::fromStdString(document_.metadata().name),
                      source_, &document_, [this](const QString& text, bool error) { report(text, error); });
}

// ---- Generate Sheets ------------------------------------------------------------------

namespace {

// The layouts, in the dialog's order: the GENERATE kind each runs, and what
// the list says.
struct GenerateLayout {
    const char* kind;
    const char* text;
};
constexpr std::array kGenerateLayouts{
    GenerateLayout{"fit", "The drawing, fitted (one sheet, or tiles at a set scale)"},
    GenerateLayout{"grid", "Tiles over the drawing, with a key plan"},
    GenerateLayout{"strips", "Plan strips along an alignment"},
    GenerateLayout{"profile", "Plan and profile along an alignment"},
    GenerateLayout{"sections", "Cross sections along an alignment"},
    GenerateLayout{"frames", "One sheet per imported plot frame"},
    GenerateLayout{"register", "Drawing register (cover sheet)"},
};

// The vertical exaggerations offered for cross sections; Auto fits it to
// the ground, as an automatic section view does.
constexpr std::array kExaggerations{1.0, 2.0, 5.0, 10.0, 20.0};

QString areaText(const Box2& box)
{
    return QString("%1,%2,%3,%4")
        .arg(verbNumber(box.min.x), verbNumber(box.min.y), verbNumber(box.max.x), verbNumber(box.max.y));
}

// A chainage typed in a From or To box: nothing for an empty box (the
// alignment's end), else the number; an error for anything else.
katana::core::Result<std::optional<double>> typedChainage(const QLineEdit& edit, const char* what)
{
    const QString text = edit.text().trimmed();
    if (text.isEmpty()) {
        return std::optional<double>{};
    }
    bool ok = false;
    const double value = text.toDouble(&ok);
    if (!ok || !std::isfinite(value)) {
        return katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                       std::string(what) + " is a chainage in metres", text.toStdString());
    }
    return std::optional<double>(value);
}

} // namespace

void SheetEditor::generateSheets()
{
    buildGenerateDialog()->exec();
}

QDialog* SheetEditor::openGenerateDialog()
{
    QDialog* dialog = buildGenerateDialog();
    dialog->open();
    return dialog;
}

// Object names: the dialog sheetGenerateDialog; sheetGenerateLayout (the
// kinds, in kGenerateLayouts' order), sheetGeneratePaper,
// sheetGenerateOrientation, sheetGenerateFrame; sheetGenerateArea (the whole
// drawing, the current plan view, a window) and the window's
// sheetGenerateAreaX0, Y0, X1 and Y1; sheetGenerateScale,
// sheetGenerateAlignment, sheetGenerateFrom, sheetGenerateTo,
// sheetGenerateOverlap, sheetGenerateKeyPlan; the cross sections
// sheetGenerateNoSections, sheetGenerateEvery with sheetGenerateInterval,
// sheetGenerateAtStations with sheetGenerateStations, sheetGenerateHalfWidth,
// sheetGenerateRows, sheetGenerateColumns and sheetGenerateVe;
// sheetGenerateModel3d, sheetGenerateLegend, sheetGenerateRotate,
// sheetGenerateReplace; the line OK runs, sheetGenerateLine; and the buttons
// sheetGenerateOk and sheetGenerateCancel.
QDialog* SheetEditor::buildGenerateDialog()
{
    auto* dialog = new QDialog(this);
    dialog->setObjectName(QStringLiteral("sheetGenerateDialog"));
    dialog->setWindowTitle(QStringLiteral("Generate Sheets"));
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setMinimumWidth(500);
    auto* layout = new QVBoxLayout(dialog);
    auto* form = new QFormLayout();
    layout->addLayout(form);
    const auto named = [](QWidget* widget, const char* name) {
        widget->setObjectName(QString::fromLatin1(name));
        return widget;
    };

    auto* kind = new QComboBox(dialog);
    for (const GenerateLayout& choice : kGenerateLayouts) {
        kind->addItem(QString::fromLatin1(choice.text), QString::fromLatin1(choice.kind));
    }
    named(kind, "sheetGenerateLayout");
    form->addRow(QStringLiteral("Layout"), kind);

    // The paper, for every layout but the plot frames', which bring their own.
    auto* paper = new QComboBox(dialog);
    for (const auto p : kPapers) {
        paper->addItem(paperName(p));
    }
    paper->setCurrentIndex(3);
    named(paper, "sheetGeneratePaper");
    auto* orientation = new QComboBox(dialog);
    orientation->addItems({QStringLiteral("Landscape"), QStringLiteral("Portrait")});
    named(orientation, "sheetGenerateOrientation");
    auto* frame = new QCheckBox(QStringLiteral("Frame and title block"), dialog);
    frame->setChecked(true);
    frame->setToolTip(QStringLiteral("A portrait sheet has no frame: the title block is laid out for landscape"));
    named(frame, "sheetGenerateFrame");
    auto* paperRow = new QWidget(dialog);
    auto* paperLayout = new QHBoxLayout(paperRow);
    paperLayout->setContentsMargins(0, 0, 0, 0);
    paperLayout->addWidget(paper);
    paperLayout->addWidget(orientation);
    paperLayout->addWidget(frame, 1);
    form->addRow(QStringLiteral("Paper"), paperRow);

    // What a fit or the tiles cover.
    auto* area = new QComboBox(dialog);
    area->addItem(QStringLiteral("The whole drawing"), QStringLiteral("drawing"));
    if (planViewArea) {
        if (const auto shown = planViewArea(); shown && !shown->empty()) {
            area->addItem(QStringLiteral("The current plan view"), QStringLiteral("view"));
        }
    }
    area->addItem(QStringLiteral("A window"), QStringLiteral("window"));
    named(area, "sheetGenerateArea");
    form->addRow(QStringLiteral("Area"), area);
    Box2 extent = planDrawnBounds(source().plan, {}, {});
    if (extent.empty()) {
        extent = Box2(Point2(0.0, 0.0), Point2(100.0, 100.0));
    }
    auto* windowRow = new QWidget(dialog);
    auto* windowLayout = new QHBoxLayout(windowRow);
    windowLayout->setContentsMargins(0, 0, 0, 0);
    std::array<QDoubleSpinBox*, 4> corners{};
    const std::array<double, 4> cornerValues{extent.min.x, extent.min.y, extent.max.x, extent.max.y};
    const std::array<const char*, 4> cornerNames{"sheetGenerateAreaX0", "sheetGenerateAreaY0",
                                                 "sheetGenerateAreaX1", "sheetGenerateAreaY1"};
    const std::array<const char*, 4> cornerTips{"West edge (E)", "South edge (N)", "East edge (E)",
                                                "North edge (N)"};
    for (std::size_t i = 0; i < corners.size(); ++i) {
        corners[i] = spin(windowRow, -1e9, 1e9, std::round(cornerValues[i] * 1000.0) / 1000.0, 3);
        named(corners[i], cornerNames[i]);
        corners[i]->setToolTip(QString::fromLatin1(cornerTips[i]));
        windowLayout->addWidget(corners[i]);
    }
    form->addRow(QStringLiteral("Window"), windowRow);

    auto* scale = scaleBox(dialog, true);
    named(scale, "sheetGenerateScale");
    form->addRow(QStringLiteral("Scale"), scale);
    auto* alignment = new QComboBox(dialog);
    alignment->addItem(QStringLiteral("(none)"), QString());
    for (const std::string& name : alignmentNames(document_)) {
        alignment->addItem(QString::fromStdString(name), QString::fromStdString(name));
    }
    alignment->setToolTip(QStringLiteral("None: the drawing's only alignment, or for a fit the area"));
    named(alignment, "sheetGenerateAlignment");
    form->addRow(QStringLiteral("Alignment"), alignment);
    auto* from = new QLineEdit(dialog);
    from->setPlaceholderText(QStringLiteral("the start"));
    named(from, "sheetGenerateFrom");
    auto* to = new QLineEdit(dialog);
    to->setPlaceholderText(QStringLiteral("the end"));
    named(to, "sheetGenerateTo");
    auto* rangeRow = new QWidget(dialog);
    auto* rangeLayout = new QHBoxLayout(rangeRow);
    rangeLayout->setContentsMargins(0, 0, 0, 0);
    rangeLayout->addWidget(from);
    rangeLayout->addWidget(new QLabel(QStringLiteral("to"), rangeRow));
    rangeLayout->addWidget(to);
    form->addRow(QStringLiteral("Chainages"), rangeRow);
    auto* overlap = spin(dialog, 0.0, 1000.0, 10.0, 1, QStringLiteral(" m"));
    named(overlap, "sheetGenerateOverlap");
    form->addRow(QStringLiteral("Overlap"), overlap);
    auto* keyPlan = new QCheckBox(QStringLiteral("A key plan sheet first"), dialog);
    keyPlan->setChecked(true);
    named(keyPlan, "sheetGenerateKeyPlan");
    form->addRow(QString(), keyPlan);

    // Cross sections: after a fit's or a profile's plan, or the whole of a
    // sections layout; every so many metres or at the chainages listed.
    auto* noSections = new QRadioButton(QStringLiteral("None"), dialog);
    named(noSections, "sheetGenerateNoSections");
    noSections->setChecked(true);
    auto* every = new QRadioButton(QStringLiteral("Every"), dialog);
    named(every, "sheetGenerateEvery");
    auto* atStations = new QRadioButton(QStringLiteral("At chainages"), dialog);
    named(atStations, "sheetGenerateAtStations");
    auto* sectionsRow = new QWidget(dialog);
    auto* sectionsLayout = new QHBoxLayout(sectionsRow);
    sectionsLayout->setContentsMargins(0, 0, 0, 0);
    auto* interval = spin(sectionsRow, 0.5, 10000.0, 20.0, 2, QStringLiteral(" m"));
    named(interval, "sheetGenerateInterval");
    auto* stations = new QLineEdit(sectionsRow);
    stations->setPlaceholderText(QStringLiteral("e.g. 20, 40, 60"));
    named(stations, "sheetGenerateStations");
    sectionsLayout->addWidget(noSections);
    sectionsLayout->addWidget(every);
    sectionsLayout->addWidget(interval);
    sectionsLayout->addWidget(atStations);
    sectionsLayout->addWidget(stations, 1);
    form->addRow(QStringLiteral("Cross sections"), sectionsRow);
    auto* halfWidth = spin(dialog, 0.5, 1000.0, 20.0, 2, QStringLiteral(" m"));
    named(halfWidth, "sheetGenerateHalfWidth");
    form->addRow(QStringLiteral("Section half width"), halfWidth);
    auto* rows = new QSpinBox(dialog);
    rows->setRange(1, 8);
    rows->setValue(3);
    named(rows, "sheetGenerateRows");
    auto* columns = new QSpinBox(dialog);
    columns->setRange(1, 6);
    columns->setValue(2);
    named(columns, "sheetGenerateColumns");
    form->addRow(QStringLiteral("Sections down"), rows);
    form->addRow(QStringLiteral("Sections across"), columns);
    auto* ve = new QComboBox(dialog);
    ve->addItem(QStringLiteral("Auto"), QStringLiteral("auto"));
    for (const double x : kExaggerations) {
        ve->addItem(QString("%1 x").arg(verbNumber(x)), verbNumber(x));
    }
    ve->setToolTip(QStringLiteral("Auto: fitted to the ground each section cuts"));
    named(ve, "sheetGenerateVe");
    form->addRow(QStringLiteral("Vertical exaggeration"), ve);

    auto* snapshot = new QCheckBox(QStringLiteral("A 3D snapshot beside the plan"), dialog);
    named(snapshot, "sheetGenerateModel3d");
    auto* legend = new QCheckBox(QStringLiteral("A legend beside the plan"), dialog);
    legend->setChecked(true);
    named(legend, "sheetGenerateLegend");
    form->addRow(QString(), snapshot);
    form->addRow(QString(), legend);
    auto* rotate = new QCheckBox(QStringLiteral("Rotate the drawing to fill the sheet"), dialog);
    named(rotate, "sheetGenerateRotate");
    rotate->setToolTip(QStringLiteral("Turn the plan when that shows the drawing at a larger scale, "
                                      "or on fewer sheets, and no further than it needs"));
    form->addRow(QString(), rotate);
    auto* replace = new QCheckBox(QStringLiteral("Replace the sheets there are now"), dialog);
    named(replace, "sheetGenerateReplace");
    form->addRow(QString(), replace);

    // The line OK runs, as it will be typed into the log: what a person
    // learns the verb from, and what an agent would type instead.
    auto* lineLabel = new QLabel(dialog);
    named(lineLabel, "sheetGenerateLine");
    lineLabel->setWordWrap(true);
    lineLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(lineLabel);

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dialog);
    QPushButton* ok = buttons->button(QDialogButtonBox::Ok);
    ok->setText(QStringLiteral("Generate"));
    named(ok, "sheetGenerateOk");
    named(buttons->button(QDialogButtonBox::Cancel), "sheetGenerateCancel");
    connect(buttons, &QDialogButtonBox::accepted, dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    layout->addWidget(buttons);

    const auto kindNow = [kind] { return kind->currentData().toString(); };
    const auto fixedScale = [scale] { return parseScale(scale->currentText()); };

    // The GENERATE line the choices make, every option the layout takes
    // written out, so the line says the whole choice; or what is wrong with
    // them.
    const auto buildLine = [=, this]() -> katana::core::Result<QString> {
        const QString k = kindNow();
        QStringList words{QStringLiteral("GENERATE"), k};
        const bool frames = k == QLatin1String("frames");
        const bool alongAlignment =
            k == QLatin1String("strips") || k == QLatin1String("profile") || k == QLatin1String("sections");
        const QString aligned = alignment->currentData().toString();
        if (!frames) {
            words << "paper=" + paper->currentText();
            words << (orientation->currentIndex() == 1 ? QStringLiteral("portrait") : QStringLiteral("landscape"));
        }
        words << QString("frame=%1").arg(frame->isChecked() ? "on" : "off");
        if (k == QLatin1String("frames") || k == QLatin1String("register")) {
            return words.join(' ');
        }
        const bool fit = k == QLatin1String("fit");
        if ((fit && aligned.isEmpty()) || k == QLatin1String("grid")) {
            const QString chosen = area->currentData().toString();
            if (chosen == QLatin1String("view")) {
                const auto shown = planViewArea ? planViewArea() : std::nullopt;
                if (!shown || shown->empty()) {
                    return katana::core::makeError(katana::core::ErrorCode::InvalidState,
                                                   "there is no plan view to take the area from");
                }
                words << "area=" + areaText(*shown);
            } else if (chosen == QLatin1String("window")) {
                const Box2 window(Point2(std::min(corners[0]->value(), corners[2]->value()),
                                         std::min(corners[1]->value(), corners[3]->value())),
                                  Point2(std::max(corners[0]->value(), corners[2]->value()),
                                         std::max(corners[1]->value(), corners[3]->value())));
                if (!(window.width() > 0.0) || !(window.height() > 0.0)) {
                    return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                                   "the window needs a width and a height");
                }
                words << "area=" + areaText(window);
            }
        }
        if ((fit || alongAlignment) && !aligned.isEmpty()) {
            words << "alignment=" + quoted(aligned);
        }
        const std::optional<double> fixed = fixedScale();
        if (k == QLatin1String("grid")) {
            // Tiles are cut at a fixed scale; Auto is GENERATE grid's own 500.
            if (fixed) {
                words << "scale=" + verbNumber(*fixed);
            }
        } else {
            words << (fixed ? "scale=" + verbNumber(*fixed) : QStringLiteral("scale=auto"));
        }
        const bool fixedStrips = k == QLatin1String("strips") && fixed;
        if (k == QLatin1String("grid") || fixedStrips) {
            words << "overlap=" + verbNumber(overlap->value());
            words << QString("keyplan=%1").arg(keyPlan->isChecked() ? "on" : "off");
        }
        if (fixedStrips) {
            for (const auto& [edit, key] : {std::pair{from, "from"}, std::pair{to, "to"}}) {
                auto chainage = typedChainage(*edit, key);
                if (!chainage) {
                    return chainage.error();
                }
                if (*chainage) {
                    words << QString("%1=%2").arg(QLatin1String(key), verbNumber(**chainage));
                }
            }
        }
        const bool withPlan = fit || k == QLatin1String("profile");
        if (withPlan) {
            if (every->isChecked()) {
                words << "interval=" + verbNumber(interval->value())
                      << "halfwidth=" + verbNumber(halfWidth->value());
            }
            words << QString("model3d=%1").arg(snapshot->isChecked() ? "on" : "off");
            words << QString("legend=%1").arg(legend->isChecked() ? "on" : "off");
        }
        if (fit && rotate->isChecked() && rotate->isEnabled()) {
            words << QStringLiteral("rotate=on");
        }
        if (k == QLatin1String("sections")) {
            if (atStations->isChecked()) {
                const auto list = stationList(stations->text());
                if (!list || list->isEmpty()) {
                    return katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                                   "list the chainages to cut, separated by commas",
                                                   stations->text().toStdString());
                }
                words << "stations=" + *list;
            } else {
                words << "interval=" + verbNumber(interval->value());
            }
            words << "halfwidth=" + verbNumber(halfWidth->value())
                  << QString("rows=%1").arg(rows->value()) << QString("columns=%1").arg(columns->value())
                  << "ve=" + ve->currentData().toString();
        }
        if (replace->isChecked()) {
            words << QStringLiteral("replace=on");
        }
        return words.join(' ');
    };

    const auto enable = [=, this] {
        const QString k = kindNow();
        const bool fit = k == QLatin1String("fit");
        const bool grid = k == QLatin1String("grid");
        const bool strips = k == QLatin1String("strips");
        const bool profile = k == QLatin1String("profile");
        const bool sections = k == QLatin1String("sections");
        const bool frames = k == QLatin1String("frames");
        const bool reg = k == QLatin1String("register");
        const bool aligned = !alignment->currentData().toString().isEmpty();
        const bool fixedStrips = strips && fixedScale().has_value();
        paper->setEnabled(!frames);
        orientation->setEnabled(!frames);
        area->setEnabled((fit && !aligned) || grid);
        const bool window = area->isEnabled() && area->currentData().toString() == QLatin1String("window");
        for (QDoubleSpinBox* corner : corners) {
            corner->setEnabled(window);
        }
        scale->setEnabled(!frames && !reg);
        alignment->setEnabled(fit || strips || profile || sections);
        from->setEnabled(fixedStrips);
        to->setEnabled(fixedStrips);
        overlap->setEnabled(grid || fixedStrips);
        keyPlan->setEnabled(grid || fixedStrips);
        // None for a plan's sections, At chainages for a sections layout:
        // a choice the layout does not take is moved off.
        noSections->setEnabled(fit || profile);
        every->setEnabled(fit || profile || sections);
        atStations->setEnabled(sections);
        if (sections && noSections->isChecked()) {
            every->setChecked(true);
        } else if (!sections && atStations->isChecked()) {
            noSections->setChecked(true);
        }
        interval->setEnabled(every->isEnabled() && every->isChecked());
        stations->setEnabled(sections && atStations->isChecked());
        halfWidth->setEnabled(sections || ((fit || profile) && every->isChecked()));
        rows->setEnabled(sections);
        columns->setEnabled(sections);
        ve->setEnabled(sections);
        snapshot->setEnabled(fit || profile);
        legend->setEnabled(fit || profile);
        // Only a plan of an area turns; a strip follows its alignment.
        rotate->setEnabled(fit && !aligned && noSections->isChecked());
        replace->setEnabled(!reg);
        const auto line = buildLine();
        lineLabel->setText(line ? *line : QString::fromStdString(line.error().message));
        lineLabel->setStyleSheet(line ? QString() : QStringLiteral("color: #d05050;"));
        ok->setEnabled(line.ok());
    };
    // A layout along an alignment starts on the first one; a fit on none,
    // which is the drawing.
    connect(kind, &QComboBox::currentIndexChanged, dialog, [=] {
        const QString k = kindNow();
        if (k == QLatin1String("fit")) {
            alignment->setCurrentIndex(0);
        } else if (alignment->currentIndex() == 0 && alignment->count() > 1 &&
                   (k == QLatin1String("strips") || k == QLatin1String("profile") ||
                    k == QLatin1String("sections"))) {
            alignment->setCurrentIndex(1);
        }
        enable();
    });
    for (QComboBox* combo : {paper, orientation, area, alignment, ve}) {
        connect(combo, &QComboBox::currentIndexChanged, dialog, enable);
    }
    connect(scale, &QComboBox::currentTextChanged, dialog, enable);
    for (QLineEdit* edit : {from, to, stations}) {
        connect(edit, &QLineEdit::textChanged, dialog, enable);
    }
    for (QAbstractButton* button : std::initializer_list<QAbstractButton*>{
             frame, keyPlan, noSections, every, atStations, snapshot, legend, rotate, replace}) {
        connect(button, &QAbstractButton::toggled, dialog, enable);
    }
    for (QDoubleSpinBox* number : {overlap, interval, halfWidth, corners[0], corners[1], corners[2], corners[3]}) {
        connect(number, &QDoubleSpinBox::valueChanged, dialog, enable);
    }
    for (QSpinBox* number : {rows, columns}) {
        connect(number, &QSpinBox::valueChanged, dialog, enable);
    }
    enable();

    connect(dialog, &QDialog::accepted, this, [this, buildLine, kindNow, replace] {
        const auto line = buildLine();
        if (!line) {
            report(QString::fromStdString(line.error().describe()), true);
            return;
        }
        // Where the new sheets go: a register in front, the rest after the
        // sheets there are, or in their place.
        const bool cover = kindNow() == QLatin1String("register");
        const std::size_t first =
            cover || replace->isChecked() ? 0 : document_.sheetSet().sheets.size();
        if (runLine(*line).ok && first < document_.sheetSet().sheets.size()) {
            setCurrentSheet(first);
            canvas_->fitPage();
        }
    });
    return dialog;
}

// ---- Title Block ----------------------------------------------------------------------

void SheetEditor::editTitleBlock()
{
    SheetSet set = document_.sheetSet();
    plotting::SheetDefaults& d = set.defaults;
    QDialog dialog(this);
    dialog.setWindowTitle(QStringLiteral("Title Block"));
    dialog.setMinimumWidth(620);
    auto* layout = new QVBoxLayout(&dialog);
    auto* tabs = new QTabWidget(&dialog);
    layout->addWidget(tabs);

    const auto line = [](QWidget* parent, const std::string& value, const QString& placeholder = {}) {
        auto* edit = new QLineEdit(QString::fromStdString(value), parent);
        edit->setPlaceholderText(placeholder);
        return edit;
    };

    // Project.
    auto* project = new QWidget(tabs);
    auto* pf = new QFormLayout(project);
    auto* organisation = line(project, d.organisation, QStringLiteral("your organisation's name"));
    pf->addRow(QStringLiteral("Organisation"), organisation);
    std::array<QLineEdit*, 4> lines{};
    const SheetSource src = source();
    for (std::size_t i = 0; i < 4; ++i) {
        QString placeholder;
        if (i == 0) {
            placeholder = QString::fromStdString(src.fields.projectName);
        } else if (i == 1) {
            placeholder = QString::fromStdString(src.fields.projectDescription);
        }
        lines[i] = line(project, i < d.projectLines.size() ? d.projectLines[i] : std::string{}, placeholder);
        pf->addRow(QString("Project line %1").arg(i + 1), lines[i]);
    }
    auto* client = line(project, d.client);
    pf->addRow(QStringLiteral("Client"), client);
    auto* setNumber = line(project, d.setNumber);
    pf->addRow(QStringLiteral("Drawing set number"), setNumber);
    auto* numbering = line(project, set.numbering, QStringLiteral("{n}"));
    numbering->setToolTip(QStringLiteral("{n} the sheet's position, {n:02} padded, {N} the count, {set} the set number"));
    pf->addRow(QStringLiteral("Sheet numbering"), numbering);
    auto* coordinates = line(project, d.coordinateSystem, QString::fromStdString(src.fields.coordinateSystem));
    pf->addRow(QStringLiteral("Coordinate system"), coordinates);
    auto* datum = line(project, d.heightDatum);
    pf->addRow(QStringLiteral("Height datum"), datum);
    auto* modelName = line(project, d.modelName);
    pf->addRow(QStringLiteral("Model name"), modelName);
    tabs->addTab(project, QStringLiteral("Project"));

    // Sign-offs.
    auto* people = new QWidget(tabs);
    auto* sf = new QFormLayout(people);
    struct Row {
        plotting::SignOff* who;
        QString label;
        QLineEdit* name = nullptr;
        QLineEdit* date = nullptr;
    };
    std::array<Row, 5> rowsOf{Row{&d.locator, QStringLiteral("Utilities located by")},
                              Row{&d.surveyor, QStringLiteral("Surveyed by")},
                              Row{&d.compiler, QStringLiteral("Compiled by")},
                              Row{&d.reviewer, QStringLiteral("Reviewed by")},
                              Row{&d.approver, QStringLiteral("Approved by")}};
    for (Row& r : rowsOf) {
        auto* h = new QHBoxLayout();
        r.name = line(people, r.who->name, QStringLiteral("name"));
        r.date = line(people, r.who->date, QStringLiteral("dd/mm/yy (blank: the plot date)"));
        h->addWidget(r.name, 2);
        h->addWidget(r.date, 1);
        sf->addRow(r.label, h);
    }
    tabs->addTab(people, QStringLiteral("Sign-offs"));

    // Notes.
    auto* notesPage = new QWidget(tabs);
    auto* nl = new QVBoxLayout(notesPage);
    nl->addWidget(new QLabel(QStringLiteral("Printed in the title block's notes cell, wrapped to fit:"), notesPage));
    auto* notes = new QPlainTextEdit(QString::fromStdString(d.notes), notesPage);
    nl->addWidget(notes);
    tabs->addTab(notesPage, QStringLiteral("Notes"));

    // Revisions.
    auto* revisionsPage = new QWidget(tabs);
    auto* rl = new QVBoxLayout(revisionsPage);
    auto* revisions = new QTableWidget(static_cast<int>(set.revisions.size()), 4, revisionsPage);
    revisions->setHorizontalHeaderLabels({QStringLiteral("Rev"), QStringLiteral("Date"),
                                          QStringLiteral("Description"), QStringLiteral("By")});
    revisions->horizontalHeader()->setSectionResizeMode(2, QHeaderView::Stretch);
    for (int i = 0; i < revisions->rowCount(); ++i) {
        const plotting::Revision& r = set.revisions[static_cast<std::size_t>(i)];
        revisions->setItem(i, 0, new QTableWidgetItem(QString::fromStdString(r.code)));
        revisions->setItem(i, 1, new QTableWidgetItem(QString::fromStdString(r.date)));
        revisions->setItem(i, 2, new QTableWidgetItem(QString::fromStdString(r.description)));
        revisions->setItem(i, 3, new QTableWidgetItem(QString::fromStdString(r.by)));
    }
    rl->addWidget(revisions);
    auto* rb = new QHBoxLayout();
    auto* addRevision = new QPushButton(QStringLiteral("Add"), revisionsPage);
    auto* removeRevision = new QPushButton(QStringLiteral("Remove"), revisionsPage);
    rb->addWidget(addRevision);
    rb->addWidget(removeRevision);
    rb->addStretch(1);
    rl->addLayout(rb);
    connect(addRevision, &QPushButton::clicked, &dialog, [revisions] {
        const int row = revisions->rowCount();
        revisions->insertRow(row);
        const QString code = row < 26 ? QString(QChar('A' + row)) : QString::number(row);
        revisions->setItem(row, 0, new QTableWidgetItem(code));
        revisions->setItem(row, 1, new QTableWidgetItem(QDate::currentDate().toString("dd/MM/yy")));
        revisions->setItem(row, 2, new QTableWidgetItem());
        revisions->setItem(row, 3, new QTableWidgetItem());
    });
    connect(removeRevision, &QPushButton::clicked, &dialog, [revisions] {
        if (revisions->currentRow() >= 0) {
            revisions->removeRow(revisions->currentRow());
        }
    });
    tabs->addTab(revisionsPage, QStringLiteral("Revisions"));

    // Logo.
    auto* logoPage = new QWidget(tabs);
    auto* ll = new QVBoxLayout(logoPage);
    auto* preview = new QLabel(logoPage);
    preview->setMinimumHeight(90);
    preview->setAlignment(Qt::AlignCenter);
    preview->setStyleSheet(QStringLiteral("background: white; border: 1px solid #999;"));
    const auto showLogo = [this, preview] {
        const auto path = plotting::logoPath(document_);
        QImage image;
        if (path) {
            image.load(QString::fromStdWString(path->wstring()));
        }
        if (image.isNull()) {
            preview->setText(QStringLiteral("No logo. The slot prints empty."));
        } else {
            preview->setPixmap(QPixmap::fromImage(image.scaled(424, 96, Qt::KeepAspectRatio,
                                                               Qt::SmoothTransformation)));
        }
    };
    showLogo();
    ll->addWidget(new QLabel(QStringLiteral("Your organisation's logo, printed in the title block's logo slot "
                                            "(53 x 12 mm), its shape kept. PNG, JPEG, GIF or BMP."),
                             logoPage));
    ll->addWidget(preview);
    auto* lb = new QHBoxLayout();
    auto* chooseLogo = new QPushButton(QStringLiteral("Choose Logo..."), logoPage);
    auto* clearLogo = new QPushButton(QStringLiteral("No Logo"), logoPage);
    lb->addWidget(chooseLogo);
    lb->addWidget(clearLogo);
    lb->addStretch(1);
    ll->addLayout(lb);
    ll->addStretch(1);
    bool logoChanged = false;
    connect(chooseLogo, &QPushButton::clicked, &dialog, [&, this] {
        const QString file = QFileDialog::getOpenFileName(
            &dialog, QStringLiteral("Logo"), QString(),
            QStringLiteral("Images (*.png *.jpg *.jpeg *.gif *.bmp)"));
        if (file.isEmpty()) {
            return;
        }
        auto stored = plotting::importLogo(document_, file.toStdWString());
        if (!stored) {
            QMessageBox::warning(&dialog, QStringLiteral("Logo"),
                                 QString::fromStdString(stored.error().describe()));
            return;
        }
        logoChanged = true;
        showLogo();
    });
    connect(clearLogo, &QPushButton::clicked, &dialog, [&, this] {
        (void)plotting::editSheetSet(document_, [](SheetSet& s) {
            s.defaults.logoAsset.clear();
            return Status{};
        }, "SHEET_LOGO");
        logoChanged = true;
        showLogo();
    });
    tabs->addTab(logoPage, QStringLiteral("Logo"));

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    layout->addWidget(buttons);
    if (dialog.exec() != QDialog::Accepted) {
        return;
    }

    const auto text = [](QLineEdit* e) { return e->text().trimmed().toStdString(); };
    // The logo is stored by its own step; everything else in one more.
    const std::string logo = document_.sheetSet().defaults.logoAsset;
    std::vector<std::string> projectLines;
    for (QLineEdit* e : lines) {
        projectLines.push_back(text(e));
    }
    while (!projectLines.empty() && projectLines.back().empty()) {
        projectLines.pop_back();
    }
    std::vector<plotting::SignOff> signOffs;
    for (const Row& r : rowsOf) {
        signOffs.push_back({text(r.name), text(r.date)});
    }
    std::vector<plotting::Revision> revisionRows;
    for (int i = 0; i < revisions->rowCount(); ++i) {
        const auto cell = [&](int c) {
            return revisions->item(i, c) != nullptr ? revisions->item(i, c)->text().trimmed().toStdString()
                                                    : std::string{};
        };
        if (!cell(0).empty() || !cell(2).empty()) {
            revisionRows.push_back({cell(0), cell(1), cell(2), cell(3)});
        }
    }
    const auto status = plotting::editSheetSet(document_, [&](SheetSet& s) {
        plotting::SheetDefaults& e = s.defaults;
        e.organisation = text(organisation);
        e.projectLines = projectLines;
        e.client = text(client);
        e.setNumber = text(setNumber);
        e.coordinateSystem = text(coordinates);
        e.heightDatum = text(datum);
        e.modelName = text(modelName);
        e.notes = notes->toPlainText().trimmed().toStdString();
        e.locator = signOffs[0];
        e.surveyor = signOffs[1];
        e.compiler = signOffs[2];
        e.reviewer = signOffs[3];
        e.approver = signOffs[4];
        e.logoAsset = logo;
        s.numbering = numbering->text().trimmed().isEmpty() ? std::string("{n}") : text(numbering);
        s.revisions = revisionRows;
        return Status{};
    }, "TITLE_BLOCK");
    if (!status) {
        report(QString::fromStdString(status.error().describe()), true);
    } else if (logoChanged) {
        canvas_->invalidate();
    }
}

} // namespace katana::qt
