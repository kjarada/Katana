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
        return entityPen; // "view_colour" in a library: whatever the entity is
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

namespace {

// The face a style text is painted in at `pixels`: its own family when the
// font database has it, otherwise a sans-serif of the same size.
[[nodiscard]] QFont styleTextFont(QFont font, const katana::cad::StyleTextMark& text, int pixels)
{
    font.setPixelSize(std::max(1, pixels));
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
    return font;
}

// Where the baseline starts, relative to the anchor, in unrotated pixels.
// A library justifies as "vertical-horizontal": "middle-centre", "top-left". A
// spelling this does not know draws from the point, which is what an
// unjustified text already does.
[[nodiscard]] QPointF justifiedOrigin(const QFontMetricsF& metrics, const QString& value,
                                      const std::string& justify)
{
    double dx = 0.0;
    double dy = 0.0;
    if (justify.find("centre") != std::string::npos ||
        justify.find("center") != std::string::npos) {
        dx = -0.5 * metrics.horizontalAdvance(value);
    } else if (justify.find("right") != std::string::npos) {
        dx = -metrics.horizontalAdvance(value);
    }
    if (justify.find("middle") != std::string::npos) {
        dy = 0.5 * metrics.capHeight();
    } else if (justify.find("top") != std::string::npos) {
        dy = metrics.capHeight();
    }
    return QPointF(dx, dy);
}

[[nodiscard]] double widthFactorOf(const katana::cad::StyleTextMark& text)
{
    return std::isfinite(text.widthFactor) && text.widthFactor > 0.0 ? text.widthFactor : 1.0;
}

} // namespace

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
    if (const double factor = widthFactorOf(text); factor != 1.0) {
        painter.scale(factor, 1.0);
    }
    const QFont font = styleTextFont(
        painter.font(), text,
        static_cast<int>(std::lround(std::min(pixels, kMaximumStyleTextPixels))));
    painter.setFont(font);
    const QString value = QString::fromStdString(text.text);
    painter.drawText(justifiedOrigin(QFontMetricsF(font), value, text.justify), value);
    painter.restore();
}

katana::geometry::Box2 styleTextExtent(const katana::cad::StyleTextMark& text, const QFont& base)
{
    katana::geometry::Box2 box;
    if (text.text.empty() || !(text.height > 0.0)) {
        return box;
    }
    // Measured at a reference size and scaled to the text's height: a glyph's
    // proportions do not depend on its size, and this needs no view.
    constexpr int kReferencePixels = 100;
    const QFont font = styleTextFont(base, text, kReferencePixels);
    const QFontMetricsF metrics(font);
    const QString value = QString::fromStdString(text.text);
    const QPointF origin = justifiedOrigin(metrics, value, text.justify);
    // Pixels (y down) to model units (y up) about the anchor, then turned by
    // the text's angle - the same steps paintStyleText takes, in reverse.
    const double unit = text.height / kReferencePixels;
    const double factor = widthFactorOf(text);
    const double cosine = std::cos(text.angle);
    const double sine = std::sin(text.angle);
    const double left = origin.x() * factor;
    const double right = (origin.x() + metrics.horizontalAdvance(value)) * factor;
    const double top = origin.y() - metrics.ascent();
    const double bottom = origin.y() + metrics.descent();
    for (const QPointF corner :
         {QPointF(left, top), QPointF(right, top), QPointF(left, bottom), QPointF(right, bottom)}) {
        const double x = corner.x() * unit;
        const double y = -corner.y() * unit;
        box.expand(katana::geometry::Point2(text.at.x + x * cosine - y * sine,
                                            text.at.y + x * sine + y * cosine));
    }
    return box;
}

katana::geometry::Box2 paintedExtent(const katana::cad::StyleDrawing& drawing, const QFont& base)
{
    katana::geometry::Box2 box;
    for (const auto& stroke : drawing.strokes) {
        for (const auto& vertex : stroke.path.vertices) {
            box.expand(vertex);
        }
    }
    for (const auto& text : drawing.texts) {
        box.expand(styleTextExtent(text, base));
    }
    return box;
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
