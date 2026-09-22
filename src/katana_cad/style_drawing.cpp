#include "katana/cad/style_drawing.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>

#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::StyleUnits;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Vec2;

constexpr double kPi = std::numbers::pi;
// The same count cad::symbolStrokes uses, and for the same reason: a symbol
// is small on any output and the eye cannot tell 24 chords from a circle at
// that size.
constexpr int kCircleChords = 24;
// A pattern shorter than this would repeat without end along any real line.
// It is the geometric tolerance, which is what "two points in the same place"
// already means everywhere else.
constexpr double kShortestPeriod = katana::math::tolerance::kGeometric;
// A line cannot carry more instances of a pattern than this. A definition
// with a 1 mm period laid along a 30 km survey traverse would otherwise ask
// for thirty million instances and take the application with it; the drawing
// is truncated and stays a drawing.
constexpr std::size_t kMaxInstances = 20000;

// The units a definition's coordinates are in, as a multiplier onto model
// units.
[[nodiscard]] double unitScale(const LineStyle& style, double paperScale)
{
    return style.units == StyleUnits::Paper ? paperScale : 1.0;
}

// Where the strokes are laid out before they are placed: the definition's own
// coordinates, with `factor` and the origin applied, ready to be scaled.
struct Local {
    // A run of connected points, and the pen in force when it was drawn.
    struct Run {
        std::vector<Point2> points;
        bool closed = false;
        std::string pen;
    };
    std::vector<Run> runs;
    struct Text {
        Point2 at;
        const StrokeText* text = nullptr;
        std::string pen;
    };
    std::vector<Text> texts;

    // How far the PEN travelled along the definition's x, moves included.
    //
    // This is the period of the pattern when the file gives no `length`, and
    // it is not the same as the span of what was drawn: a linestyle ends its
    // period with a bare `move`, and that move IS the gap.
    // "move 0 0 / draw 3 0 / move 5 0" is a three-unit dash and a two-unit
    // gap. Measuring only the drawn part gave a period of 3, so every dash
    // butted against the next and the whole linestyle came out as a solid
    // line - which is exactly what it looked like.
    double lowestX = 0.0;
    double highestX = 0.0;
    bool anyPoint = false;

    void reach(const Point2& point)
    {
        lowestX = anyPoint ? std::min(lowestX, point.x) : point.x;
        highestX = anyPoint ? std::max(highestX, point.x) : point.x;
        anyPoint = true;
    }
};

void addArc(std::vector<Point2>& into, const Point2& centre, double radius, double fromDegrees,
            double toDegrees)
{
    const double from = fromDegrees * kPi / 180.0;
    const double to = toDegrees * kPi / 180.0;
    // 12d writes an arc both ways round (`arc 1.75 0 180` and `arc 1.75 180
    // 0`), so the sweep's sign is taken from the numbers rather than assumed.
    const double sweep = to - from;
    const int steps =
        std::max(2, static_cast<int>(std::ceil(std::abs(sweep) / (2.0 * kPi) * kCircleChords)));
    for (int i = 0; i <= steps; ++i) {
        const double angle = from + sweep * i / steps;
        into.emplace_back(centre.x + radius * std::cos(angle), centre.y + radius * std::sin(angle));
    }
}

// The definition's strokes, flattened into runs of connected points. A Move
// starts a new run; a Draw extends it; a circle, an arc and a dot are runs of
// their own about the current point.
[[nodiscard]] Local flatten(const LineStyle& style)
{
    Local local;
    const double factor = style.factor;
    const auto place = [&style, factor](const Point2& p) {
        return Point2((p.x - style.origin.x) * factor, (p.y - style.origin.y) * factor);
    };

    Point2 pen{};      // the current point, in the definition's coordinates
    std::string ink{}; // the colour in force
    Local::Run* run = nullptr;
    for (const Stroke& stroke : style.strokes) {
        switch (stroke.op) {
        case StrokeOp::Pen:
            // "view_colour" is 12d for "whatever the entity is", which is the
            // empty pen here.
            ink = stroke.pen == "view_colour" ? std::string{} : stroke.pen;
            run = nullptr; // a colour change ends the run it interrupts
            break;
        case StrokeOp::Move:
            pen = stroke.point;
            local.reach(place(pen));
            run = nullptr;
            break;
        case StrokeOp::Draw: {
            if (run == nullptr) {
                local.runs.push_back(Local::Run{{place(pen)}, false, ink});
                run = &local.runs.back();
            }
            pen = stroke.point;
            run->points.push_back(place(pen));
            local.reach(place(pen));
            break;
        }
        case StrokeOp::Circle: {
            Local::Run ring;
            ring.closed = true;
            ring.pen = ink;
            const Point2 centre = place(pen);
            local.reach({centre.x - stroke.radius * factor, centre.y});
            local.reach({centre.x + stroke.radius * factor, centre.y});
            for (int i = 0; i < kCircleChords; ++i) {
                const double angle = 2.0 * kPi * i / kCircleChords;
                ring.points.emplace_back(centre.x + stroke.radius * factor * std::cos(angle),
                                         centre.y + stroke.radius * factor * std::sin(angle));
            }
            local.runs.push_back(std::move(ring));
            run = nullptr;
            break;
        }
        case StrokeOp::Arc: {
            Local::Run arc;
            arc.pen = ink;
            addArc(arc.points, place(pen), stroke.radius * factor, stroke.startAngle,
                   stroke.endAngle);
            local.runs.push_back(std::move(arc));
            run = nullptr;
            break;
        }
        case StrokeOp::Dot: {
            // A dot is a mark with no size of its own in the file - every one
            // of the 179 in the two libraries is `dot 0`. It comes out as a
            // one-point run, which is what the renderer already draws a point
            // mark for.
            Local::Run dot;
            dot.pen = ink;
            dot.points.push_back(place(pen));
            local.runs.push_back(std::move(dot));
            run = nullptr;
            break;
        }
        case StrokeOp::Text:
            if (stroke.text < style.texts.size()) {
                local.texts.push_back(Local::Text{place(pen), &style.texts[stroke.text], ink});
            }
            run = nullptr;
            break;
        }
    }
    return local;
}

// The span of the definition along its own +x, which is the period of the
// pattern when the file does not give a `length`.
[[nodiscard]] double naturalPeriod(const Local& local)
{
    return local.anyPoint ? local.highestX - local.lowestX : 0.0;
}

// Where a distance along a polyline lands, and which way the line is going
// there. A distance outside the line is CLAMPED to its end, and the caller is
// told, so that a stroke reaching past the end stops at the end and one
// entirely beyond it is dropped. Letting it run on drew a 164 m dash for a
// 30 m fence and scribbled it across the drawing.
struct Frame {
    Point2 at{};
    Vec2 along{1.0, 0.0};
};

class Path {
  public:
    explicit Path(const Polyline2& line)
    {
        const auto& vertices = line.vertices;
        if (vertices.empty()) {
            return;
        }
        points_.push_back(vertices.front());
        starts_.push_back(0.0);
        for (std::size_t i = 1; i < vertices.size(); ++i) {
            const double step = vertices[i - 1].distanceTo(vertices[i]);
            if (step <= 0.0) {
                continue; // a repeated vertex has no direction to offer
            }
            length_ += step;
            points_.push_back(vertices[i]);
            starts_.push_back(length_);
        }
        if (line.closed && points_.size() > 2) {
            const double step = points_.back().distanceTo(points_.front());
            if (step > 0.0) {
                length_ += step;
                points_.push_back(points_.front());
                starts_.push_back(length_);
            }
        }
    }

    [[nodiscard]] bool usable() const { return points_.size() >= 2 && length_ > 0.0; }
    [[nodiscard]] double length() const { return length_; }

    [[nodiscard]] Frame at(double distance) const
    {
        distance = std::clamp(distance, 0.0, length_);
        // The segment whose span contains the distance; the first or last
        // where it falls outside, so an overhang keeps going straight.
        const auto found = std::upper_bound(starts_.begin(), starts_.end(), distance);
        std::size_t segment = found == starts_.begin()
                                  ? 0
                                  : static_cast<std::size_t>(found - starts_.begin()) - 1;
        segment = std::min(segment, points_.size() - 2);
        const Point2& from = points_[segment];
        const Point2& to = points_[segment + 1];
        const double span = starts_[segment + 1] - starts_[segment];
        const Vec2 direction = span > 0.0 ? (to - from) / span : Vec2{1.0, 0.0};
        const double into = distance - starts_[segment];
        return Frame{Point2(from.x + direction.x * into, from.y + direction.y * into), direction};
    }

  private:
    std::vector<Point2> points_{};
    std::vector<double> starts_{};
    double length_ = 0.0;
};

// One local point put on the line: `start` along, then the local x further
// along and the local y to the left of wherever that lands. `inside` says
// whether that distance was on the line at all.
[[nodiscard]] Point2 onPath(const Path& path, double start, const Point2& local, double scale,
                            bool& inside)
{
    const double along = start + local.x * scale;
    inside = along >= -kShortestPeriod && along <= path.length() + kShortestPeriod;
    const Frame frame = path.at(std::clamp(along, 0.0, path.length()));
    const double offset = local.y * scale;
    return Point2(frame.at.x - frame.along.y * offset, frame.at.y + frame.along.x * offset);
}

// For the placements that put a definition down whole - a symbol, a two-point
// style - where there is no line to clip against.
void appendRuns(StyleDrawing& into, const Local& local, const auto& map)
{
    for (const Local::Run& run : local.runs) {
        StyleStroke stroke;
        stroke.pen = run.pen;
        stroke.path.closed = run.closed;
        stroke.path.vertices.reserve(run.points.size());
        for (const Point2& point : run.points) {
            stroke.path.vertices.push_back(map(point));
        }
        into.strokes.push_back(std::move(stroke));
    }
}

} // namespace

katana::geometry::Box2 StyleDrawing::bounds() const
{
    katana::geometry::Box2 box;
    for (const StyleStroke& stroke : strokes) {
        for (const Point2& point : stroke.path.vertices) {
            box.expand(point);
        }
    }
    for (const StyleTextMark& text : texts) {
        box.expand(text.at);
    }
    return box;
}

StyleDrawing symbolDrawing(const LineStyle& style, const Point2& at, double size, double rotation,
                           double paperScale)
{
    const Local local = flatten(style);
    StyleDrawing drawing;

    double scale = unitScale(style, paperScale);
    if (size > 0.0) {
        // `size` is a WIDTH, as 12d's is and as Style::symbolSize is, so the
        // definition is scaled to span it. A definition with no width - a
        // single vertical stroke - keeps its own scale rather than being
        // divided by zero.
        const katana::geometry::Box2 box = style.bounds();
        const double width = std::max(box.max.x - box.min.x, box.max.y - box.min.y);
        if (width > kShortestPeriod) {
            scale = size / width;
        }
    }
    const double cosine = std::cos(rotation);
    const double sine = std::sin(rotation);
    const auto map = [&](const Point2& point) {
        const double x = point.x * scale;
        const double y = point.y * scale;
        return Point2(at.x + x * cosine - y * sine, at.y + x * sine + y * cosine);
    };
    appendRuns(drawing, local, map);
    for (const Local::Text& text : local.texts) {
        StyleTextMark mark;
        mark.at = map(text.at);
        mark.text = text.text->text;
        mark.height = text.text->height * scale;
        mark.angle = rotation + text.text->angle * kPi / 180.0;
        mark.justify = text.text->justify;
        mark.font = text.text->font;
        mark.widthFactor = text.text->widthFactor;
        mark.pen = text.pen;
        drawing.texts.push_back(std::move(mark));
    }
    return drawing;
}

StyleDrawing twoPointDrawing(const LineStyle& style, const Point2& from, const Point2& to,
                             double paperScale)
{
    const Local local = flatten(style);
    StyleDrawing drawing;
    // The anchors in the same frame the strokes were flattened into.
    const auto anchor = [&style](const Point2& p) {
        return Point2((p.x - style.origin.x) * style.factor,
                      (p.y - style.origin.y) * style.factor);
    };
    const Point2 a = anchor(style.anchor1);
    const Point2 b = anchor(style.anchor2);
    const Vec2 span = b - a;
    const double spanLength = std::hypot(span.x, span.y);
    const Vec2 target = to - from;
    const double targetLength = std::hypot(target.x, target.y);
    if (spanLength <= kShortestPeriod || targetLength <= kShortestPeriod) {
        // Nothing to map onto: fall back to placing it as a symbol, which at
        // least draws the definition rather than nothing.
        return symbolDrawing(style, from, 0.0, 0.0, paperScale);
    }
    // The similarity that carries a -> from and b -> to. A similarity rather
    // than an independent scale per axis, so the definition is not sheared: a
    // stretched doorway is still a doorway, a sheared one is a mistake.
    const double scale = targetLength / spanLength;
    const double turn = std::atan2(target.y, target.x) - std::atan2(span.y, span.x);
    const double cosine = std::cos(turn) * scale;
    const double sine = std::sin(turn) * scale;
    const auto map = [&](const Point2& point) {
        const double x = point.x - a.x;
        const double y = point.y - a.y;
        return Point2(from.x + x * cosine - y * sine, from.y + x * sine + y * cosine);
    };
    appendRuns(drawing, local, map);
    for (const Local::Text& text : local.texts) {
        StyleTextMark mark;
        mark.at = map(text.at);
        mark.text = text.text->text;
        mark.height = text.text->height * scale;
        mark.angle = turn + text.text->angle * kPi / 180.0;
        mark.justify = text.text->justify;
        mark.font = text.text->font;
        mark.widthFactor = text.text->widthFactor;
        mark.pen = text.pen;
        drawing.texts.push_back(std::move(mark));
    }
    return drawing;
}

StyleDrawing linestyleDrawing(const LineStyle& style, const Polyline2& line, double paperScale)
{
    StyleDrawing drawing;
    const Path path(line);
    if (!path.usable()) {
        return drawing;
    }
    const Local local = flatten(style);
    const double scale = unitScale(style, paperScale);
    double period = (style.length > 0.0 ? style.length : naturalPeriod(local)) * scale;
    if (period <= kShortestPeriod) {
        // A definition with no length and no extent along the line - a single
        // vertical tick - is drawn once at the start rather than endlessly.
        period = path.length() + 1.0;
    }
    const std::size_t instances = std::min(
        kMaxInstances, static_cast<std::size_t>(std::floor(path.length() / period)) + 1);
    for (std::size_t i = 0; i < instances; ++i) {
        const double start = static_cast<double>(i) * period;
        // Every run clipped to the line: a point past the end is pulled back
        // to it, and a run that lies wholly outside is not drawn at all.
        for (const Local::Run& run : local.runs) {
            StyleStroke stroke;
            stroke.pen = run.pen;
            stroke.path.closed = run.closed;
            stroke.path.vertices.reserve(run.points.size());
            bool anyInside = false;
            for (const Point2& point : run.points) {
                bool inside = false;
                stroke.path.vertices.push_back(onPath(path, start, point, scale, inside));
                anyInside = anyInside || inside;
            }
            if (anyInside) {
                drawing.strokes.push_back(std::move(stroke));
            }
        }
        const auto map = [&](const Point2& point) {
            bool ignored = false;
            return onPath(path, start, point, scale, ignored);
        };
        for (const Local::Text& text : local.texts) {
            const double along = start + text.at.x * scale;
            if (along < 0.0 || along > path.length()) {
                continue; // the whole word would sit off the end of the line
            }
            StyleTextMark mark;
            mark.at = map(text.at);
            mark.text = text.text->text;
            mark.height = text.text->height * scale;
            // Along the line where it sits, plus whatever the file asks for.
            const Frame frame = path.at(start + text.at.x * scale);
            mark.angle =
                std::atan2(frame.along.y, frame.along.x) + text.text->angle * kPi / 180.0;
            mark.justify = text.text->justify;
            mark.font = text.text->font;
            mark.widthFactor = text.text->widthFactor;
            mark.pen = text.pen;
            drawing.texts.push_back(std::move(mark));
        }
    }
    return drawing;
}

StyleDrawing styleDrawing(const LineStyle& style, const Polyline2& line, double paperScale)
{
    if (line.vertices.empty()) {
        return {};
    }
    if (style.atVertices) {
        StyleDrawing drawing;
        for (const Point2& vertex : line.vertices) {
            StyleDrawing one = symbolDrawing(style, vertex, 0.0, 0.0, paperScale);
            drawing.strokes.insert(drawing.strokes.end(),
                                   std::make_move_iterator(one.strokes.begin()),
                                   std::make_move_iterator(one.strokes.end()));
            drawing.texts.insert(drawing.texts.end(), std::make_move_iterator(one.texts.begin()),
                                 std::make_move_iterator(one.texts.end()));
        }
        return drawing;
    }
    if (style.units == StyleUnits::TwoPoint) {
        return twoPointDrawing(style, line.vertices.front(), line.vertices.back(), paperScale);
    }
    return linestyleDrawing(style, line, paperScale);
}

} // namespace katana::cad
