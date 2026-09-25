#include "plotting/sheet_rulers.hpp"

#include <algorithm>
#include <array>
#include <cmath>

#include <QColor>
#include <QFont>
#include <QPainter>
#include <QPen>

namespace katana::qt {

namespace {

const QColor kRulerGround(236, 237, 240);
const QColor kRulerPaper(255, 255, 255);
const QColor kRulerInk(70, 72, 78);
const QColor kRulerHighlight(0, 120, 215, 55);
const QColor kRulerCursor(220, 40, 60);
const QColor kGridLine(0, 120, 215, 26);
const QColor kGridMajor(0, 120, 215, 52);

constexpr double kNumberSpacingPixels = 50.0;
constexpr double kTickSpacingPixels = 5.0;
constexpr double kGridSpacingPixels = 4.0;

// The whole steps `step` apart from `from` to `to` (either way round), by
// index so that a long ruler does not add up rounding.
template <typename Visit> void eachStep(double from, double to, double step, Visit&& visit)
{
    if (!(step > 0.0)) {
        return;
    }
    const double lo = std::min(from, to);
    const double hi = std::max(from, to);
    const auto first = static_cast<long long>(std::ceil(lo / step));
    const auto last = static_cast<long long>(std::floor(hi / step));
    for (long long i = first; i <= last; ++i) {
        visit(static_cast<double>(i) * step, i);
    }
}

QString number(double mm)
{
    return QString::number(mm, 'f', mm == std::floor(mm) ? 0 : 1);
}

} // namespace

RulerSteps rulerSteps(double pixelsPerMillimetre)
{
    static constexpr std::array<double, 12> kSteps{1.0,   2.0,   5.0,   10.0,   20.0,   50.0,
                                                   100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0};
    RulerSteps steps{kSteps.back(), kSteps.back() / 10.0};
    const double ppmm = pixelsPerMillimetre > 0.0 ? pixelsPerMillimetre : 1.0;
    for (const double step : kSteps) {
        if (step * ppmm >= kNumberSpacingPixels) {
            steps.numbered = step;
            break;
        }
    }
    // A tenth, a fifth or a half of the numbered step: the finest that is
    // not crowded.
    steps.tick = steps.numbered;
    for (const double divisor : {10.0, 5.0, 2.0}) {
        if (steps.numbered / divisor * ppmm >= kTickSpacingPixels) {
            steps.tick = steps.numbered / divisor;
            break;
        }
    }
    return steps;
}

void paintSheetRulers(QPainter& painter, const QSize& size, const RulerView& view)
{
    const double r = kSheetRulerPixels;
    const double ppmm = view.pixelsPerMillimetre;
    if (!(ppmm > 0.0)) {
        return;
    }
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, false);
    const QRectF top(r, 0.0, size.width() - r, r);
    const QRectF left(0.0, r, r, size.height() - r);
    painter.fillRect(top, kRulerGround);
    painter.fillRect(left, kRulerGround);

    // Paper millimetres to the widget and back, along each ruler.
    const auto xAt = [&](double mm) { return view.paperOrigin.x() + mm * ppmm; };
    const auto yAt = [&](double mm) { return view.paperOrigin.y() + (view.paperHeightMm - mm) * ppmm; };
    const auto mmAtX = [&](double x) { return (x - view.paperOrigin.x()) / ppmm; };
    const auto mmAtY = [&](double y) { return view.paperHeightMm - (y - view.paperOrigin.y()) / ppmm; };

    // The paper's span, white; the selection's, shaded.
    painter.fillRect(QRectF(QPointF(std::max(xAt(0.0), r), 0.0),
                            QPointF(std::max(xAt(view.paperWidthMm), r), r)),
                     kRulerPaper);
    painter.fillRect(QRectF(QPointF(0.0, std::max(yAt(view.paperHeightMm), r)),
                            QPointF(r, std::max(yAt(0.0), r))),
                     kRulerPaper);
    if (!view.highlight.empty()) {
        painter.fillRect(QRectF(QPointF(std::max(xAt(view.highlight.min.x), r), 0.0),
                                QPointF(std::max(xAt(view.highlight.max.x), r), r)),
                         kRulerHighlight);
        painter.fillRect(QRectF(QPointF(0.0, std::max(yAt(view.highlight.max.y), r)),
                                QPointF(r, std::max(yAt(view.highlight.min.y), r))),
                         kRulerHighlight);
    }

    const RulerSteps steps = rulerSteps(ppmm);
    const auto perNumber = static_cast<long long>(std::llround(steps.numbered / steps.tick));
    QFont font = painter.font();
    font.setPixelSize(9);
    painter.setFont(font);
    painter.setPen(QPen(kRulerInk, 0.0));
    const auto tickLength = [&](long long i) {
        if (i % perNumber == 0) {
            return r * 0.55;
        }
        return perNumber % 2 == 0 && i % (perNumber / 2) == 0 ? r * 0.35 : r * 0.2;
    };

    // Along the top.
    painter.save();
    painter.setClipRect(top);
    eachStep(mmAtX(r), mmAtX(size.width()), steps.tick, [&](double mm, long long i) {
        const double x = std::round(xAt(mm)) + 0.5;
        painter.drawLine(QPointF(x, r), QPointF(x, r - tickLength(i)));
        if (i % perNumber == 0) {
            painter.drawText(QPointF(x + 2.0, 9.0), number(mm));
        }
    });
    painter.restore();

    // Down the left, its numbers turned to read from below.
    painter.save();
    painter.setClipRect(left);
    eachStep(mmAtY(size.height()), mmAtY(r), steps.tick, [&](double mm, long long i) {
        const double y = std::round(yAt(mm)) + 0.5;
        painter.drawLine(QPointF(r, y), QPointF(r - tickLength(i), y));
        if (i % perNumber == 0) {
            painter.save();
            painter.translate(9.0, y - 2.0);
            painter.rotate(-90.0);
            painter.drawText(QPointF(0.0, 0.0), number(mm));
            painter.restore();
        }
    });
    painter.restore();

    // The cursor on both.
    if (view.cursor) {
        painter.setPen(QPen(kRulerCursor, 1.0));
        const double x = xAt(view.cursor->x);
        const double y = yAt(view.cursor->y);
        if (x >= r) {
            painter.drawLine(QPointF(x, 0.0), QPointF(x, r));
        }
        if (y >= r) {
            painter.drawLine(QPointF(0.0, y), QPointF(r, y));
        }
    }

    // Their edges, and the corner.
    painter.setPen(QPen(kRulerInk.lighter(160), 1.0));
    painter.drawLine(QPointF(r, r - 0.5), QPointF(size.width(), r - 0.5));
    painter.drawLine(QPointF(r - 0.5, r), QPointF(r - 0.5, size.height()));
    painter.fillRect(QRectF(0.0, 0.0, r, r), kRulerGround);
    painter.setPen(QPen(kRulerInk, 0.0));
    QFont small = font;
    small.setPixelSize(8);
    painter.setFont(small);
    painter.drawText(QRectF(0.0, 0.0, r, r), Qt::AlignCenter, QStringLiteral("mm"));
    painter.restore();
}

void paintPaperGrid(QPainter& painter, const RulerView& view, double spacingMm)
{
    const double ppmm = view.pixelsPerMillimetre;
    if (!(spacingMm > 0.0) || !(ppmm > 0.0)) {
        return;
    }
    // Every line, or every 2nd, 5th or 10th of them where they would crowd.
    long long every = 1;
    for (const long long skip : {1LL, 2LL, 5LL, 10LL, 20LL, 50LL}) {
        every = skip;
        if (spacingMm * static_cast<double>(skip) * ppmm >= kGridSpacingPixels) {
            break;
        }
    }
    const double step = spacingMm * static_cast<double>(every);
    const QRectF paper(view.paperOrigin,
                       QSizeF(view.paperWidthMm * ppmm, view.paperHeightMm * ppmm));
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setClipRect(paper, Qt::IntersectClip);
    const QPen minor(kGridLine, 0.0);
    const QPen major(kGridMajor, 0.0);
    // Every tenth line of the grid (not of what is drawn) is a major one.
    const long long majorEvery = std::max(10LL / every, 1LL);
    eachStep(0.0, view.paperWidthMm, step, [&](double mm, long long i) {
        painter.setPen(i % majorEvery == 0 ? major : minor);
        const double x = std::round(view.paperOrigin.x() + mm * ppmm) + 0.5;
        painter.drawLine(QPointF(x, paper.top()), QPointF(x, paper.bottom()));
    });
    eachStep(0.0, view.paperHeightMm, step, [&](double mm, long long i) {
        painter.setPen(i % majorEvery == 0 ? major : minor);
        const double y =
            std::round(view.paperOrigin.y() + (view.paperHeightMm - mm) * ppmm) + 0.5;
        painter.drawLine(QPointF(paper.left(), y), QPointF(paper.right(), y));
    });
    painter.restore();
}

} // namespace katana::qt
