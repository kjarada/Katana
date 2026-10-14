// The drawing's surfaces in the plan view, each in its display style
// (terrain/surface_plan_painter.hpp, docs/surface_display.md "In plan").

#include "terrain/surface_plan_painter.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>

#include <QFontMetricsF>
#include <QImage>
#include <QLinearGradient>
#include <QLineF>
#include <QList>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QPolygonF>
#include <QTransform>

#include "plotting/plot_style.hpp"

namespace katana::qt {

namespace cad = katana::cad;
using katana::geometry::Box2;
using katana::geometry::Point2;

namespace {

QColor toQColor(const katana::entity::Color& colour)
{
    return QColor(colour.r, colour.g, colour.b, colour.a);
}

// A colour as the medium prints it: on paper the plot's rule (white prints
// black, greyscale, monochrome - cad::paperColour), on screen as it is.
QColor inkOf(const katana::entity::Color& colour, const SurfacePaintOptions& options)
{
    if (options.paper && options.plot != nullptr) {
        return toQColor(cad::paperColour(colour, *options.plot));
    }
    return toQColor(colour);
}

// A pen of `pixels` on screen - cosmetic, so a turn or a scale of the painter
// cannot thicken it - or of `millimetres` of paper.
QPen penOf(const QColor& colour, double pixels, double millimetres, const SurfacePaintOptions& options)
{
    if (options.paper) {
        QPen pen(colour, millimetres * options.pixelsPerMillimetre);
        pen.setCapStyle(Qt::RoundCap);
        pen.setJoinStyle(Qt::RoundJoin);
        return pen;
    }
    QPen pen(colour, pixels);
    pen.setCosmetic(true);
    return pen;
}

QPolygonF toScreen(const cad::ViewTransform& view, const std::vector<Point2>& points)
{
    QPolygonF polygon;
    polygon.reserve(static_cast<qsizetype>(points.size()));
    for (const Point2& point : points) {
        const Point2 p = view.worldToScreen(point);
        polygon << QPointF(p.x, p.y);
    }
    return polygon;
}

} // namespace

void paintSurfacePlan(QPainter& painter, const cad::SurfacePlan& plan, const cad::ViewTransform& view,
                      const SurfacePaintOptions& options)
{
    painter.save();

    // The faces, the tint of what is too dense to stroke, and points too
    // dense to mark: one image, wrapped without a copy and laid where its
    // box is, each image pixel a device pixel on an unturned screen.
    if (!plan.image.empty()) {
        const QImage image(reinterpret_cast<const uchar*>(plan.image.pixels.data()), plan.image.width,
                           plan.image.height, plan.image.width * 4,
                           QImage::Format_ARGB32_Premultiplied);
        const double size = plan.image.box.width() / plan.image.width;
        const Point2 corner = view.worldToScreen(Point2(plan.image.box.min.x, plan.image.box.max.y));
        const double step = size * view.scale;
        painter.save();
        painter.setTransform(QTransform(step, 0.0, 0.0, step, corner.x, corner.y), true);
        // Smoothed only on paper, where the image is coarser than the
        // device; on screen each image pixel is a device pixel.
        painter.setRenderHint(QPainter::SmoothPixmapTransform, options.paper);
        if (options.paper && options.plot != nullptr &&
            options.plot->colourMode != cad::PlotColourMode::Colour) {
            painter.drawImage(QPointF(0.0, 0.0), plotImage(image, *options.plot));
        } else {
            painter.drawImage(QPointF(0.0, 0.0), image);
        }
        painter.restore();
    }

    painter.setBrush(Qt::NoBrush);
    if (!plan.triangles.empty()) {
        painter.setPen(penOf(inkOf(plan.triangleColour, options), 1.0, kSurfaceTrianglePaperMillimetres,
                             options));
        QList<QLineF> lines;
        lines.reserve(static_cast<qsizetype>(plan.triangles.size()));
        for (const katana::geometry::Segment2& edge : plan.triangles) {
            const Point2 a = view.worldToScreen(edge.start);
            const Point2 b = view.worldToScreen(edge.end);
            lines.append(QLineF(a.x, a.y, b.x, b.y));
        }
        painter.drawLines(lines);
    }
    // Minors then majors (surfacePlan's order), each in its own pen.
    if (!plan.contours.empty()) {
        const QPen minor = penOf(inkOf(plan.minorColour, options), 1.0,
                                 kSurfaceMinorContourPaperMillimetres, options);
        const QPen major = penOf(inkOf(plan.majorColour, options), kSurfaceHeavyLinePixels,
                                 kSurfaceMajorContourPaperMillimetres, options);
        for (const cad::SurfacePlanContour& contour : plan.contours) {
            painter.setPen(contour.major ? major : minor);
            painter.drawPolyline(toScreen(view, contour.points));
        }
    }
    if (!plan.breaklines.empty()) {
        painter.setPen(penOf(inkOf(plan.breaklineColour, options), kSurfaceHeavyLinePixels,
                             kSurfaceBreaklinePaperMillimetres, options));
        for (const auto& line : plan.breaklines) {
            painter.drawPolyline(toScreen(view, line));
        }
    }
    if (!plan.boundary.empty()) {
        painter.setPen(penOf(inkOf(plan.boundaryColour, options), kSurfaceHeavyLinePixels,
                             kSurfaceBoundaryPaperMillimetres, options));
        for (const auto& line : plan.boundary) {
            painter.drawPolyline(toScreen(view, line));
        }
    }
    if (!plan.points.empty()) {
        const double side = options.paper ? kSurfacePointPaperMillimetres * options.pixelsPerMillimetre
                                          : kSurfacePointPixels;
        painter.setPen(Qt::NoPen);
        painter.setBrush(inkOf(plan.pointColour, options));
        QList<QRectF> marks;
        marks.reserve(static_cast<qsizetype>(plan.points.size()));
        for (const Point2& point : plan.points) {
            const Point2 p = view.worldToScreen(point);
            marks.append(QRectF(p.x - side / 2.0, p.y - side / 2.0, side, side));
        }
        painter.drawRects(marks);
        painter.setBrush(Qt::NoBrush);
    }

    // The labels on the majors, each haloed in the ground so it reads where
    // the contour runs through it, and turned to read upright on the device
    // whatever the contour's direction and the frame's turn.
    if (!plan.labels.empty()) {
        QFont font(options.fontFamily);
        const double pixels = options.paper ? kSurfaceLabelPaperMillimetres * options.pixelsPerMillimetre
                                            : kSurfaceLabelPixels;
        // A whole pixel size: at 300 dpi a 1.8 mm label is 21.3 px and set
        // at 21, 0.03 mm short - below a pen's width.
        font.setPixelSize(std::max(1, static_cast<int>(std::lround(pixels))));
        const QFontMetricsF metrics(font);
        // In the majors' colour - unless that is too near the halo's to be
        // read (a dark brown major on the screen's dark ground): then lighter
        // on a dark halo, darker on a light one. Luminance by the BT.709
        // weights on the sRGB values, near enough for "can this be read";
        // the 0.3 is chosen here, by looking: the default major brown (110,
        // 70, 40), 0.30, on the screen's ground (30, 35, 41), 0.13, is 0.16
        // apart and hard to read; the minor brown (160, 120, 80), 0.49, is
        // 0.36 apart and plain.
        QColor ink = inkOf(plan.majorColour, options);
        const auto luminance = [](const QColor& colour) {
            return 0.2126 * colour.redF() + 0.7152 * colour.greenF() + 0.0722 * colour.blueF();
        };
        if (std::abs(luminance(ink) - luminance(options.halo)) < 0.3) {
            ink = luminance(options.halo) < 0.5 ? ink.lighter(200) : ink.darker(200);
        }
        QPen halo(options.halo, std::max(2.0, 0.25 * pixels));
        halo.setJoinStyle(Qt::RoundJoin);
        constexpr double kDegrees = 180.0 / std::numbers::pi;
        for (const cad::SurfacePlanLabel& label : plan.labels) {
            const Point2 at = view.worldToScreen(label.at);
            // The contour's direction on the device, clockwise in degrees
            // (screen y runs down; the frame turns the drawing
            // counter-clockwise by `rotation`).
            double device = -(label.angle + options.rotation) * kDegrees;
            double turn = -label.angle * kDegrees;
            device = std::remainder(device, 360.0);
            if (device > 90.0 || device < -90.0) {
                turn += 180.0;
            }
            const QString text = QString::fromStdString(label.text);
            QPainterPath path;
            path.addText(-metrics.horizontalAdvance(text) / 2.0, (metrics.ascent() - metrics.descent()) / 2.0,
                         font, text);
            painter.save();
            painter.translate(at.x, at.y);
            painter.rotate(turn);
            painter.strokePath(path, halo);
            painter.fillPath(path, ink);
            painter.restore();
        }
    }
    painter.restore();
}

SurfacePaintStats paintSurfacesInPlan(QPainter& painter, const katana::terrain::SurfaceStore& surfaces,
                                      const cad::SurfaceStyleSet& styles, const cad::ViewTransform& view,
                                      const Box2& visible, const SurfacePaintOptions& options,
                                      cad::SurfacePlanCache& cache)
{
    SurfacePaintStats stats;
    // Keep what the drawing still has, hidden or not: showing a surface again
    // must not build its grid again.
    std::vector<const katana::terrain::TinSurface*> alive;
    for (const katana::terrain::NamedSurface& named : surfaces.all()) {
        alive.push_back(named.surface.get());
    }
    cache.retain(alive);
    if (!(view.scale > 0.0) || visible.empty()) {
        return stats;
    }

    // A view pixel in the model, which the level of detail measures: a
    // logical pixel on screen, kPlanPaperPixelMillimetres of paper on paper.
    const double unitsPerPixel =
        options.paper ? cad::kPlanPaperPixelMillimetres * options.pixelsPerMillimetre / view.scale
                      : 1.0 / view.scale;
    // An image pixel: a device pixel on screen; on paper the device's pixel
    // at the imagery share (the dpi cap).
    const double imagePixel = options.paper
                                  ? 1.0 / (view.scale * std::clamp(options.imageryShare, 1.0e-6, 1.0))
                                  : 1.0 / (view.scale * std::max(options.deviceRatio, 1.0e-6));
    for (const katana::terrain::NamedSurface& named : surfaces.all()) {
        if (!named.visible || named.surface == nullptr || named.surface->empty() ||
            !named.surface->bounds().intersects(visible)) {
            continue;
        }
        const cad::SurfaceDisplayStyle& style = cad::styleOf(styles, named);
        cad::SurfacePlanView plan;
        plan.visible = visible;
        plan.unitsPerPixel = unitsPerPixel;
        const cad::SurfacePlanImageGrid grid =
            cad::planImageGrid(visible, named.surface->bounds(), imagePixel, options.imagePixelCap);
        plan.imageBox = grid.box;
        plan.imageWidth = grid.width;
        plan.imageHeight = grid.height;
        const cad::SurfacePlan drawn = cad::surfacePlan(named, style, plan, cache);
        if (drawn.stats.trianglesInView == 0) {
            continue;
        }
        paintSurfacePlan(painter, drawn, view, options);
        ++stats.surfacesDrawn;
        stats.trianglesInView += drawn.stats.trianglesInView;
        stats.edgesStroked += drawn.stats.edgesStroked;
        stats.trianglesTinted += drawn.stats.trianglesTinted;
        stats.contourRuns += drawn.stats.contourRuns;
        stats.labels += drawn.stats.labels;
        stats.imagePixels += drawn.stats.imagePixels;
        stats.pixelsCovered += drawn.stats.pixelsCovered;
        if (!drawn.problem.empty()) {
            stats.problems.push_back(named.name + ": " + drawn.problem);
        }
        if (drawn.legend.has_value()) {
            stats.legends.push_back({named.name, style.name, *drawn.legend});
        }
    }
    return stats;
}

QRectF paintSurfaceLegend(QPainter& painter, const cad::SurfaceLegend& legend, const QString& caption,
                          const QPointF& topRight, const QFont& font, const QColor& text,
                          const QColor& backing)
{
    if (legend.entries.empty()) {
        return {};
    }
    const QFontMetricsF metrics(font);
    const double line = metrics.height();
    const double pad = 6.0;
    const double swatch = std::max(10.0, line * 0.8);
    const bool gradient = legend.gradient;
    // The rows: a gradient's two ends, or an entry each up to the cap, the
    // last saying how many are left out.
    struct Row {
        QColor colour;
        QString label;
        bool more = false;
    };
    std::vector<Row> rows;
    if (!gradient) {
        const std::size_t shown = legend.entries.size() > static_cast<std::size_t>(kLegendRows)
                                      ? static_cast<std::size_t>(kLegendRows) - 1
                                      : legend.entries.size();
        // Highest first, as a map key reads: high ground at the top.
        for (std::size_t i = 0; i < shown; ++i) {
            const cad::SurfaceLegendEntry& entry = legend.entries[legend.entries.size() - 1 - i];
            rows.push_back({toQColor(entry.colour), QString::fromStdString(entry.label)});
        }
        if (shown < legend.entries.size()) {
            rows.push_back({QColor(), QString("+ %1 more").arg(legend.entries.size() - shown), true});
        }
    }
    const QString title = QString::fromStdString(legend.title);
    const QString high = gradient ? QString::fromStdString(legend.entries.back().label) : QString();
    const QString low = gradient ? QString::fromStdString(legend.entries.front().label) : QString();
    double labelWidth = std::max(metrics.horizontalAdvance(caption), metrics.horizontalAdvance(title));
    for (const Row& row : rows) {
        labelWidth = std::max(labelWidth, swatch + pad + metrics.horizontalAdvance(row.label));
    }
    const double barHeight = 6.0 * line;
    if (gradient) {
        labelWidth = std::max({labelWidth, swatch + pad + metrics.horizontalAdvance(high),
                               swatch + pad + metrics.horizontalAdvance(low)});
    }
    const double bodyHeight = gradient ? barHeight : static_cast<double>(rows.size()) * (swatch + 2.0);
    const double width = labelWidth + 2.0 * pad;
    const double height = 2.0 * line + bodyHeight + 2.0 * pad + 4.0;
    const QRectF panel(topRight.x() - width, topRight.y(), width, height);

    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setFont(font);
    painter.setPen(Qt::NoPen);
    painter.setBrush(backing);
    painter.drawRoundedRect(panel, 4.0, 4.0);
    painter.setPen(text);
    double y = panel.top() + pad;
    painter.drawText(QPointF(panel.left() + pad, y + metrics.ascent()), caption);
    y += line;
    painter.drawText(QPointF(panel.left() + pad, y + metrics.ascent()), title);
    y += line + 4.0;
    if (gradient) {
        const QRectF bar(panel.left() + pad, y, swatch, barHeight);
        QLinearGradient ramp(bar.bottomLeft(), bar.topLeft());
        const std::size_t count = legend.entries.size();
        for (std::size_t i = 0; i < count; ++i) {
            const double t = count > 1 ? static_cast<double>(i) / static_cast<double>(count - 1) : 0.0;
            ramp.setColorAt(t, toQColor(legend.entries[i].colour));
        }
        painter.setPen(Qt::NoPen);
        painter.setBrush(ramp);
        painter.drawRect(bar);
        painter.setPen(text);
        painter.drawText(QPointF(bar.right() + pad, bar.top() + metrics.ascent()), high);
        painter.drawText(QPointF(bar.right() + pad, bar.bottom()), low);
    } else {
        for (const Row& row : rows) {
            if (!row.more) {
                painter.setPen(Qt::NoPen);
                painter.setBrush(row.colour);
                painter.drawRect(QRectF(panel.left() + pad, y, swatch, swatch));
            }
            painter.setPen(text);
            painter.drawText(QPointF(panel.left() + pad + swatch + pad, y + swatch - metrics.descent()),
                             row.label);
            y += swatch + 2.0;
        }
    }
    painter.restore();
    return panel;
}

} // namespace katana::qt
