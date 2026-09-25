#include "sheet_editor.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <map>
#include <set>
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
#include <QFocusEvent>
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
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/math/numerics.hpp"
#include "plotting/sheet_list_widget.hpp"
#include "plotting/sheet_rulers.hpp"

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
                            ViewportKind::Notes,         ViewportKind::Image};

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

} // namespace

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

void SheetCanvas::selectionChanged()
{
    update();
    if (onSelectionChanged) {
        onSelectionChanged(selected_);
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

std::vector<std::string> SheetCanvas::selectedIds() const
{
    std::vector<std::string> ids;
    if (const Sheet* sheet = currentSheet()) {
        for (const Viewport& v : sheet->viewports) {
            if (isSelected(v.id)) {
                ids.push_back(v.id);
            }
        }
    }
    return ids;
}

bool SheetCanvas::isSelected(std::string_view viewportId) const
{
    return !viewportId.empty() && std::ranges::find(selection_, viewportId) != selection_.end();
}

void SheetCanvas::setSelection(std::vector<std::string> ids, std::string primary)
{
    std::vector<std::string> kept;
    for (std::string& id : ids) {
        if (viewport(id) != nullptr && std::ranges::find(kept, id) == kept.end()) {
            kept.push_back(std::move(id));
        }
    }
    if (std::ranges::find(kept, primary) == kept.end()) {
        primary = kept.empty() ? std::string{} : kept.back();
    }
    if (kept == selection_ && primary == selected_) {
        return;
    }
    selection_ = std::move(kept);
    selected_ = std::move(primary);
    selectionChanged();
}

void SheetCanvas::toggleSelected(const std::string& viewportId)
{
    if (viewport(viewportId) == nullptr) {
        return;
    }
    std::vector<std::string> ids = selection_;
    if (isSelected(viewportId)) {
        std::erase(ids, viewportId);
        setSelection(std::move(ids), selected_ == viewportId ? std::string{} : selected_);
    } else {
        ids.push_back(viewportId);
        setSelection(std::move(ids), viewportId);
    }
}

void SheetCanvas::selectAll()
{
    std::vector<std::string> ids;
    if (const Sheet* sheet = currentSheet()) {
        for (const Viewport& v : sheet->viewports) {
            if (!v.rect.empty()) {
                ids.push_back(v.id);
            }
        }
    }
    setSelection(std::move(ids), selected_);
}

void SheetCanvas::cycleSelection(bool forward)
{
    if (const Sheet* sheet = currentSheet()) {
        select(plotting::cycleViewport(*sheet, selected_, forward));
    }
}

// Drops from the selection what is no longer on the sheet: after an undo, a
// delete, a paste that was undone.
void SheetCanvas::pruneSelection()
{
    const auto gone = [this](const std::string& id) { return viewport(id) == nullptr; };
    if (std::ranges::none_of(selection_, gone) && (selected_.empty() || !gone(selected_))) {
        return;
    }
    std::erase_if(selection_, gone);
    if (selected_.empty() || gone(selected_)) {
        selected_ = selection_.empty() ? std::string{} : selection_.back();
    }
    selectionChanged();
}

void SheetCanvas::invalidate()
{
    ++version_;
    pruneSelection();
    update();
}

double SheetCanvas::rulerPixels() const { return rulers_ ? kSheetRulerPixels : 0.0; }

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

void SheetCanvas::zoomToSelection()
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return;
    }
    const std::vector<std::string> ids = selectedIds();
    zoomTo(plotting::viewportBounds(*sheet, ids));
}

void SheetCanvas::setSnapToGrid(bool on)
{
    snapGrid_ = on;
    update();
}

void SheetCanvas::setRulersShown(bool on)
{
    // The paper stays where it is; the rulers cover or uncover the desk.
    rulers_ = on;
    update();
}

ResolvedViewport SheetCanvas::resolvedPlan(const Viewport& v) const
{
    if (!v.autoScale && !v.autoCentre) {
        return ResolvedViewport{v.scale, v.centre};
    }
    if (resolvedVersion_ != version_) {
        resolved_.clear();
        resolvedVersion_ = version_;
    }
    if (const auto it = resolved_.find(v.id); it != resolved_.end()) {
        return it->second;
    }
    const ResolvedViewport at = resolvePlanViewport(v, editor_.source());
    resolved_.emplace(v.id, at);
    return at;
}

SheetCursorReadout SheetCanvas::readoutAt(const QPointF& widget) const
{
    const Sheet* sheet = currentSheet();
    if (sheet == nullptr) {
        return {};
    }
    return sheetCursorReadout(*sheet, widgetToPaper(widget),
                              [this](const Viewport& v) { return resolvedPlan(v); });
}

void SheetCanvas::updateReadout(const QPointF& widget)
{
    readout_ = readoutAt(widget);
    cursorPaper_ = readout_.onSheet ? std::optional<Point2>(readout_.paper) : std::nullopt;
    if (onCursorMoved) {
        onCursorMoved(readout_);
    }
    // The rulers mark the cursor: only they need painting for a move.
    if (rulers_ && drag_ == Drag::None) {
        const int r = kSheetRulerPixels;
        update(QRect(0, 0, width(), r));
        update(QRect(0, 0, r, height()));
    }
}

Box2 SheetCanvas::liveRect(const std::string& id) const
{
    const Viewport* v = viewport(id);
    if (v == nullptr) {
        return {};
    }
    if (pressMoved_ && drag_ == Drag::Move) {
        if (const auto it = startRects_.find(id); it != startRects_.end()) {
            return startRects_.size() == 1
                       ? liveRect_
                       : Box2(it->second.min + liveDelta_, it->second.max + liveDelta_);
        }
    }
    if (pressMoved_ && drag_ == Drag::Resize && id == selected_) {
        return liveRect_;
    }
    return v->rect;
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
        const bool dragging = pressMoved_ && (drag_ == Drag::Move || drag_ == Drag::Resize);
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
                                     .translated(around.bottomLeft() + QPointF(4.0, 18.0))
                                     .adjusted(-4.0, -2.0, 4.0, 2.0);
            painter.fillRect(plate, QColor(30, 32, 38, 200));
            painter.setPen(Qt::white);
            painter.drawText(around.bottomLeft() + QPointF(4.0, 18.0), label);
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

void SheetCanvas::paintDraggedContent(QPainter& painter)
{
    const bool moving = drag_ == Drag::Move && pressMoved_ && !startRects_.empty();
    const bool sizing = drag_ == Drag::Resize && pressMoved_;
    if ((!moving && !sizing) || image_.isNull()) {
        return;
    }
    // The last paint, in the image's own pixels: where a paper rectangle was
    // painted in it.
    const double ratio = image_.devicePixelRatio();
    const QPointF shift = origin_ - imageOrigin_;
    const auto inImage = [&](const Box2& paper) {
        const QRectF r = widgetRect(paper).translated(-shift);
        return QRectF(r.topLeft() * ratio, r.size() * ratio);
    };
    std::map<std::string, Box2> from;
    if (moving) {
        from = startRects_;
    } else {
        from.emplace(selected_, startRect_);
    }
    painter.save();
    // Where they were, faded: the paper beneath them is not painted yet.
    for (const auto& [id, start] : from) {
        const QRectF r = widgetRect(start);
        painter.fillRect(r, QColor(255, 255, 255, 190));
        painter.setPen(QPen(QColor(120, 122, 128), 1.0, Qt::DotLine));
        painter.drawRect(r);
    }
    for (const auto& [id, start] : from) {
        const QRectF to = widgetRect(liveRect(id));
        if (moving) {
            painter.drawImage(to, image_, inImage(start));
            continue;
        }
        // Sized: what it showed stays centred - a plan keeps its centre and
        // scale - and the rest is empty paper until it is painted again.
        const QRectF was = widgetRect(start);
        painter.save();
        painter.setClipRect(to);
        painter.fillRect(to, Qt::white);
        painter.drawImage(was.translated(to.center() - was.center()), image_, inImage(start));
        painter.restore();
    }
    painter.restore();
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
        // Shift, else replacing it.
        bandAdds_ = shift || control;
        if (!bandAdds_) {
            select({});
        }
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
    if (!startRects_.empty()) {
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
    if (drag_ == Drag::Move) {
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
        // A click on empty paper has already cleared the selection.
        if (moved) {
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

void SheetCanvas::cancelDrag()
{
    drag_ = Drag::None;
    pressMoved_ = false;
    narrowOnClick_ = false;
    startRects_.clear();
    panViewId_.clear();
    viewShift_ = Point2();
    guideX_.reset();
    guideY_.reset();
    unsetCursor();
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

void SheetCanvas::keyReleaseEvent(QKeyEvent* event)
{
    if (event->key() == Qt::Key_Space && !event->isAutoRepeat()) {
        spaceHeld_ = false;
        if (drag_ != Drag::Pan) {
            unsetCursor();
        }
        return;
    }
    QWidget::keyReleaseEvent(event);
}

bool SheetCanvas::event(QEvent* event)
{
    // Tab steps through the viewports rather than out of the canvas; on a
    // sheet with none it moves the focus on as usual.
    if (event->type() == QEvent::KeyPress) {
        const auto* key = static_cast<QKeyEvent*>(event);
        const Sheet* sheet = currentSheet();
        if ((key->key() == Qt::Key_Tab || key->key() == Qt::Key_Backtab) && sheet != nullptr &&
            !plotting::cycleViewport(*sheet, {}, true).empty()) {
            const bool back =
                key->key() == Qt::Key_Backtab || (key->modifiers() & Qt::ShiftModifier) != 0;
            cycleSelection(!back);
            return true;
        }
    }
    return QWidget::event(event);
}

void SheetCanvas::leaveEvent(QEvent* event)
{
    // Off the canvas: the rulers' mark and the status line go blank.
    cursorPaper_.reset();
    readout_ = {};
    if (onCursorMoved) {
        onCursorMoved(readout_);
    }
    if (rulers_) {
        const int r = kSheetRulerPixels;
        update(QRect(0, 0, width(), r));
        update(QRect(0, 0, r, height()));
    }
    QWidget::leaveEvent(event);
}

void SheetCanvas::focusOutEvent(QFocusEvent* event)
{
    // Space let go where the canvas could not hear it.
    spaceHeld_ = false;
    QWidget::focusOutEvent(event);
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
    connect(generate, &QAction::triggered, this, [this] { generateSheets(); });

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
            auto* group = new QLabel(QString("%1 views selected: a drag, the arrows, Delete, Copy and "
                                             "Duplicate act on them all. These properties are %2's.")
                                         .arg(count)
                                         .arg(QString::fromStdString(id)),
                                     panel);
            group->setObjectName(QStringLiteral("sheetSelectionNote"));
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
        if (plan || section) {
            auto* scale = scaleBox(box, plan);
            if (plan && v.autoScale) {
                scale->setCurrentIndex(0);
                const ResolvedViewport resolved = resolvePlanViewport(v, source());
                scale->setToolTip(QString("Automatic: now %1").arg(scaleLabel(resolved.scale)));
            } else {
                scale->setCurrentText(scaleLabel(v.scale));
            }
            connect(scale, &QComboBox::textActivated, this, [edit, scale](const QString& text) {
                if (text == QStringLiteral("Auto")) {
                    edit([](Viewport& e) { e.autoScale = true; });
                } else if (auto value = parseScale(text)) {
                    edit([value](Viewport& e) {
                        e.scale = *value;
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
            auto* ve = spin(box, 0.1, 100.0, v.verticalExaggeration, 2, QStringLiteral(" x"));
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
        if (v.kind == ViewportKind::LongSection || (plan && !v.source.alignment.empty())) {
            auto* from = spin(box, -1e9, 1e9, v.source.chainageFrom, 3);
            auto* to = spin(box, -1e9, 1e9, v.source.chainageTo, 3);
            from->setToolTip(QStringLiteral("From equal to To: the whole alignment"));
            connect(from, &QDoubleSpinBox::valueChanged, this,
                    [edit](double c) { edit([c](Viewport& e) { e.source.chainageFrom = c; }); });
            connect(to, &QDoubleSpinBox::valueChanged, this,
                    [edit](double c) { edit([c](Viewport& e) { e.source.chainageTo = c; }); });
            form->addRow(QStringLiteral("Chainage from"), from);
            form->addRow(QStringLiteral("Chainage to"), to);
        }
        if (v.kind == ViewportKind::CrossSections) {
            QStringList stations;
            for (const double s : v.source.stations) {
                stations << QString::number(s, 'f', 3);
            }
            auto* list = new QLineEdit(stations.join(QStringLiteral(", ")), box);
            list->setPlaceholderText(QStringLiteral("e.g. 20, 40, 60"));
            connect(list, &QLineEdit::editingFinished, this, [edit, list] {
                std::vector<double> values;
                for (const QString& part : list->text().split(QRegularExpression("[,;\\s]+"), Qt::SkipEmptyParts)) {
                    bool ok = false;
                    const double value = part.toDouble(&ok);
                    if (ok) {
                        values.push_back(value);
                    }
                }
                edit([values](Viewport& e) { e.source.stations = values; });
            });
            form->addRow(QStringLiteral("Chainages"), list);
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

            auto* layers = new QPushButton(
                QString("Hidden layers (%1)...").arg(v.hiddenLayers.size()), box);
            connect(layers, &QPushButton::clicked, this, [this, id, v] {
                QDialog dialog(this);
                dialog.setWindowTitle(QStringLiteral("Layers Shown in This View"));
                auto* l = new QVBoxLayout(&dialog);
                auto* listWidget = new QListWidget(&dialog);
                for (const auto& layer : document_.model().layers.all()) {
                    auto* item = new QListWidgetItem(QString::fromStdString(layer.name), listWidget);
                    item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
                    item->setCheckState(v.hiddenLayers.hidesDirectly(layer.name) ? Qt::Unchecked
                                                                                 : Qt::Checked);
                }
                l->addWidget(listWidget);
                auto* ok = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
                connect(ok, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
                connect(ok, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
                l->addWidget(ok);
                if (dialog.exec() != QDialog::Accepted) {
                    return;
                }
                std::vector<std::pair<std::string, bool>> states;
                for (int i = 0; i < listWidget->count(); ++i) {
                    states.emplace_back(listWidget->item(i)->text().toStdString(),
                                        listWidget->item(i)->checkState() != Qt::Checked);
                }
                (void)plotting::editViewport(
                    document_, id,
                    [&states](Viewport& e) {
                        for (const auto& [name, hide] : states) {
                            if (hide) {
                                e.hiddenLayers.hide(name);
                            } else {
                                e.hiddenLayers.show(name);
                            }
                        }
                        return Status{};
                    },
                    "VIEWPORT_LAYERS");
            });
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
        if (v.kind == ViewportKind::Image) {
            auto* file = new QLineEdit(QString::fromStdString(v.text), box);
            file->setPlaceholderText(QStringLiteral("a file in the project's assets folder"));
            connect(file, &QLineEdit::editingFinished, this, [edit, file] {
                edit([t = file->text().toStdString()](Viewport& e) { e.text = t; });
            });
            form->addRow(QStringLiteral("Image"), file);
        }
        auto* locked = new QCheckBox(QStringLiteral("Locked (tiling and dragging leave it)"), box);
        locked->setChecked(v.locked);
        connect(locked, &QCheckBox::toggled, this,
                [edit](bool on) { edit([on](Viewport& e) { e.locked = on; }); });
        form->addRow(QString(), locked);

        auto* rectLabel = new QLabel(
            QString("%1 x %2 mm at %3, %4")
                .arg(v.rect.width(), 0, 'f', 1).arg(v.rect.height(), 0, 'f', 1)
                .arg(v.rect.min.x, 0, 'f', 1).arg(v.rect.min.y, 0, 'f', 1),
            box);
        form->addRow(QStringLiteral("On the paper"), rectLabel);
        layout->addWidget(box);

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

void SheetEditor::showContextMenu(const QPointF& global, const std::string& viewportId)
{
    QMenu menu(this);
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
        // The other sheets' plans, outlined and numbered.
        const SheetSource src = source();
        for (const Sheet& other : set.sheets) {
            if (other.id == sheet.id) {
                continue;
            }
            for (const Viewport& p : other.viewports) {
                if (p.kind != ViewportKind::Plan || p.rect.empty()) {
                    continue;
                }
                const ResolvedViewport at = resolvePlanViewport(p, src);
                const double hw = p.rect.width() * at.scale / 2000.0;
                const double hh = p.rect.height() * at.scale / 2000.0;
                plotting::WorldMark mark;
                mark.kind = plotting::WorldMark::Kind::SheetOutline;
                mark.sheet = other.id;
                for (const Point2& corner : {Point2(-hw, -hh), Point2(hw, -hh), Point2(hw, hh), Point2(-hw, hh)}) {
                    mark.points.push_back(at.centre + rotated(corner, p.rotation));
                }
                v.marks.push_back(std::move(mark));
                break;
            }
        }
        break;
    }
    case ViewportKind::LongSection:
        v.rect = freePlace(sheet, 380.0, 110.0);
        v.scale = 500.0;
        v.verticalExaggeration = 10.0;
        v.autoCentre = true;
        if (!alignments.empty()) {
            v.source.alignment = alignments.front();
        }
        break;
    case ViewportKind::CrossSections:
        v.rect = freePlace(sheet, 180.0, 110.0);
        v.scale = 200.0;
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
    std::vector<std::size_t> pages;
    if (!allSheets) {
        if (currentSheet() >= set.sheets.size()) {
            return katana::core::makeError(katana::core::ErrorCode::InvalidArgument, "there is no sheet to plot");
        }
        pages.push_back(currentSheet());
    }
    SheetPaintCache cache;
    std::vector<std::string> problems;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const Status status = plotSheetsToPdf(path, set, pages, source(), 300.0, cache,
                                          QString::fromStdString(document_.metadata().name), &problems);
    QApplication::restoreOverrideCursor();
    if (!status) {
        return status;
    }
    for (const std::string& problem : problems) {
        report(QString::fromStdString(problem), true);
    }
    report(QString("Plotted %1 sheet%2 to %3.")
               .arg(allSheets ? set.sheets.size() : 1)
               .arg(allSheets && set.sheets.size() != 1 ? "s" : "")
               .arg(path));
    return {};
}

void SheetEditor::plotInteractive(bool allSheets)
{
    if (document_.sheetSet().sheets.empty()) {
        report(QStringLiteral("There are no sheets to plot. Generate some first."), true);
        return;
    }
    const QString path = QFileDialog::getSaveFileName(this, allSheets ? QStringLiteral("Plot All Sheets")
                                                                      : QStringLiteral("Plot Sheet"),
                                                      QString(), QStringLiteral("PDF (*.pdf)"));
    if (path.isEmpty()) {
        return;
    }
    if (auto s = plotToPdf(path, allSheets); !s) {
        report(QString::fromStdString(s.error().describe()), true);
    }
}

// ---- Generate Sheets ------------------------------------------------------------------

void SheetEditor::generateSheets()
{
    QDialog dialog(this);
    dialog.setWindowTitle(QStringLiteral("Generate Sheets"));
    dialog.setMinimumWidth(460);
    auto* layout = new QVBoxLayout(&dialog);
    auto* form = new QFormLayout();
    layout->addLayout(form);

    auto* kind = new QComboBox(&dialog);
    kind->addItems({QStringLiteral("The drawing, fitted (one sheet, or tiles at a set scale)"),
                    QStringLiteral("Tiles over the drawing, with a key plan"),
                    QStringLiteral("Plan strips along an alignment"),
                    QStringLiteral("Plan and profile along an alignment"),
                    QStringLiteral("Cross sections along an alignment"),
                    QStringLiteral("One sheet per imported plot frame")});
    form->addRow(QStringLiteral("Layout"), kind);

    auto* paper = new QComboBox(&dialog);
    for (const auto p : kPapers) {
        paper->addItem(paperName(p));
    }
    paper->setCurrentIndex(3);
    form->addRow(QStringLiteral("Paper"), paper);
    auto* scale = scaleBox(&dialog, true);
    form->addRow(QStringLiteral("Scale"), scale);
    auto* alignment = new QComboBox(&dialog);
    for (const std::string& name : alignmentNames(document_)) {
        alignment->addItem(QString::fromStdString(name));
    }
    form->addRow(QStringLiteral("Alignment"), alignment);
    auto* overlap = spin(&dialog, 0.0, 1000.0, 10.0, 1, QStringLiteral(" m"));
    form->addRow(QStringLiteral("Overlap"), overlap);
    auto* keyPlan = new QCheckBox(QStringLiteral("A key plan sheet first"), &dialog);
    keyPlan->setChecked(true);
    form->addRow(QString(), keyPlan);
    auto* interval = spin(&dialog, 0.5, 10000.0, 20.0, 2, QStringLiteral(" m"));
    form->addRow(QStringLiteral("Section interval"), interval);
    auto* halfWidth = spin(&dialog, 0.5, 1000.0, 20.0, 2, QStringLiteral(" m"));
    form->addRow(QStringLiteral("Section half width"), halfWidth);
    auto* rows = new QSpinBox(&dialog);
    rows->setRange(1, 8);
    rows->setValue(3);
    auto* columns = new QSpinBox(&dialog);
    columns->setRange(1, 6);
    columns->setValue(2);
    form->addRow(QStringLiteral("Sections down"), rows);
    form->addRow(QStringLiteral("Sections across"), columns);
    auto* snapshot = new QCheckBox(QStringLiteral("A 3D snapshot beside the plan"), &dialog);
    auto* legend = new QCheckBox(QStringLiteral("A legend beside the plan"), &dialog);
    legend->setChecked(true);
    form->addRow(QString(), snapshot);
    form->addRow(QString(), legend);
    auto* replace = new QCheckBox(QStringLiteral("Replace the sheets there are now"), &dialog);
    form->addRow(QString(), replace);

    const auto enable = [&] {
        const int k = kind->currentIndex();
        const bool along = k >= 2 && k <= 4;
        alignment->setEnabled(along);
        overlap->setEnabled(k == 1 || k == 2);
        keyPlan->setEnabled(k == 1 || k == 2);
        interval->setEnabled(k == 4);
        halfWidth->setEnabled(k == 4);
        rows->setEnabled(k == 4);
        columns->setEnabled(k == 4);
        snapshot->setEnabled(k == 0);
        legend->setEnabled(k == 0);
        scale->setEnabled(k != 5);
    };
    connect(kind, &QComboBox::currentIndexChanged, &dialog, enable);
    enable();

    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    buttons->button(QDialogButtonBox::Ok)->setText(QStringLiteral("Generate"));
    connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    layout->addWidget(buttons);
    if (dialog.exec() != QDialog::Accepted) {
        return;
    }

    plotting::SheetTemplate paperTemplate;
    paperTemplate.paper = kPapers[static_cast<std::size_t>(paper->currentIndex())];
    const std::optional<double> fixed = parseScale(scale->currentText());
    const std::string name = alignment->currentText().toStdString();
    const SheetSource src = source();
    const katana::entity::Model& model = document_.model();

    katana::core::Result<std::vector<Sheet>> sheets = std::vector<Sheet>{};
    const int k = kind->currentIndex();
    const auto needAlignment = [&]() -> katana::core::Result<katana::geometry::SolvedAlignment> {
        const auto* a = model.alignments.find(name);
        if (a == nullptr) {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           "choose an alignment (define one with ALIGN NEW)");
        }
        return katana::geometry::solveAlignment(a->horizontal);
    };
    const Box2 extent = planDrawnBounds(src.plan, {}, {});
    if (k == 0) {
        plotting::LayoutRequest request;
        request.planArea = extent;
        request.scale = fixed.value_or(0.0);
        request.model3d = snapshot->isChecked();
        request.legend = legend->isChecked();
        request.paper = paperTemplate;
        sheets = plotting::smartLayout(model, request);
    } else if (k == 1) {
        plotting::GridRequest request;
        request.area = extent;
        request.scale = fixed.value_or(500.0);
        request.overlapM = overlap->value();
        request.keyPlan = keyPlan->isChecked();
        request.paper = paperTemplate;
        sheets = plotting::gridSheets(request);
    } else if (k == 2 && fixed) {
        auto solved = needAlignment();
        if (!solved) {
            sheets = solved.error();
        } else {
            plotting::StripRequest request;
            request.scale = *fixed;
            request.overlapM = overlap->value();
            request.keyPlan = keyPlan->isChecked();
            request.paper = paperTemplate;
            sheets = plotting::stripSheets(*solved, name, request);
        }
    } else if (k == 2 || k == 3) {
        plotting::LayoutRequest request;
        request.alignment = name;
        request.planAlongAlignment = true;
        request.longSection = k == 3;
        request.scale = fixed.value_or(0.0);
        request.paper = paperTemplate;
        sheets = plotting::smartLayout(model, request);
    } else if (k == 4) {
        auto solved = needAlignment();
        if (!solved) {
            sheets = solved.error();
        } else {
            plotting::CrossSectionRequest request;
            request.interval = interval->value();
            request.halfWidth = halfWidth->value();
            request.rows = static_cast<std::size_t>(rows->value());
            request.columns = static_cast<std::size_t>(columns->value());
            request.scale = fixed.value_or(0.0);
            for (const auto& surface : src.surfaces) {
                if (surface.visible && surface.surface != nullptr) {
                    request.surfaces.push_back({surface.name, surface.surface});
                }
            }
            request.paper = paperTemplate;
            sheets = plotting::crossSectionSheets(*solved, name, request);
        }
    } else {
        std::vector<std::string> skipped;
        sheets = plotting::sheetsFromPlotFrames(model, paperTemplate, &skipped);
        for (const std::string& why : skipped) {
            report(QString::fromStdString(why), true);
        }
    }
    if (!sheets) {
        report(QString::fromStdString(sheets.error().describe()), true);
        return;
    }
    if (sheets->empty()) {
        report(QStringLiteral("Nothing to lay out."), true);
        return;
    }
    const std::size_t count = sheets->size();
    const std::size_t first = replace->isChecked() ? 0 : document_.sheetSet().sheets.size();
    Status status;
    if (replace->isChecked()) {
        status = plotting::editSheetSet(document_, [&sheets](SheetSet& set) {
            set.sheets.clear();
            plotting::prepareForAppend(set, *sheets);
            set.sheets = std::move(*sheets);
            return Status{};
        }, "GENERATE_SHEETS");
    } else {
        status = plotting::addSheets(document_, std::move(*sheets), "GENERATE_SHEETS");
    }
    if (!status) {
        report(QString::fromStdString(status.error().describe()), true);
        return;
    }
    setCurrentSheet(first);
    canvas_->fitPage();
    report(QString("Generated %1 sheet%2.").arg(count).arg(count == 1 ? "" : "s"));
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
