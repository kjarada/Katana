#include "katana/cad/symbols.hpp"

#include "katana/entity/tables.hpp"

#include <cmath>
#include <numbers>

namespace katana::cad {

namespace {

using katana::geometry::Point2;
using katana::geometry::Polyline2;

constexpr double kPi = std::numbers::pi;
// Enough that a symbol, which is small on any output, reads as round.
constexpr int kCircleChords = 24;

// Strokes in the unit square, before scale and rotation.
struct Unit {
    std::vector<Polyline2> strokes;

    void line(double x0, double y0, double x1, double y1)
    {
        strokes.push_back(Polyline2{{Point2(x0, y0), Point2(x1, y1)}, false});
    }
    void ring(double cx, double cy, double r)
    {
        Polyline2 circle;
        circle.closed = true;
        for (int i = 0; i < kCircleChords; ++i) {
            const double a = 2.0 * kPi * i / kCircleChords;
            circle.vertices.emplace_back(cx + r * std::cos(a), cy + r * std::sin(a));
        }
        strokes.push_back(std::move(circle));
    }
    void polygon(std::initializer_list<Point2> points)
    {
        strokes.push_back(Polyline2{std::vector<Point2>(points), true});
    }
};

Unit unitShape(std::string_view symbol)
{
    Unit u;
    if (symbol == "circle") {
        u.ring(0, 0, 1);
    } else if (symbol == "ring") {
        u.ring(0, 0, 1);
        u.ring(0, 0, 0.5);
    } else if (symbol == "dot") {
        u.ring(0, 0, 0.35);
        u.ring(0, 0, 0.2);
        u.ring(0, 0, 0.05);
    } else if (symbol == "square") {
        u.polygon({Point2(-1, -1), Point2(1, -1), Point2(1, 1), Point2(-1, 1)});
    } else if (symbol == "triangle") {
        u.polygon({Point2(-1, -0.75), Point2(1, -0.75), Point2(0, 1)});
    } else if (symbol == "diamond") {
        u.polygon({Point2(0, -1), Point2(1, 0), Point2(0, 1), Point2(-1, 0)});
    } else if (symbol == "cross") {
        u.line(-1, -1, 1, 1);
        u.line(-1, 1, 1, -1);
    } else if (symbol == "plus") {
        u.line(-1, 0, 1, 0);
        u.line(0, -1, 0, 1);
    } else if (symbol == "tick") {
        u.line(-0.8, 0, -0.2, -0.8);
        u.line(-0.2, -0.8, 1, 0.9);
    } else if (symbol == "star") {
        for (int i = 0; i < 4; ++i) {
            const double a = kPi * i / 4.0;
            u.line(-std::cos(a), -std::sin(a), std::cos(a), std::sin(a));
        }
    } else if (symbol == "tree") {
        // A trunk and a crown, the way a survey plan draws one.
        u.line(0, -1, 0, -0.2);
        u.ring(0, 0.3, 0.7);
    } else if (symbol == "pole") {
        // A circle with a bar through it: the plan symbol of a pole.
        u.ring(0, 0, 0.5);
        u.line(-1, 0, 1, 0);
    } else if (symbol == "manhole") {
        u.ring(0, 0, 1);
        u.line(-0.7, -0.7, 0.7, 0.7);
        u.line(-0.7, 0.7, 0.7, -0.7);
    } else if (symbol == "arrow") {
        u.line(-1, 0, 1, 0);
        u.line(0.4, 0.5, 1, 0);
        u.line(0.4, -0.5, 1, 0);
    } else if (symbol == "flag") {
        u.line(-0.6, -1, -0.6, 1);
        u.polygon({Point2(-0.6, 1), Point2(0.8, 0.6), Point2(-0.6, 0.2)});
    } else if (symbol == "target") {
        // The survey mark: a circle with a cross that reaches past it.
        u.ring(0, 0, 0.6);
        u.line(-1, 0, 1, 0);
        u.line(0, -1, 0, 1);
    }
    return u;
}

} // namespace

std::vector<Polyline2> symbolStrokes(std::string_view symbol, const Point2& centre, double size,
                                     double rotation)
{
    // Exact first, then the shape the name's words suggest. Katana used to
    // store the GUESS on the style and throw the real name away, which meant
    // a loaded 12d symbol library could never be matched against it. The real
    // name is kept now, so the guess belongs here - at the last moment, when
    // nothing better has answered.
    Unit unit = unitShape(symbol);
    if (unit.strokes.empty() && !symbol.empty()) {
        unit = unitShape(katana::entity::builtInSymbolFor(symbol));
    }
    const double c = std::cos(rotation);
    const double s = std::sin(rotation);
    for (Polyline2& stroke : unit.strokes) {
        for (Point2& vertex : stroke.vertices) {
            const double x = vertex.x * size;
            const double y = vertex.y * size;
            vertex = Point2(centre.x + x * c - y * s, centre.y + x * s + y * c);
        }
    }
    return std::move(unit.strokes);
}

} // namespace katana::cad
