#include "katana/cad/style_drawing.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>
#include <utility>

#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StyleUnits;
using katana::geometry::Box2;
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

// The units a definition's coordinates are in, as a multiplier onto model
// units.
[[nodiscard]] double unitScale(StyleUnits units, double paperScale)
{
    return units == StyleUnits::Paper ? paperScale : 1.0;
}

void addArc(std::vector<Point2>& into, const Point2& centre, double radius, double fromDegrees,
            double toDegrees)
{
    const double from = fromDegrees * kPi / 180.0;
    const double to = toDegrees * kPi / 180.0;
    // Libraries write an arc both ways round (`arc 1.75 0 180` and `arc 1.75 180
    // 0`), so the sweep's sign is taken from the numbers rather than assumed.
    const double sweep = to - from;
    const int steps =
        std::max(2, static_cast<int>(std::ceil(std::abs(sweep) / (2.0 * kPi) * kCircleChords)));
    for (int i = 0; i <= steps; ++i) {
        const double angle = from + sweep * i / steps;
        into.emplace_back(centre.x + radius * std::cos(angle), centre.y + radius * std::sin(angle));
    }
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

    // The stretches of the line inside `box`, as distances along it, in order
    // and with touching stretches merged. Each segment is clipped to the box
    // on its own parameter (Liang and Barsky's), which is exact for straight
    // segments and a polyline is nothing else.
    [[nodiscard]] std::vector<std::pair<double, double>> inside(const Box2& box) const
    {
        std::vector<std::pair<double, double>> spans;
        if (box.empty()) {
            return spans;
        }
        for (std::size_t i = 0; i + 1 < points_.size(); ++i) {
            const Point2& a = points_[i];
            const Point2& b = points_[i + 1];
            double enter = 0.0;
            double leave = 1.0;
            const auto clip = [&enter, &leave](double p, double q) {
                if (p == 0.0) {
                    return q >= 0.0; // parallel to this edge: inside it or not at all
                }
                const double t = q / p;
                if (p < 0.0) {
                    enter = std::max(enter, t);
                } else {
                    leave = std::min(leave, t);
                }
                return enter <= leave;
            };
            const double dx = b.x - a.x;
            const double dy = b.y - a.y;
            if (!clip(-dx, a.x - box.min.x) || !clip(dx, box.max.x - a.x) ||
                !clip(-dy, a.y - box.min.y) || !clip(dy, box.max.y - a.y)) {
                continue;
            }
            const double span = starts_[i + 1] - starts_[i];
            const double from = starts_[i] + enter * span;
            const double to = starts_[i] + leave * span;
            if (!spans.empty() && from <= spans.back().second + kShortestPeriod) {
                spans.back().second = std::max(spans.back().second, to);
            } else {
                spans.emplace_back(from, to);
            }
        }
        return spans;
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
void appendRuns(StyleDrawing& into, const FlatDefinition& flat, const auto& map)
{
    for (const FlatDefinition::Run& run : flat.runs) {
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

[[nodiscard]] StyleTextMark textMark(const FlatDefinition::Text& text, const Point2& at,
                                     double scale, double angle)
{
    StyleTextMark mark;
    mark.at = at;
    mark.text = text.text.text;
    mark.height = text.text.height * scale;
    mark.angle = angle + text.text.angle * kPi / 180.0;
    mark.justify = text.text.justify;
    mark.font = text.text.font;
    mark.widthFactor = text.text.widthFactor;
    mark.pen = text.pen;
    return mark;
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

// The definition's strokes, flattened into runs of connected points. A Move
// starts a new run; a Draw extends it; a circle, an arc and a dot are runs of
// their own about the current point.
FlatDefinition flattenDefinition(const LineStyle& style)
{
    FlatDefinition flat;
    flat.units = style.units;
    flat.atVertices = style.atVertices;
    flat.length = style.length;
    flat.bounds = style.bounds();
    const double factor = style.factor;
    const auto place = [&style, factor](const Point2& p) {
        return Point2((p.x - style.origin.x) * factor, (p.y - style.origin.y) * factor);
    };
    flat.anchor1 = place(style.anchor1);
    flat.anchor2 = place(style.anchor2);
    const auto reach = [&flat](const Point2& point) {
        flat.lowestX = flat.anyPoint ? std::min(flat.lowestX, point.x) : point.x;
        flat.highestX = flat.anyPoint ? std::max(flat.highestX, point.x) : point.x;
        flat.anyPoint = true;
    };
    const auto across = [&flat](double y) { flat.reachAcross = std::max(flat.reachAcross, y); };

    Point2 pen{};      // the current point, in the definition's coordinates
    std::string ink{}; // the colour in force
    FlatDefinition::Run* run = nullptr;
    for (const Stroke& stroke : style.strokes) {
        switch (stroke.op) {
        case StrokeOp::Pen:
            // "view_colour" means "whatever the entity is", which is the
            // empty pen here.
            ink = stroke.pen == "view_colour" ? std::string{} : stroke.pen;
            run = nullptr; // a colour change ends the run it interrupts
            break;
        case StrokeOp::Move:
            pen = stroke.point;
            reach(place(pen));
            run = nullptr;
            break;
        case StrokeOp::Draw: {
            if (run == nullptr) {
                flat.runs.push_back(FlatDefinition::Run{{place(pen)}, false, ink});
                run = &flat.runs.back();
                across(std::abs(place(pen).y));
            }
            pen = stroke.point;
            run->points.push_back(place(pen));
            reach(place(pen));
            across(std::abs(place(pen).y));
            break;
        }
        case StrokeOp::Circle: {
            FlatDefinition::Run ring;
            ring.closed = true;
            ring.pen = ink;
            const Point2 centre = place(pen);
            // A LENGTH: see the arc below for what the sign is not.
            const double radius = std::abs(stroke.radius) * factor;
            reach({centre.x - radius, centre.y});
            reach({centre.x + radius, centre.y});
            across(std::abs(centre.y) + std::abs(radius));
            for (int i = 0; i < kCircleChords; ++i) {
                const double angle = 2.0 * kPi * i / kCircleChords;
                ring.points.emplace_back(centre.x + radius * std::cos(angle),
                                         centre.y + radius * std::sin(angle));
            }
            flat.runs.push_back(std::move(ring));
            run = nullptr;
            break;
        }
        case StrokeOp::Arc: {
            FlatDefinition::Run arc;
            arc.pen = ink;
            // The radius is a LENGTH. Libraries write some arcs with a negative
            // one (45 of them, in the U-turn and speed-zone road markings),
            // and the sign is not a side: the angles alone say where the arc
            // runs, and the moves written either side of each such arc
            // land on the ends |r| gives. The signed radius drew every one
            // turned half a turn about its centre.
            const double radius = std::abs(stroke.radius) * factor;
            addArc(arc.points, place(pen), radius, stroke.startAngle, stroke.endAngle);
            across(std::abs(place(pen).y) + std::abs(radius));
            flat.runs.push_back(std::move(arc));
            run = nullptr;
            break;
        }
        case StrokeOp::Dot: {
            // A dot is a mark with no size of its own in the file - every one
            // of the 179 in the two libraries is `dot 0`. It comes out as a
            // one-point run, which is what the renderer already draws a point
            // mark for.
            FlatDefinition::Run dot;
            dot.pen = ink;
            dot.points.push_back(place(pen));
            across(std::abs(place(pen).y));
            flat.runs.push_back(std::move(dot));
            run = nullptr;
            break;
        }
        case StrokeOp::Text:
            if (stroke.text < style.texts.size()) {
                const auto& text = style.texts[stroke.text];
                flat.texts.push_back(FlatDefinition::Text{place(pen), text, ink});
                // A text's extent needs a font and is not known here; its
                // height times its length is a bound no real font exceeds.
                across(std::abs(place(pen).y) +
                       std::abs(text.height * factor) *
                           static_cast<double>(std::max<std::size_t>(1, text.text.size())));
            }
            run = nullptr;
            break;
        }
    }
    // Measured from what was made rather than kept up in each case above, so
    // a mark added to the flattening later is counted without anyone having
    // to remember to.
    const auto drawn = [&flat](double x) {
        flat.drawnLowX = flat.anyDrawn ? std::min(flat.drawnLowX, x) : x;
        flat.drawnHighX = flat.anyDrawn ? std::max(flat.drawnHighX, x) : x;
        flat.anyDrawn = true;
    };
    for (const FlatDefinition::Run& one : flat.runs) {
        for (const Point2& point : one.points) {
            drawn(point.x);
        }
    }
    for (const FlatDefinition::Text& text : flat.texts) {
        drawn(text.at.x);
    }
    return flat;
}

StyleDrawing symbolDrawing(const FlatDefinition& flat, const Point2& at, double size,
                           double rotation, double paperScale)
{
    StyleDrawing drawing;
    double scale = unitScale(flat.units, paperScale);
    if (size > 0.0) {
        // `size` is a WIDTH, as a library's is and as Style::symbolSize is, so the
        // definition is scaled to span it. A definition with no width - a
        // single vertical stroke - keeps its own scale rather than being
        // divided by zero.
        const double width = std::max(flat.bounds.max.x - flat.bounds.min.x,
                                      flat.bounds.max.y - flat.bounds.min.y);
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
    appendRuns(drawing, flat, map);
    for (const FlatDefinition::Text& text : flat.texts) {
        drawing.texts.push_back(textMark(text, map(text.at), scale, rotation));
    }
    return drawing;
}

StyleDrawing symbolDrawing(const LineStyle& style, const Point2& at, double size, double rotation,
                           double paperScale)
{
    return symbolDrawing(flattenDefinition(style), at, size, rotation, paperScale);
}

StyleDrawing twoPointDrawing(const FlatDefinition& flat, const Point2& from, const Point2& to,
                             double paperScale)
{
    StyleDrawing drawing;
    const Point2 a = flat.anchor1;
    const Point2 b = flat.anchor2;
    const Vec2 span = b - a;
    const double spanLength = std::hypot(span.x, span.y);
    const Vec2 target = to - from;
    const double targetLength = std::hypot(target.x, target.y);
    if (spanLength <= kShortestPeriod || targetLength <= kShortestPeriod) {
        // Nothing to map onto: fall back to placing it as a symbol, which at
        // least draws the definition rather than nothing.
        return symbolDrawing(flat, from, 0.0, 0.0, paperScale);
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
    appendRuns(drawing, flat, map);
    for (const FlatDefinition::Text& text : flat.texts) {
        drawing.texts.push_back(textMark(text, map(text.at), scale, turn));
    }
    return drawing;
}

StyleDrawing twoPointDrawing(const LineStyle& style, const Point2& from, const Point2& to,
                             double paperScale)
{
    return twoPointDrawing(flattenDefinition(style), from, to, paperScale);
}

LinestyleLayout layLinestyle(const FlatDefinition& flat, const Polyline2& line,
                             const LinestyleOptions& options)
{
    using Outcome = LinestyleLayout::Outcome;
    LinestyleLayout layout;
    const Path path(line);
    if (!path.usable()) {
        layout.outcome = Outcome::NothingToLay;
        return layout;
    }
    const double scale = unitScale(flat.units, options.paperScale);
    double period = (flat.length > 0.0 ? flat.length : flat.naturalPeriod()) * scale;
    const bool repeats = period > kShortestPeriod;
    if (!repeats) {
        // A definition with no length and no extent along the line - a single
        // vertical tick - is drawn once at the start rather than endlessly.
        period = path.length() + 1.0;
    } else if (options.viewScale > 0.0 &&
               period * options.viewScale < options.minimumPeriodPixels) {
        layout.outcome = Outcome::TooFine;
        return layout;
    }
    // Counted in doubles: a millimetre pattern along a 30 km traverse is
    // thirty million repeats, and the count must be compared with the budget
    // before anything is laid rather than overflow on the way.
    const double last = std::floor(path.length() / period);
    std::vector<std::pair<double, double>> repeatsLaid; // first and last repeat, inclusive
    if (options.visible) {
        // Repeat k covers [k p + low, k p + high] along the line, so it can
        // reach the stretch [a, b] only for k in
        // [ceil((a - high) / p), floor((b - low) / p)].
        //
        // low and high take in the MARKS as well as the pen: an arc reaches a
        // radius past the move to its centre. The margin's extra period does
        // not cover that at the end of the line, where b is the line's end
        // and not the view's, so a pen-only range dropped the last repeat of
        // a scallop that the unclipped laying (the previews) drew.
        const double margin = flat.reachAcross * scale + (repeats ? period : 0.0);
        double low = flat.lowestX;
        double high = flat.highestX;
        if (flat.anyDrawn) {
            low = flat.anyPoint ? std::min(low, flat.drawnLowX) : flat.drawnLowX;
            high = flat.anyPoint ? std::max(high, flat.drawnHighX) : flat.drawnHighX;
        }
        low *= scale;
        high *= scale;
        for (const auto& [a, b] : path.inside(options.visible->inflated(margin))) {
            double first = std::max(0.0, std::ceil((a - high) / period));
            const double lastHere = std::min(last, std::floor((b - low) / period));
            if (!repeatsLaid.empty()) {
                first = std::max(first, repeatsLaid.back().second + 1.0);
            }
            if (first <= lastHere) {
                repeatsLaid.emplace_back(first, lastHere);
            }
        }
    } else {
        repeatsLaid.emplace_back(0.0, last);
    }
    double count = 0.0;
    for (const auto& [first, lastHere] : repeatsLaid) {
        count += lastHere - first + 1.0;
    }
    if (count > static_cast<double>(options.maximumInstances)) {
        layout.outcome = Outcome::OverBudget;
        return layout;
    }

    StyleDrawing& drawing = layout.drawing;
    const auto lay = [&](double start) {
        // Every run clipped to the line: a point past the end is pulled back
        // to it, and a run that lies wholly outside is not drawn at all.
        for (const FlatDefinition::Run& run : flat.runs) {
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
        for (const FlatDefinition::Text& text : flat.texts) {
            const double along = start + text.at.x * scale;
            if (along < 0.0 || along > path.length()) {
                continue; // the whole word would sit off the end of the line
            }
            bool ignored = false;
            // Along the line where it sits, plus whatever the file asks for.
            const Frame frame = path.at(along);
            drawing.texts.push_back(textMark(text, onPath(path, start, text.at, scale, ignored),
                                             scale, std::atan2(frame.along.y, frame.along.x)));
        }
    };
    for (const auto& [first, lastHere] : repeatsLaid) {
        for (double k = first; k <= lastHere; k += 1.0) {
            lay(k * period);
        }
    }
    layout.outcome = Outcome::Laid;
    layout.instances = static_cast<std::size_t>(count);
    return layout;
}

StyleDrawing linestyleDrawing(const LineStyle& style, const Polyline2& line, double paperScale)
{
    LinestyleOptions options;
    options.paperScale = paperScale;
    return layLinestyle(flattenDefinition(style), line, options).drawing;
}

LinestyleLayout styleDrawing(const FlatDefinition& flat, const Polyline2& line,
                             const LinestyleOptions& options)
{
    using Outcome = LinestyleLayout::Outcome;
    LinestyleLayout layout;
    if (line.vertices.empty()) {
        layout.outcome = Outcome::NothingToLay;
        return layout;
    }
    if (flat.atVertices) {
        for (const Point2& vertex : line.vertices) {
            StyleDrawing one = symbolDrawing(flat, vertex, 0.0, 0.0, options.paperScale);
            layout.drawing.strokes.insert(layout.drawing.strokes.end(),
                                          std::make_move_iterator(one.strokes.begin()),
                                          std::make_move_iterator(one.strokes.end()));
            layout.drawing.texts.insert(layout.drawing.texts.end(),
                                        std::make_move_iterator(one.texts.begin()),
                                        std::make_move_iterator(one.texts.end()));
        }
        layout.outcome = Outcome::Laid;
        layout.instances = line.vertices.size();
        return layout;
    }
    if (flat.units == StyleUnits::TwoPoint) {
        layout.drawing = twoPointDrawing(flat, line.vertices.front(), line.vertices.back(),
                                         options.paperScale);
        layout.outcome = Outcome::Laid;
        layout.instances = 1;
        return layout;
    }
    return layLinestyle(flat, line, options);
}

StyleDrawing styleDrawing(const LineStyle& style, const Polyline2& line, double paperScale)
{
    LinestyleOptions options;
    options.paperScale = paperScale;
    return styleDrawing(flattenDefinition(style), line, options).drawing;
}

} // namespace katana::cad
