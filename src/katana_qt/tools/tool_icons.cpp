#include "tools/tool_icons.hpp"

#include <cmath>

#include <QFont>
#include <QIconEngine>
#include <QPainter>
#include <QPixmap>

#include "theme.hpp"

namespace katana::qt::tools {

ToolInk::ToolInk(QPainter& painter, const QColor& neutral, const QColor& accent)
    : painter_(painter), neutral_(neutral), accent_(accent)
{
}

void ToolInk::stroke(const QPainterPath& path, bool accent, double width) const
{
    painter_.setBrush(Qt::NoBrush);
    painter_.setPen(QPen(tone(accent), width, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter_.drawPath(path);
}

void ToolInk::dashed(const QPainterPath& path, bool accent) const
{
    QPen pen(tone(accent), kStroke, Qt::CustomDashLine, Qt::FlatCap, Qt::RoundJoin);
    pen.setDashPattern({1.6, 1.4});
    painter_.setBrush(Qt::NoBrush);
    painter_.setPen(pen);
    painter_.drawPath(path);
}

void ToolInk::fill(const QPainterPath& path, bool accent, int alpha) const
{
    QColor color = tone(accent);
    color.setAlpha(color.alpha() * alpha / 255);
    painter_.setPen(Qt::NoPen);
    painter_.setBrush(color);
    painter_.drawPath(path);
}

void ToolInk::line(double x1, double y1, double x2, double y2, bool accent, double width) const
{
    QPainterPath path(QPointF(x1, y1));
    path.lineTo(x2, y2);
    stroke(path, accent, width);
}

void ToolInk::dot(double x, double y, double radius, bool accent) const
{
    QPainterPath path;
    path.addEllipse(QPointF(x, y), radius, radius);
    fill(path, accent);
}

void ToolInk::node(double x, double y, bool accent) const
{
    QPainterPath path;
    path.addRect(QRectF(x - 1.5, y - 1.5, 3.0, 3.0));
    fill(path, accent);
}

void ToolInk::label(double x, double y, double size, const QString& text, bool accent) const
{
    painter_.save();
    QFont font = painter_.font();
    font.setPixelSize(100); // drawn large and scaled down: pixel sizes are integers
    font.setBold(true);
    painter_.setFont(font);
    painter_.setPen(tone(accent));
    painter_.translate(x, y);
    painter_.scale(size / 100.0, size / 100.0);
    painter_.drawText(QRectF(-300, -60, 600, 120), Qt::AlignCenter, text);
    painter_.restore();
}

QPainterPath polyline(std::initializer_list<QPointF> points, bool closed)
{
    QPainterPath path;
    bool first = true;
    for (const QPointF& point : points) {
        if (first) {
            path.moveTo(point);
            first = false;
        } else {
            path.lineTo(point);
        }
    }
    if (closed) {
        path.closeSubpath();
    }
    return path;
}

QPainterPath rectangle(double x, double y, double w, double h)
{
    QPainterPath path;
    path.addRect(QRectF(x, y, w, h));
    return path;
}

QPainterPath circle(double x, double y, double radius)
{
    QPainterPath path;
    path.addEllipse(QPointF(x, y), radius, radius);
    return path;
}

QPainterPath arc(double cx, double cy, double radius, double startDegrees, double sweepDegrees)
{
    // QPainterPath::arcTo already measures angles counter-clockwise on screen,
    // which is what "as on paper" means once y points down.
    const QRectF box(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius);
    QPainterPath path;
    path.arcMoveTo(box, startDegrees);
    path.arcTo(box, startDegrees, sweepDegrees);
    return path;
}

namespace {

bool paintTool(std::string_view toolId, const ToolInk& ink)
{
    return paintDrawLineIcon(toolId, ink) || paintDrawCurveIcon(toolId, ink) ||
           paintModifyTransformIcon(toolId, ink) || paintModifyEditIcon(toolId, ink) ||
           paintAnnotateIcon(toolId, ink) || paintInquiryIcon(toolId, ink) ||
           paintDrawDivideIcon(toolId, ink) || paintModifyLengthIcon(toolId, ink) ||
           paintPropertyIcon(toolId, ink) || paintSelectIcon(toolId, ink) ||
           paintDrawingIcon(toolId, ink);
}

void paint(QPainter& painter, std::string_view toolId, const QRectF& rect, const QColor& neutral,
           const QColor& accent)
{
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.translate(rect.topLeft());
    painter.scale(rect.width() / kGrid, rect.height() / kGrid);
    ToolInk ink(painter, neutral, accent);
    if (!paintTool(toolId, ink)) {
        // The initial of the last part of the id, in a frame: visibly a
        // placeholder, never mistaken for a finished icon.
        const std::string_view leaf = toolId.substr(toolId.rfind('.') + 1);
        ink.stroke(rectangle(3, 3, 18, 18), false, 1.0);
        ink.label(12, 12, 12, QString(QChar(leaf.empty() ? '?' : leaf.front())).toUpper());
    }
    painter.restore();
}

class ToolIconEngine final : public QIconEngine {
  public:
    explicit ToolIconEngine(std::string id) : id_(std::move(id)) {}

    void paint(QPainter* painter, const QRect& rect, QIcon::Mode mode, QIcon::State) override
    {
        // As PaintedIconEngine in icons.cpp: disabled is the only mode drawn
        // differently; hover and checked are the button's background.
        const bool disabled = mode == QIcon::Disabled;
        tools::paint(*painter, id_, QRectF(rect), disabled ? theme::textDisabled() : theme::text(),
                     disabled ? theme::textDisabled() : theme::accent());
    }

    QPixmap pixmap(const QSize& size, QIcon::Mode mode, QIcon::State state) override
    {
        QPixmap pixmap(size);
        pixmap.fill(Qt::transparent);
        QPainter painter(&pixmap);
        paint(&painter, QRect(QPoint(0, 0), size), mode, state);
        return pixmap;
    }

    [[nodiscard]] QIconEngine* clone() const override { return new ToolIconEngine(id_); }

  private:
    std::string id_;
};

} // namespace

QIcon toolIcon(std::string_view toolId) { return QIcon(new ToolIconEngine(std::string(toolId))); }

bool hasToolIcon(std::string_view toolId)
{
    QPixmap scratch(24, 24);
    scratch.fill(Qt::transparent);
    QPainter painter(&scratch);
    ToolInk ink(painter, Qt::white, Qt::white);
    return paintTool(toolId, ink);
}

} // namespace katana::qt::tools
