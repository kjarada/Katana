#include "annotation/annotation_painter.hpp"

#include <algorithm>
#include <cmath>

#include <QFontMetricsF>
#include <QPainter>
#include <QPolygonF>

#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::qt {

using katana::cad::annotation::Drawing;
using katana::cad::annotation::TextFace;
using katana::cad::annotation::TextMeasure;
using katana::geometry::Point2;

const QFont& AnnotationFonts::font(const TextFace& face)
{
    const auto key = std::make_tuple(face.family, face.bold, face.italic);
    auto found = fonts_.find(key);
    if (found == fonts_.end()) {
        QFont made(face.family.empty() ? defaultFamily_ : QString::fromStdString(face.family));
        made.setPixelSize(kReferencePixels);
        made.setBold(face.bold);
        made.setItalic(face.italic);
        found = fonts_.emplace(key, made).first;
    }
    return found->second;
}

TextMeasure AnnotationFonts::measure()
{
    return [this](std::string_view line, const TextFace& face) {
        const QFontMetricsF metrics(font(face));
        return metrics.horizontalAdvance(
                   QString::fromUtf8(line.data(), static_cast<qsizetype>(line.size()))) /
               kReferencePixels;
    };
}

namespace {

QPolygonF polygonOf(const std::vector<Point2>& points, const AnnotationPaintTarget& target)
{
    QPolygonF polygon;
    polygon.reserve(static_cast<qsizetype>(points.size()));
    for (const Point2& point : points) {
        polygon << target.toDevice(point);
    }
    return polygon;
}

} // namespace

void paintAnnotationDrawing(QPainter& painter, const Drawing& drawing,
                            const AnnotationPaintTarget& target, AnnotationFonts& fonts)
{
    if (drawing.empty()) {
        return;
    }
    painter.save();
    QPen pen = target.pen;
    if (drawing.colour && target.colourOf) {
        pen.setColor(target.colourOf(*drawing.colour));
    }
    pen.setDashPattern(QList<qreal>{}); // annotation is never dashed by a linetype
    pen.setStyle(Qt::SolidLine);
    const QColor ink = pen.color();

    if (!drawing.masks.empty()) {
        painter.setPen(Qt::NoPen);
        painter.setBrush(target.background);
        for (const auto& mask : drawing.masks) {
            painter.drawPolygon(polygonOf(mask, target));
        }
    }
    painter.setPen(pen);
    painter.setBrush(Qt::NoBrush);
    for (const auto& stroke : drawing.strokes) {
        painter.drawPolyline(polygonOf(stroke, target));
    }
    for (const auto& outline : drawing.outlines) {
        painter.drawPolygon(polygonOf(outline, target));
    }
    if (!drawing.fills.empty()) {
        painter.setBrush(ink);
        for (const auto& fill : drawing.fills) {
            painter.drawPolygon(polygonOf(fill, target));
        }
        painter.setBrush(Qt::NoBrush);
    }

    for (const auto& run : drawing.texts) {
        const double pixels = run.height * target.pixelsPerUnit;
        const QPointF origin = target.toDevice(run.origin);
        if (pixels < target.minimumTextPixels) {
            // Too small to read: a stroke along the baseline keeps it
            // discoverable, as a plain text's does.
            const double width = katana::entity::estimatedWidth(run.text) * pixels * run.widthFactor;
            painter.drawLine(origin, origin + QPointF(std::cos(run.rotation),
                                                      -std::sin(run.rotation)) *
                                                  width);
            continue;
        }
        painter.save();
        painter.translate(origin);
        painter.rotate(-run.rotation * katana::math::kRadToDeg); // device y points down
        // The slant: a vertical leans right by the oblique angle, which with
        // y down is a negative horizontal shear.
        if (run.oblique != 0.0) {
            painter.shear(-std::tan(run.oblique), 0.0);
        }
        const double scale = pixels / AnnotationFonts::kReferencePixels;
        painter.scale(scale * run.widthFactor, scale);
        painter.setFont(fonts.font(run.face));
        painter.setPen(QPen(ink));
        painter.drawText(QPointF(0.0, 0.0), QString::fromStdString(run.text));
        painter.restore();
    }
    painter.restore();
}

} // namespace katana::qt
