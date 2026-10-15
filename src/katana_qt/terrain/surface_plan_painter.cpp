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
#include <QSizeF>
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

namespace {

// What a legend's panel is made of, laid out once so that the size the plan
// painter measures before it draws (measureSurfaceLegend) and the picture
// paintSurfaceLegend makes are the one calculation.
struct LegendLayout {
    // A swatch legend's rows: an entry each up to the cap, the last saying
    // how many are left out.
    struct Row {
        QColor colour;
        QString label;
        bool more = false;
    };
    std::vector<Row> rows;
    // A gradient: each entry's place along the bar, 0 at the foot to 1 at the
    // head, by its value; and the label written beside it, if it has one.
    struct Stop {
        double at = 0.0;
        QColor colour;
        QString label;
    };
    std::vector<Stop> stops;
    QString title;
    double line = 0.0;
    double pad = 6.0;
    double swatch = 10.0;
    double barHeight = 0.0;
    double width = 0.0;
    double height = 0.0;
};

// Where along the bar each entry of a gradient lies. By value when every entry
// has one and they rise: the built-in Cut Fill style has stops at -2, -1,
// -0.5, 0, 0.5, 1 and 2, which are not evenly spaced, and a colour placed by
// its index reads as the wrong depth against the labelled ends. A ramp's nine
// samples are evenly spaced, so by value is by index for them; entries without
// values fall back to by index.
std::vector<double> gradientPositions(const cad::SurfaceLegend& legend)
{
    const std::size_t count = legend.entries.size();
    std::vector<double> at(count, 0.0);
    if (count < 2) {
        return at;
    }
    const bool valued =
        std::ranges::all_of(legend.entries,
                            [](const cad::SurfaceLegendEntry& entry) {
                                return entry.from.has_value() && std::isfinite(*entry.from);
                            }) &&
        *legend.entries.back().from > *legend.entries.front().from;
    for (std::size_t i = 0; i < count; ++i) {
        at[i] = valued ? (*legend.entries[i].from - *legend.entries.front().from) /
                             (*legend.entries.back().from - *legend.entries.front().from)
                       : static_cast<double>(i) / static_cast<double>(count - 1);
        at[i] = std::clamp(at[i], 0.0, 1.0);
    }
    return at;
}

LegendLayout layoutLegend(const cad::SurfaceLegend& legend, const QString& caption, const QFont& font)
{
    LegendLayout layout;
    const QFontMetricsF metrics(font);
    layout.line = metrics.height();
    layout.swatch = std::max(10.0, layout.line * 0.8);
    layout.title = QString::fromStdString(legend.title);
    double labelWidth =
        std::max(metrics.horizontalAdvance(caption), metrics.horizontalAdvance(layout.title));
    if (legend.gradient) {
        const std::vector<double> at = gradientPositions(legend);
        for (std::size_t i = 0; i < legend.entries.size(); ++i) {
            const cad::SurfaceLegendEntry& entry = legend.entries[i];
            layout.stops.push_back({at[i], toQColor(entry.colour), QString::fromStdString(entry.label)});
            labelWidth = std::max(labelWidth, layout.swatch + layout.pad +
                                                  metrics.horizontalAdvance(layout.stops.back().label));
        }
        layout.barHeight = 6.0 * layout.line;
    } else {
        const std::size_t shown = legend.entries.size() > static_cast<std::size_t>(kLegendRows)
                                      ? static_cast<std::size_t>(kLegendRows) - 1
                                      : legend.entries.size();
        // Highest first, as a map key reads: high ground at the top.
        for (std::size_t i = 0; i < shown; ++i) {
            const cad::SurfaceLegendEntry& entry = legend.entries[legend.entries.size() - 1 - i];
            layout.rows.push_back({toQColor(entry.colour), QString::fromStdString(entry.label)});
        }
        if (shown < legend.entries.size()) {
            layout.rows.push_back(
                {QColor(), QString("+ %1 more").arg(legend.entries.size() - shown), true});
        }
        for (const LegendLayout::Row& row : layout.rows) {
            labelWidth =
                std::max(labelWidth, layout.swatch + layout.pad + metrics.horizontalAdvance(row.label));
        }
    }
    const double bodyHeight = legend.gradient
                                  ? layout.barHeight
                                  : static_cast<double>(layout.rows.size()) * (layout.swatch + 2.0);
    layout.width = labelWidth + 2.0 * layout.pad;
    layout.height = 2.0 * layout.line + bodyHeight + 2.0 * layout.pad + 4.0;
    return layout;
}

} // namespace

QSizeF measureSurfaceLegend(const cad::SurfaceLegend& legend, const QString& caption, const QFont& font)
{
    if (legend.entries.empty()) {
        return {};
    }
    const LegendLayout layout = layoutLegend(legend, caption, font);
    return QSizeF(layout.width, layout.height);
}

QRectF paintSurfaceLegend(QPainter& painter, const cad::SurfaceLegend& legend, const QString& caption,
                          const QPointF& topRight, const QFont& font, const QColor& text,
                          const QColor& backing)
{
    if (legend.entries.empty()) {
        return {};
    }
    const QFontMetricsF metrics(font);
    const LegendLayout layout = layoutLegend(legend, caption, font);
    const double pad = layout.pad;
    const double line = layout.line;
    const double swatch = layout.swatch;
    const QRectF panel(topRight.x() - layout.width, topRight.y(), layout.width, layout.height);

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
    painter.drawText(QPointF(panel.left() + pad, y + metrics.ascent()), layout.title);
    y += line + 4.0;
    if (legend.gradient) {
        const QRectF bar(panel.left() + pad, y, swatch, layout.barHeight);
        QLinearGradient ramp(bar.bottomLeft(), bar.topLeft());
        for (const LegendLayout::Stop& stop : layout.stops) {
            ramp.setColorAt(stop.at, stop.colour);
        }
        painter.setPen(Qt::NoPen);
        painter.setBrush(ramp);
        painter.drawRect(bar);
        painter.setPen(text);
        // The labelled stops beside the bar at their values: the two ends
        // always (the head against the bar's top, the foot against its
        // bottom), an inner one only where it clears the label above it and
        // the foot's by a line.
        const auto centreOf = [&](double at) {
            return std::clamp(bar.bottom() - at * bar.height(), bar.top() + 0.5 * line,
                              bar.bottom() - 0.5 * line);
        };
        const auto write = [&](const LegendLayout::Stop& stop) {
            painter.drawText(QPointF(bar.right() + pad,
                                     centreOf(stop.at) + 0.5 * (metrics.ascent() - metrics.descent())),
                             stop.label);
        };
        std::vector<const LegendLayout::Stop*> labelled;
        for (const LegendLayout::Stop& stop : layout.stops) {
            if (!stop.label.isEmpty()) {
                labelled.push_back(&stop);
            }
        }
        if (!labelled.empty()) {
            // Highest first: the stops rise, so the last is the head.
            std::ranges::reverse(labelled);
            write(*labelled.front());
            if (labelled.size() > 1) {
                const double foot = centreOf(labelled.back()->at);
                double above = centreOf(labelled.front()->at);
                for (std::size_t i = 1; i + 1 < labelled.size(); ++i) {
                    const double here = centreOf(labelled[i]->at);
                    if (here - above >= line && foot - here >= line) {
                        write(*labelled[i]);
                        above = here;
                    }
                }
                write(*labelled.back());
            }
        }
    } else {
        for (const LegendLayout::Row& row : layout.rows) {
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
