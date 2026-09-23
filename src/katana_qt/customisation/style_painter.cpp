#include "style_painter.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <mutex>

#include <QFont>
#include <QFontDatabase>
#include <QFontMetricsF>
#include <QPainter>
#include <QPointF>
#include <QPolygonF>
#include <QString>

#include "katana/archive12d/domain.hpp"
#include "katana/math/numerics.hpp"

namespace katana::qt {

namespace {

[[nodiscard]] QPointF toDevice(const katana::cad::ViewTransform& view,
                               const katana::geometry::Point2& world)
{
    const katana::geometry::Point2 p = view.worldToScreen(world);
    return QPointF(p.x, p.y);
}

// Whether the font database has `family`. Asked once per family, since every
// one of the 514 library texts names its font and the answer does not change
// within a session.
[[nodiscard]] bool fontInstalled(const QString& family)
{
    static std::mutex guard;
    static std::map<QString, bool> known;
    const std::lock_guard lock(guard);
    const auto found = known.find(family);
    if (found != known.end()) {
        return found->second;
    }
    const bool installed = QFontDatabase::hasFamily(family);
    known.emplace(family, installed);
    return installed;
}

} // namespace

QPen stylePenFor(const QPen& entityPen, const std::string& pen,
                 const katana::cad::PlotSettings* paper)
{
    if (pen.empty()) {
        return entityPen; // 12d's "view_colour": whatever the entity is
    }
    const auto colour = katana::archive12d::standardColour(pen);
    if (!colour) {
        return entityPen; // a pen name with no RGB here: not a guess
    }
    const katana::entity::Color ink =
        paper != nullptr ? katana::cad::paperColour(*colour, *paper) : *colour;
    QPen changed = entityPen;
    // The entity pen's alpha, not the colour's: a stroke of a locked layer's
    // entity fades with the rest of it.
    changed.setColor(QColor(ink.r, ink.g, ink.b, entityPen.color().alpha()));
    return changed;
}

void paintStyleText(QPainter& painter, const katana::cad::StyleTextMark& text,
                    const StylePaintTarget& target)
{
    if (text.text.empty() || !(text.height > 0.0)) {
        return;
    }
    const double pixels = text.height * target.view.scale;
    if (!(pixels >= kMinimumStyleTextPixels)) {
        return; // smaller than it is worth painting, and unreadable anyway
    }
    painter.save();
    painter.translate(toDevice(target.view, text.at));
    // Screen y grows downwards, so a counter-clockwise model angle turns the
    // other way on the page.
    painter.rotate(-text.angle * katana::math::kRadToDeg);
    // 326 of the 514 texts in the reference libraries are drawn narrowed
    // (0.8 or 0.85). Scaling the painter rather than asking for a stretched
    // font works with every font, stretch variants or not.
    if (std::isfinite(text.widthFactor) && text.widthFactor > 0.0 && text.widthFactor != 1.0) {
        painter.scale(text.widthFactor, 1.0);
    }
    QFont font = painter.font();
    font.setPixelSize(static_cast<int>(std::lround(std::min(pixels, kMaximumStyleTextPixels))));
    if (!text.font.empty()) {
        const QString family = QString::fromStdString(text.font);
        if (fontInstalled(family)) {
            font.setFamily(family);
        } else {
            // Every library text names "Arial", which a Linux build does not
            // have: a sans-serif of the same size, rather than whatever face
            // the font matcher happens to reach first.
            font.setStyleHint(QFont::SansSerif);
        }
    }
    painter.setFont(font);
    const QString value = QString::fromStdString(text.text);
    const QFontMetricsF metrics(font);
    // 12d justifies as "vertical-horizontal": "middle-centre", "top-left".
    // A spelling this does not know draws from the point, which is what an
    // unjustified text already does.
    double dx = 0.0;
    double dy = 0.0;
    if (text.justify.find("centre") != std::string::npos ||
        text.justify.find("center") != std::string::npos) {
        dx = -0.5 * metrics.horizontalAdvance(value);
    } else if (text.justify.find("right") != std::string::npos) {
        dx = -metrics.horizontalAdvance(value);
    }
    if (text.justify.find("middle") != std::string::npos) {
        dy = 0.5 * metrics.capHeight();
    } else if (text.justify.find("top") != std::string::npos) {
        dy = metrics.capHeight();
    }
    painter.drawText(QPointF(dx, dy), value);
    painter.restore();
}

void paintStyleDrawing(QPainter& painter, const katana::cad::StyleDrawing& drawing,
                       const StylePaintTarget& target)
{
    const QPen previous = painter.pen();
    // One definition repeats the same `colour` along a whole line, so the
    // last name's pen is kept rather than looking the name up - which folds
    // its case and allocates - for every stroke.
    std::string lastName;
    QPen lastPen = target.entityPen;
    const auto penFor = [&](const std::string& name) -> const QPen& {
        if (target.entityPenOnly || name.empty()) {
            return target.entityPen;
        }
        if (name != lastName) {
            lastName = name;
            lastPen = stylePenFor(target.entityPen, name, target.paper);
        }
        return lastPen;
    };
    for (const auto& stroke : drawing.strokes) {
        const auto& vertices = stroke.path.vertices;
        if (vertices.empty()) {
            continue;
        }
        if (vertices.size() == 1) {
            QPen dot = penFor(stroke.pen);
            dot.setCapStyle(Qt::RoundCap);
            painter.setPen(dot);
            painter.drawPoint(toDevice(target.view, vertices.front()));
            continue;
        }
        painter.setPen(penFor(stroke.pen));
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(vertices.size()) + 1);
        for (const auto& vertex : vertices) {
            polygon << toDevice(target.view, vertex);
        }
        if (stroke.path.closed) {
            polygon << toDevice(target.view, vertices.front());
        }
        painter.drawPolyline(polygon);
    }
    for (const auto& text : drawing.texts) {
        painter.setPen(penFor(text.pen));
        paintStyleText(painter, text, target);
    }
    painter.setPen(previous);
}

} // namespace katana::qt
