// The Road Objects icons (bim/road_object_icons.hpp). Each is the object as
// it stands beside a road, seen side on: the object neutral, and in the
// accent what makes it that object - the rail of a barrier, the reflector of
// a guide post, a sign's plate, the lantern, the pipe through the headwall,
// the wall's section, the wires, the marks.
//
// Painted with a pen of its own rather than tools::ToolInk, which lives with
// the tool catalogue's icons: this file is also compiled into
// katana_make_icons, the icon contact sheet, which has no catalogue. The
// stroke is the same - 1.7 units of the 24-unit grid, round caps and joins
// (icons.cpp, kStroke).

#include "bim/road_object_icons.hpp"

#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QPointF>
#include <QRectF>

#include <initializer_list>

namespace katana::qt {

namespace {

constexpr double kStroke = 1.7;

class Pen {
  public:
    Pen(QPainter& painter, const QColor& neutral, const QColor& accent)
        : painter_(painter), neutral_(neutral), accent_(accent)
    {
    }

    void stroke(const QPainterPath& path, bool accent = false, double width = kStroke) const
    {
        painter_.setBrush(Qt::NoBrush);
        painter_.setPen(
            QPen(tone(accent), width, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        painter_.drawPath(path);
    }
    void fill(const QPainterPath& path, bool accent = false, int alpha = 255) const
    {
        QColor colour = tone(accent);
        colour.setAlpha(alpha);
        painter_.setPen(Qt::NoPen);
        painter_.setBrush(colour);
        painter_.drawPath(path);
    }
    void line(double x1, double y1, double x2, double y2, bool accent = false,
              double width = kStroke) const
    {
        QPainterPath path(QPointF(x1, y1));
        path.lineTo(x2, y2);
        stroke(path, accent, width);
    }

  private:
    [[nodiscard]] const QColor& tone(bool accent) const { return accent ? accent_ : neutral_; }

    QPainter& painter_;
    QColor neutral_;
    QColor accent_;
};

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

QPainterPath polygon(std::initializer_list<QPointF> points)
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
    path.closeSubpath();
    return path;
}

// The ground every object stands on, thinner than the object so it reads as
// where it stands rather than as part of it.
void ground(const Pen& pen)
{
    pen.line(2.5, 20.5, 21.5, 20.5, false, 1.1);
}

} // namespace

bool paintRoadObjectIcon(Icon which, QPainter& painter, const QColor& neutral,
                         const QColor& accent)
{
    const Pen pen(painter, neutral, accent);
    switch (which) {
    case Icon::RoadBarrier: {
        // Three posts and the beam along their tops.
        ground(pen);
        for (const double x : {5.0, 12.0, 19.0}) {
            pen.line(x, 20.5, x, 9.0);
        }
        const QPainterPath rail = rectangle(3.0, 9.0, 18.0, 4.0);
        pen.fill(rail, true, 110);
        pen.stroke(rail, true);
        return true;
    }
    case Icon::RoadGuidePosts: {
        // Two posts going away, each with its reflector near the top.
        ground(pen);
        pen.stroke(rectangle(5.0, 5.0, 4.0, 15.5));
        pen.fill(rectangle(5.0, 6.5, 4.0, 3.0), true);
        pen.stroke(rectangle(15.0, 10.0, 3.0, 10.5));
        pen.fill(rectangle(15.0, 11.2, 3.0, 2.3), true);
        return true;
    }
    case Icon::RoadSign: {
        // A plate on its post.
        ground(pen);
        pen.line(12.0, 20.5, 12.0, 13.0);
        const QPainterPath plate = rectangle(4.5, 3.5, 15.0, 9.5);
        pen.fill(plate, true, 90);
        pen.stroke(plate, true);
        pen.line(8.0, 8.25, 16.0, 8.25, true, 1.3);
        return true;
    }
    case Icon::RoadLightPole: {
        // The pole, its arm reaching over the road, and the lantern lit.
        ground(pen);
        pen.line(7.0, 20.5, 7.0, 5.0);
        QPainterPath arm(QPointF(7.0, 7.0));
        arm.quadTo(7.5, 4.0, 15.0, 4.0);
        pen.stroke(arm);
        pen.fill(rectangle(14.0, 3.0, 6.5, 2.8), true);
        for (const double x : {15.0, 17.25, 19.5}) {
            pen.line(x, 8.5, x + (x - 17.25) * 0.4, 11.5, true, 1.2);
        }
        return true;
    }
    case Icon::RoadCulvert: {
        // The road over a headwall, the pipe's two walls through it.
        pen.line(2.0, 5.0, 22.0, 5.0);
        pen.stroke(rectangle(4.0, 8.0, 16.0, 12.5));
        pen.stroke(circle(12.0, 14.5, 4.5), true);
        pen.stroke(circle(12.0, 14.5, 2.8), true);
        return true;
    }
    case Icon::RoadWall: {
        // A retaining wall's section, the stem on its footing, and the
        // ground it holds up behind it.
        const QPainterPath section =
            polygon({{7.0, 20.5}, {17.0, 20.5}, {17.0, 17.5}, {13.5, 17.5}, {13.5, 4.0},
                     {10.5, 4.0}, {10.5, 17.5}, {7.0, 17.5}});
        pen.fill(section, true, 110);
        pen.stroke(section, true);
        pen.line(13.5, 7.0, 21.5, 7.0);
        pen.line(2.5, 17.5, 10.5, 17.5);
        return true;
    }
    case Icon::RoadFence: {
        // Posts, and the wires strung along them.
        ground(pen);
        for (const double x : {5.0, 12.0, 19.0}) {
            pen.line(x, 20.5, x, 7.0);
        }
        for (const double y : {8.5, 12.5, 16.5}) {
            pen.line(3.0, y, 21.0, y, true, 1.3);
        }
        return true;
    }
    case Icon::RoadLineMarking: {
        // A road going away, its centre line broken into marks.
        pen.line(3.0, 21.0, 9.5, 3.0);
        pen.line(21.0, 21.0, 14.5, 3.0);
        pen.line(12.0, 20.0, 12.0, 16.0, true, 2.2);
        pen.line(12.0, 12.5, 12.0, 9.5, true, 2.0);
        pen.line(12.0, 6.5, 12.0, 4.5, true, 1.8);
        return true;
    }
    case Icon::RoadObjects: {
        // The manager: a list of objects, each with its mark.
        for (const double y : {6.0, 12.0, 18.0}) {
            pen.fill(rectangle(3.5, y - 1.8, 3.6, 3.6), true);
            pen.line(10.0, y, 20.5, y);
        }
        return true;
    }
    default:
        return false;
    }
}

} // namespace katana::qt
