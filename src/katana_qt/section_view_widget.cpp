#include "section_view_widget.hpp"

#include <algorithm>
#include <cmath>

#include <QFocusEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QResizeEvent>
#include <QWheelEvent>

#include "theme.hpp"

namespace katana::qt {

namespace {

// Room for the elevation labels on the left and the station labels below.
constexpr int kLeftMargin = 62;
constexpr int kBottomMargin = 26;
constexpr int kTopMargin = 10;
constexpr int kRightMargin = 12;

constexpr double kZoomPerNotch = 1.15;

const QColor kBackground(24, 26, 32);
const QColor kGrid(48, 52, 62);
const QColor kGridMajor(74, 80, 94);
const QColor kAxis(150, 158, 175);
const QColor kText(196, 202, 214);

// A distinct colour per surface, in the order they were cut. Existing ground
// first, then design: the two a section almost always carries.
const QColor kSurfaceColors[] = {QColor(120, 200, 120), QColor(235, 170, 80),
                                 QColor(130, 175, 245), QColor(220, 120, 200),
                                 QColor(230, 230, 130)};

// The plot area - inside the axes and their labels - of a widget this size.
// Not clamped: resizeEvent takes the difference of two of these, and a clamp
// would make that difference wrong for a widget smaller than its margins.
QSizeF plotSize(const QSize& widget)
{
    return QSizeF(widget.width() - kLeftMargin - kRightMargin,
                  widget.height() - kTopMargin - kBottomMargin);
}

} // namespace

SectionViewWidget::SectionViewWidget(katana::cad::ViewState& state, QWidget* parent)
    : QWidget(parent), state_(state)
{
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setAttribute(Qt::WA_OpaquePaintEvent, true);
    setMinimumSize(80, 60);
}

void SectionViewWidget::setSection(katana::cad::Section section)
{
    state_.section = std::move(section);
    framed_ = false;
    update();
}

void SectionViewWidget::clearSection()
{
    state_.section.reset();
    framed_ = false;
    update();
}

void SectionViewWidget::setVerticalExaggeration(double factor)
{
    if (!std::isfinite(factor) || factor <= 0.0) {
        return;
    }
    state_.sectionExaggeration = factor;
    framed_ = false; // the drawing's proportions changed; reframe it
    update();
}

QPointF SectionViewWidget::toScreen(double station, double elevation) const
{
    const double x = kLeftMargin + (station - originStation_) * scale_;
    const double y = static_cast<double>(height() - kBottomMargin) -
                     (elevation - originElevation_) * scale_ * exaggeration();
    return QPointF(x, y);
}

double SectionViewWidget::stationAt(double x) const
{
    return originStation_ + (x - kLeftMargin) / scale_;
}

double SectionViewWidget::elevationAt(double y) const
{
    return originElevation_ +
           (static_cast<double>(height() - kBottomMargin) - y) / (scale_ * exaggeration());
}

QPointF SectionViewWidget::stationElevationAt(const QPointF& pixel) const
{
    return QPointF(stationAt(pixel.x()), elevationAt(pixel.y()));
}

QPointF SectionViewWidget::plotCentre() const
{
    const QSizeF plot = plotSize(size());
    return QPointF(kLeftMargin + 0.5 * plot.width(), kTopMargin + 0.5 * plot.height());
}

void SectionViewWidget::zoomExtents()
{
    frameExtents();
    update();
}

void SectionViewWidget::frameExtents()
{
    framed_ = true;
    const int plotWidth = std::max(width() - kLeftMargin - kRightMargin, 1);
    const int plotHeight = std::max(height() - kTopMargin - kBottomMargin, 1);

    const std::optional<katana::cad::Section>& section = state_.section;
    if (!section.has_value()) {
        scale_ = 1.0;
        originStation_ = 0.0;
        originElevation_ = 0.0;
        return;
    }
    const auto box = section->extent();
    if (box.empty()) {
        // The alignment missed every surface. Frame the station range anyway so
        // the axis still reads correctly and the gap is visible as a gap.
        scale_ = static_cast<double>(plotWidth) / std::max(section->length, 1.0);
        originStation_ = 0.0;
        originElevation_ = 0.0;
        return;
    }

    const double stationSpan = std::max(box.width(), 1e-6);
    const double elevationSpan = std::max(box.height(), 1e-6);

    // Fit whichever axis is tighter, with the exaggeration already applied to
    // the vertical one so that changing it reframes rather than clipping.
    const double horizontal = static_cast<double>(plotWidth) / (stationSpan * 1.04);
    const double vertical =
        static_cast<double>(plotHeight) / (elevationSpan * exaggeration() * 1.15);
    scale_ = std::max(std::min(horizontal, vertical), 1e-9);

    // Centre what is left over.
    const double usedWidth = stationSpan * scale_;
    const double usedHeight = elevationSpan * scale_ * exaggeration();
    originStation_ = box.min.x - (static_cast<double>(plotWidth) - usedWidth) * 0.5 / scale_;
    originElevation_ = box.min.y - (static_cast<double>(plotHeight) - usedHeight) * 0.5 /
                                       (scale_ * exaggeration());
}

double SectionViewWidget::niceStep(double minimumPixels, bool vertical) const
{
    const double pixelsPerUnit = vertical ? scale_ * exaggeration() : scale_;
    if (!(pixelsPerUnit > 0.0)) {
        return 1.0;
    }
    const double raw = minimumPixels / pixelsPerUnit;
    const double magnitude = std::pow(10.0, std::floor(std::log10(std::max(raw, 1e-12))));
    for (const double multiple : {1.0, 2.0, 5.0, 10.0}) {
        if (magnitude * multiple >= raw) {
            return magnitude * multiple;
        }
    }
    return magnitude * 10.0;
}

void SectionViewWidget::drawGrid(QPainter& painter) const
{
    const QRectF plot(kLeftMargin, kTopMargin, width() - kLeftMargin - kRightMargin,
                      height() - kTopMargin - kBottomMargin);
    if (plot.width() <= 0 || plot.height() <= 0) {
        return;
    }
    painter.setClipRect(plot.adjusted(-1, -1, 1, 1));

    const double stationStep = niceStep(70.0, false);
    const double elevationStep = niceStep(40.0, true);

    const double firstStation = std::ceil(stationAt(plot.left()) / stationStep) * stationStep;
    const double lastStation = stationAt(plot.right());
    const double firstElevation =
        std::ceil(elevationAt(plot.bottom()) / elevationStep) * elevationStep;
    const double lastElevation = elevationAt(plot.top());

    painter.setPen(QPen(kGrid, 1.0));
    // Bounded so a wildly zoomed-out view cannot ask for a hundred thousand
    // lines; niceStep already keeps the count near the plot size, and this is
    // the belt to its braces.
    int drawn = 0;
    for (double s = firstStation; s <= lastStation && drawn < 500; s += stationStep, ++drawn) {
        const double x = toScreen(s, 0.0).x();
        painter.drawLine(QPointF(x, plot.top()), QPointF(x, plot.bottom()));
    }
    drawn = 0;
    for (double e = firstElevation; e <= lastElevation && drawn < 500;
         e += elevationStep, ++drawn) {
        const double y = toScreen(0.0, e).y();
        painter.setPen(QPen(std::abs(e) < elevationStep * 0.5 ? kGridMajor : kGrid, 1.0));
        painter.drawLine(QPointF(plot.left(), y), QPointF(plot.right(), y));
    }

    painter.setClipping(false);
    painter.setPen(kAxis);
    painter.drawLine(plot.bottomLeft(), plot.bottomRight());
    painter.drawLine(plot.topLeft(), plot.bottomLeft());

    painter.setPen(kText);
    const QFont original = painter.font();
    QFont small = original;
    small.setPointSizeF(std::max(original.pointSizeF() - 1.0, 6.0));
    painter.setFont(small);

    drawn = 0;
    for (double s = firstStation; s <= lastStation && drawn < 200; s += stationStep, ++drawn) {
        const double x = toScreen(s, 0.0).x();
        painter.drawText(QRectF(x - 40.0, plot.bottom() + 3.0, 80.0, kBottomMargin - 4.0),
                         Qt::AlignHCenter | Qt::AlignTop, QString::number(s, 'f', 1));
    }
    drawn = 0;
    for (double e = firstElevation; e <= lastElevation && drawn < 200;
         e += elevationStep, ++drawn) {
        const double y = toScreen(0.0, e).y();
        painter.drawText(QRectF(2.0, y - 8.0, kLeftMargin - 8.0, 16.0),
                         Qt::AlignRight | Qt::AlignVCenter, QString::number(e, 'f', 2));
    }
    painter.setFont(original);
}

void SectionViewWidget::drawSurfaces(QPainter& painter) const
{
    const std::optional<katana::cad::Section>& section = state_.section;
    if (!section.has_value()) {
        return;
    }
    const QRectF plot(kLeftMargin, kTopMargin, width() - kLeftMargin - kRightMargin,
                      height() - kTopMargin - kBottomMargin);
    painter.setClipRect(plot);
    painter.setRenderHint(QPainter::Antialiasing, true);

    constexpr std::size_t kColorCount = sizeof(kSurfaceColors) / sizeof(kSurfaceColors[0]);
    for (std::size_t s = 0; s < section->surfaces.size(); ++s) {
        const auto& surface = section->surfaces[s];
        painter.setPen(QPen(kSurfaceColors[s % kColorCount], 1.8));

        // Broken into runs at every gap, so a stretch where the alignment left
        // the surface is a break in the line rather than a straight segment
        // across ground that was never measured.
        QPolygonF run;
        for (const auto& sample : surface.samples) {
            if (!sample.elevation.has_value()) {
                if (run.size() > 1) {
                    painter.drawPolyline(run);
                }
                run.clear();
                continue;
            }
            run.append(toScreen(sample.station, *sample.elevation));
        }
        if (run.size() > 1) {
            painter.drawPolyline(run);
        }

        // Slope breaks marked, because on a profile those are the points that
        // get levelled and set out.
        painter.setPen(QPen(kSurfaceColors[s % kColorCount], 1.0));
        painter.setBrush(kSurfaceColors[s % kColorCount]);
        for (std::size_t i = 0; i < surface.samples.size(); ++i) {
            if (surface.reasons[i] != katana::cad::SampleReason::SurfaceBreak ||
                !surface.samples[i].elevation.has_value()) {
                continue;
            }
            painter.drawEllipse(toScreen(surface.samples[i].station,
                                         *surface.samples[i].elevation),
                                2.2, 2.2);
        }
        painter.setBrush(Qt::NoBrush);
    }
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setClipping(false);
}

void SectionViewWidget::drawCrossings(QPainter& painter) const
{
    lastDrawnCrossings_ = 0;
    lastHiddenCrossings_ = 0;
    const std::optional<katana::cad::Section>& section = state_.section;
    if (!section.has_value() || section->crossings.empty()) {
        return;
    }
    const QRectF plot(kLeftMargin, kTopMargin, width() - kLeftMargin - kRightMargin,
                      height() - kTopMargin - kBottomMargin);
    painter.setClipRect(plot);
    painter.setPen(QPen(QColor(210, 90, 90), 1.0, Qt::DashLine));

    for (const auto& crossing : section->crossings) {
        // A crossing is where an entity on a layer cuts the section line, so
        // a layer hidden in this view takes its crossings out of it too. At
        // paint time rather than by cutting the section again: the section
        // is shared, and which layers a view hides changes far more often.
        if (state_.layers.hides(crossing.layer)) {
            ++lastHiddenCrossings_;
            continue;
        }
        const double x = toScreen(crossing.station, 0.0).x();
        if (x < plot.left() || x > plot.right()) {
            continue;
        }
        painter.drawLine(QPointF(x, plot.top()), QPointF(x, plot.bottom()));
        ++lastDrawnCrossings_;
    }
    painter.setClipping(false);
}

void SectionViewWidget::drawLegend(QPainter& painter) const
{
    const std::optional<katana::cad::Section>& section = state_.section;
    if (!section.has_value()) {
        return;
    }
    constexpr std::size_t kColorCount = sizeof(kSurfaceColors) / sizeof(kSurfaceColors[0]);
    int y = kTopMargin + 4;
    painter.setPen(kText);
    painter.drawText(QPointF(kLeftMargin + 8, y + 10),
                     QString("L %1   V x%2")
                         .arg(section->length, 0, 'f', 2)
                         .arg(exaggeration(), 0, 'f', 1));
    y += 16;
    for (std::size_t s = 0; s < section->surfaces.size(); ++s) {
        painter.setPen(QPen(kSurfaceColors[s % kColorCount], 2.0));
        painter.drawLine(kLeftMargin + 8, y + 6, kLeftMargin + 26, y + 6);
        painter.setPen(kText);
        painter.drawText(QPointF(kLeftMargin + 32, y + 10),
                         QString::fromStdString(section->surfaces[s].name));
        y += 15;
    }
}

void SectionViewWidget::paintEvent(QPaintEvent* /*event*/)
{
    QPainter painter(this);
    painter.fillRect(rect(), kBackground);

    // At the first paint, not the first resize: a paint happens at the size
    // the view is seen at, where a dock's first resize may be provisional.
    if (!framed_) {
        frameExtents();
    }

    const std::optional<katana::cad::Section>& section = state_.section;
    if (!section.has_value()) {
        lastDrawnCrossings_ = 0;
        lastHiddenCrossings_ = 0;
        painter.setPen(theme::textMuted());
        painter.drawText(rect().adjusted(12, 12, -12, -12), Qt::AlignCenter | Qt::TextWordWrap,
                         QStringLiteral("No section yet.\nSelect a line or polyline, then "
                                        "Terrain > Cut Section Along Selection."));
        return;
    }

    drawGrid(painter);
    drawCrossings(painter);
    drawSurfaces(painter);
    drawLegend(painter);

    if (onFrameStats) {
        QString text = QString("Section  length %1  %2 crossings")
                           .arg(section->length, 0, 'f', 2)
                           .arg(section->crossings.size());
        if (lastHiddenCrossings_ > 0) {
            text += QString(", %1 hidden in this view").arg(lastHiddenCrossings_);
        }
        onFrameStats(text);
    }
}

void SectionViewWidget::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    // Resizing a dock used to throw the user's pan and zoom away and frame
    // the whole section again. Now the station and elevation at the middle of
    // the plot area stay there and the scale is kept, as cad::ViewTransform
    // keeps its centre: the plot area grows or shrinks about its middle. A
    // section never framed yet has nothing to keep; its first paint frames it.
    if (!framed_ || !event->oldSize().isValid()) {
        return;
    }
    const QSizeF before = plotSize(event->oldSize());
    const QSizeF after = plotSize(event->size());
    // The station at the plot's middle is origin + width / (2 scale), and the
    // elevation there origin + height / (2 scale exaggeration); holding each
    // fixed while the width or height changes moves the origin by half the
    // change.
    originStation_ += 0.5 * (before.width() - after.width()) / scale_;
    originElevation_ += 0.5 * (before.height() - after.height()) / (scale_ * exaggeration());
}

void SectionViewWidget::focusInEvent(QFocusEvent* event)
{
    QWidget::focusInEvent(event);
    // As a click does; ViewportWidget::focusInEvent says why not the focus a
    // closing menu gives back.
    if (onActivated && event->reason() != Qt::PopupFocusReason) {
        onActivated();
    }
}

void SectionViewWidget::mousePressEvent(QMouseEvent* event)
{
    if (onActivated) {
        onActivated();
    }
    setFocus(Qt::MouseFocusReason);
    if (event->button() == Qt::MiddleButton ||
        (event->button() == Qt::LeftButton && (event->modifiers() & Qt::ShiftModifier) != 0)) {
        panning_ = true;
        lastMouse_ = event->pos();
    }
}

void SectionViewWidget::mouseMoveEvent(QMouseEvent* event)
{
    if (panning_) {
        const QPoint delta = event->pos() - lastMouse_;
        lastMouse_ = event->pos();
        originStation_ -= delta.x() / scale_;
        originElevation_ += delta.y() / (scale_ * exaggeration());
        update();
        return;
    }
    if (!state_.section.has_value() || !onFrameStats) {
        return;
    }
    // Live readout: station and elevation under the cursor is the number
    // someone actually wants off a section.
    onFrameStats(QString("Station %1   Elevation %2   (V x%3)")
                     .arg(stationAt(event->pos().x()), 0, 'f', 3)
                     .arg(elevationAt(event->pos().y()), 0, 'f', 3)
                     .arg(exaggeration(), 0, 'f', 1));
}

void SectionViewWidget::mouseReleaseEvent(QMouseEvent* /*event*/) { panning_ = false; }

void SectionViewWidget::mouseDoubleClickEvent(QMouseEvent* /*event*/) { zoomExtents(); }

void SectionViewWidget::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches == 0.0) {
        return;
    }
    // Zoom about the cursor: the station and elevation under it must not move.
    const QPointF position = event->position();
    const double station = stationAt(position.x());
    const double elevation = elevationAt(position.y());

    scale_ = std::clamp(scale_ * std::pow(kZoomPerNotch, notches), 1e-9, 1e9);

    originStation_ = station - (position.x() - kLeftMargin) / scale_;
    originElevation_ = elevation - (static_cast<double>(height() - kBottomMargin) - position.y()) /
                                       (scale_ * exaggeration());
    framed_ = true;
    update();
    event->accept();
}

} // namespace katana::qt
