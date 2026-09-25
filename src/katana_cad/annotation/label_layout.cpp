#include "katana/cad/annotation/label_layout.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <map>
#include <optional>
#include <string>
#include <utility>

#include "katana/cad/annotation/leader_draw.hpp"
#include "katana/cad/annotation/text_layout.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

using katana::entity::Entity;
using katana::entity::LabelGeometry;
using katana::entity::LabelKind;
using katana::entity::LabelPiece;
using katana::entity::LabelPlacement;
using katana::entity::LabelStyle;
using katana::entity::TextJustify;
using katana::geometry::Segment2;
namespace tol = katana::math::tolerance;

namespace {

// Rings of displaced positions beyond the first: at the style's offset plus
// this many text heights. Two rings, because a third is far enough from its
// feature that a reader no longer connects them even with a leader.
constexpr double kDisplacedRings[] = {2.0, 4.0};
// Where along a segment or an arc a label may slide, as a fraction of its
// length, in order of preference: the middle, then outwards.
constexpr double kSlidePositions[] = {0.5, 0.35, 0.65, 0.2, 0.8};
// The grid cell of the overlap search, in millimetres on paper: about four
// lines of 2.5 mm text, so a label's box meets a handful of cells.
constexpr double kGridCellMillimetres = 10.0;
// How far past the view a label's anchor may be and still have its text
// reach into it, in millimetres on paper.
constexpr double kReachMillimetres = 30.0;

struct Candidate {
    Point2 at{};
    double rotation = 0.0;
    TextJustify justify = TextJustify::BottomLeft;
    bool displaced = false;
    // Set ON the line with a mask behind it, so the line itself is no
    // obstacle.
    bool onLine = false;
};

// A piece waiting to be placed, with what orders it.
struct Pending {
    const Entity* entity = nullptr;
    const LabelGeometry* label = nullptr;
    const LabelStyle* style = nullptr;
    std::size_t pieceIndex = 0;
    LabelPiece piece;
    std::string text;
    bool dragged = false;
};

Vec2 unit(double angle)
{
    return Vec2(std::cos(angle), std::sin(angle));
}

// The direction a text along `direction` reads in: turned half a turn when
// it would read upside down (text_layout.hpp's rule).
double readingAngle(double direction)
{
    return readableRotation(direction, TextJustify::BottomLeft).rotation;
}

// ---- geometry of the overlap test -----------------------------------------------------

bool separated(const std::vector<Point2>& a, const std::vector<Point2>& b)
{
    const std::size_t n = a.size();
    for (std::size_t i = 0; i < n; ++i) {
        const Vec2 edge = a[(i + 1) % n] - a[i];
        const Vec2 axis = edge.perpendicular();
        if (!(axis.length() > 0.0)) {
            continue;
        }
        double minA = std::numeric_limits<double>::infinity();
        double maxA = -minA;
        for (const Point2& p : a) {
            const double d = axis.dot(p - a[0]);
            minA = std::min(minA, d);
            maxA = std::max(maxA, d);
        }
        double minB = std::numeric_limits<double>::infinity();
        double maxB = -minB;
        for (const Point2& p : b) {
            const double d = axis.dot(p - a[0]);
            minB = std::min(minB, d);
            maxB = std::max(maxB, d);
        }
        if (maxA <= minB || maxB <= minA) {
            return true;
        }
    }
    return false;
}

bool pointInConvex(const std::vector<Point2>& polygon, const Point2& p)
{
    bool positive = false;
    bool negative = false;
    const std::size_t n = polygon.size();
    for (std::size_t i = 0; i < n; ++i) {
        const double c = (polygon[(i + 1) % n] - polygon[i]).cross(p - polygon[i]);
        positive = positive || c > 0.0;
        negative = negative || c < 0.0;
    }
    return !(positive && negative);
}

bool segmentsCross(const Point2& a, const Point2& b, const Point2& c, const Point2& d)
{
    const double d1 = (b - a).cross(c - a);
    const double d2 = (b - a).cross(d - a);
    const double d3 = (d - c).cross(a - c);
    const double d4 = (d - c).cross(b - c);
    return ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) &&
           ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0));
}

bool segmentMeetsConvex(const std::vector<Point2>& polygon, const Segment2& segment)
{
    if (pointInConvex(polygon, segment.start) || pointInConvex(polygon, segment.end)) {
        return true;
    }
    const std::size_t n = polygon.size();
    for (std::size_t i = 0; i < n; ++i) {
        if (segmentsCross(polygon[i], polygon[(i + 1) % n], segment.start, segment.end)) {
            return true;
        }
    }
    return false;
}

Box2 boxOf(const std::vector<Point2>& points)
{
    Box2 box;
    for (const Point2& p : points) {
        box.expand(p);
    }
    return box;
}

// The point of a polygon's boundary nearest `p`: where a leader meets a box.
Point2 nearestOnBoundary(const std::vector<Point2>& polygon, const Point2& p)
{
    Point2 best = polygon.front();
    double bestDistance = std::numeric_limits<double>::infinity();
    const std::size_t n = polygon.size();
    for (std::size_t i = 0; i < n; ++i) {
        const Segment2 edge{polygon[i], polygon[(i + 1) % n]};
        const Point2 q = edge.closestPoint(p);
        const double d = q.distanceTo(p);
        if (d < bestDistance) {
            bestDistance = d;
            best = q;
        }
    }
    return best;
}

// A uniform grid over boxes, for the overlap search. Keyed by cell in an
// ordered map: the lookups ask only "is anything here", so no answer depends
// on the order, and an ordered map keeps it so.
class Grid {
  public:
    explicit Grid(double cell) : cell_(cell > 0.0 ? cell : 1.0) {}

    template <typename Visit> bool anyNear(const Box2& box, Visit&& visit) const
    {
        const auto [x0, y0, x1, y1] = cells(box);
        for (std::int64_t x = x0; x <= x1; ++x) {
            for (std::int64_t y = y0; y <= y1; ++y) {
                const auto found = cells_.find({x, y});
                if (found == cells_.end()) {
                    continue;
                }
                for (const std::size_t index : found->second) {
                    if (visit(index)) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    void insert(const Box2& box, std::size_t index)
    {
        const auto [x0, y0, x1, y1] = cells(box);
        // A box too big for the grid (a label the size of the drawing) is
        // put on every cell it covers up to a limit, and past it on a list
        // every query sees.
        if ((x1 - x0 + 1) * (y1 - y0 + 1) > 4096) {
            oversized_.push_back(index);
            return;
        }
        for (std::int64_t x = x0; x <= x1; ++x) {
            for (std::int64_t y = y0; y <= y1; ++y) {
                cells_[{x, y}].push_back(index);
            }
        }
    }

    [[nodiscard]] const std::vector<std::size_t>& oversized() const { return oversized_; }

  private:
    struct Range {
        std::int64_t x0, y0, x1, y1;
    };
    [[nodiscard]] Range cells(const Box2& box) const
    {
        const auto at = [&](double v) {
            return static_cast<std::int64_t>(std::floor(v / cell_));
        };
        return {at(box.min.x), at(box.min.y), at(box.max.x), at(box.max.y)};
    }

    double cell_;
    std::map<std::pair<std::int64_t, std::int64_t>, std::vector<std::size_t>> cells_;
    std::vector<std::size_t> oversized_;
};

// ---- candidates ----------------------------------------------------------------------

void pointCandidates(const LabelPiece& piece, const LabelStyle& style, double gap, double height,
                     double horizontal, std::vector<Candidate>& out)
{
    struct Position {
        LabelPlacement where;
        double dx, dy;
        TextJustify justify;
    };
    // Imhof's order for a point: upper right, lower right, upper left, lower
    // left, then right, above, left, below - the diagonals keep the text
    // clear of lines leaving the point along the axes.
    constexpr double d = 0.70710678118654752440; // the diagonal, so every position is `gap` away
    static constexpr Position kPositions[] = {
        {LabelPlacement::Auto, d, d, TextJustify::BottomLeft},
        {LabelPlacement::Auto, d, -d, TextJustify::TopLeft},
        {LabelPlacement::Auto, -d, d, TextJustify::BottomRight},
        {LabelPlacement::Auto, -d, -d, TextJustify::TopRight},
        {LabelPlacement::Right, 1.0, 0.0, TextJustify::MiddleLeft},
        {LabelPlacement::Above, 0.0, 1.0, TextJustify::BottomCentre},
        {LabelPlacement::Left, -1.0, 0.0, TextJustify::MiddleRight},
        {LabelPlacement::Below, 0.0, -1.0, TextJustify::TopCentre},
    };
    std::vector<Position> ordered(std::begin(kPositions), std::end(kPositions));
    if (style.placement == LabelPlacement::Right || style.placement == LabelPlacement::Above ||
        style.placement == LabelPlacement::Left || style.placement == LabelPlacement::Below) {
        std::stable_partition(ordered.begin(), ordered.end(),
                              [&](const Position& p) { return p.where == style.placement; });
    }
    const Vec2 x = unit(horizontal);
    const Vec2 y = x.perpendicular();
    if (style.placement == LabelPlacement::Centroid || style.placement == LabelPlacement::Along) {
        out.push_back(Candidate{piece.anchor, horizontal, TextJustify::MiddleCentre, false, true});
    }
    const std::size_t count = style.displace ? ordered.size() : 1;
    for (std::size_t i = 0; i < count; ++i) {
        const Position& p = ordered[i];
        out.push_back(Candidate{piece.anchor + (x * p.dx + y * p.dy) * gap, horizontal, p.justify,
                                false, false});
    }
    if (!style.displace) {
        return;
    }
    for (const double ring : kDisplacedRings) {
        for (const Position& p : ordered) {
            out.push_back(Candidate{piece.anchor + (x * p.dx + y * p.dy) * (gap + ring * height),
                                    horizontal, p.justify, true, false});
        }
    }
}

// Along a line: the point at `t` and the direction there.
struct Station {
    Point2 at;
    double direction;
    // Towards the outside of an arc; the line's left for a segment.
    Vec2 outside;
};

Station stationOn(const LabelPiece& piece, double t)
{
    if (piece.shape == LabelPiece::Shape::Arc) {
        const Point2 at = piece.arc.pointAt(t);
        const Vec2 radial = (at - piece.arc.center).normalized();
        const Vec2 tangent = piece.arc.sweep >= 0.0 ? radial.perpendicular() : -radial.perpendicular();
        return Station{at, tangent.angle(), radial};
    }
    const Vec2 along = piece.to - piece.from;
    return Station{piece.from + along * t, along.angle(), along.normalized().perpendicular()};
}

void lineCandidates(const LabelPiece& piece, const LabelStyle& style, double gap, double height,
                    double horizontal, std::vector<Candidate>& out)
{
    const bool aligned = style.orientation == katana::entity::LabelOrientation::Aligned;
    // Above is the outside of an arc and the left of a segment seen along
    // it: for a boundary drawn anticlockwise, outside the lot.
    const bool below = style.placement == LabelPlacement::Below;
    const bool along = style.placement == LabelPlacement::Along ||
                       style.placement == LabelPlacement::Centroid;
    const auto candidateAt = [&](double t, bool outsideSide, double distance, bool displaced) {
        const Station s = stationOn(piece, t);
        const double rotation = aligned ? readingAngle(s.direction) : horizontal;
        const Vec2 up = unit(rotation).perpendicular();
        const Vec2 side = outsideSide ? s.outside : -s.outside;
        // The text's own up or down, whichever faces away from the line, so
        // the gap is measured from its nearer edge.
        const bool textAbove = side.dot(up) >= 0.0;
        const TextJustify justify = aligned ? (textAbove ? TextJustify::BottomCentre
                                                         : TextJustify::TopCentre)
                                            : (textAbove ? TextJustify::BottomCentre
                                                         : TextJustify::TopCentre);
        return Candidate{s.at + side * distance, rotation, justify, displaced, false};
    };
    if (along) {
        for (const double t : kSlidePositions) {
            const Station s = stationOn(piece, t);
            out.push_back(Candidate{s.at, aligned ? readingAngle(s.direction) : horizontal,
                                    TextJustify::MiddleCentre, false, true});
            if (!style.displace) {
                return;
            }
        }
    }
    const bool preferred = !below;
    for (const double t : kSlidePositions) {
        out.push_back(candidateAt(t, preferred, gap, false));
        if (!style.displace) {
            return;
        }
        out.push_back(candidateAt(t, !preferred, gap, false));
    }
    for (const double ring : kDisplacedRings) {
        out.push_back(candidateAt(0.5, preferred, gap + ring * height, true));
        out.push_back(candidateAt(0.5, !preferred, gap + ring * height, true));
    }
}

void areaCandidates(const LabelPiece& piece, const LabelStyle& style, double height,
                    double horizontal, std::vector<Candidate>& out)
{
    const Vec2 x = unit(horizontal);
    const Vec2 y = x.perpendicular();
    if (!style.displace) {
        // Where the style puts it, over a mask if a line runs through.
        out.push_back(Candidate{piece.anchor, horizontal, TextJustify::MiddleCentre, false, true});
        return;
    }
    // Clear of every line first: the interior point, then stepped a line up
    // and down and sideways - still inside most lots. Only when none of those
    // is clear does the interior point take the text over a mask, which hides
    // what it covers: a pit or a kerb inside the lot drawn out of sight
    // before its time.
    out.push_back(Candidate{piece.anchor, horizontal, TextJustify::MiddleCentre, false, false});
    for (const auto& [dx, dy] : {std::pair{0.0, 1.5}, std::pair{0.0, -1.5}, std::pair{3.0, 0.0},
                                 std::pair{-3.0, 0.0}, std::pair{0.0, 3.0}, std::pair{0.0, -3.0}}) {
        out.push_back(Candidate{piece.anchor + (x * dx + y * dy) * height, horizontal,
                                TextJustify::MiddleCentre, false, false});
    }
    out.push_back(Candidate{piece.anchor, horizontal, TextJustify::MiddleCentre, false, true});
}

void stationCandidates(const LabelPiece& piece, const LabelStyle& style, double tick, double gap,
                       std::vector<Candidate>& out)
{
    // Across the alignment, beyond the tick, reading outwards from it; on the
    // left looking along increasing chainage unless the style says below.
    const Vec2 left = unit(piece.direction).perpendicular();
    const bool leftFirst = style.placement != LabelPlacement::Below;
    for (const bool onLeft : {leftFirst, !leftFirst}) {
        const Vec2 side = onLeft ? left : -left;
        const double outward = side.angle();
        const Readable readable = readableRotation(outward, TextJustify::MiddleLeft);
        out.push_back(Candidate{piece.anchor + side * (tick + gap), readable.rotation,
                                readable.justify, false, false});
        if (!style.displace) {
            return;
        }
    }
}

// The marks a piece carries whatever its text does: a point's marker, a
// chainage tick.
Drawing marksOf(const LabelPiece& piece, const LabelStyle& style, double scale)
{
    Drawing marks;
    if (piece.shape == LabelPiece::Shape::Station) {
        const double half = katana::entity::annotationModelSize(style.tickLength, scale);
        if (half > 0.0) {
            const Vec2 across = unit(piece.direction).perpendicular() * half;
            marks.strokes.push_back({piece.anchor - across, piece.anchor + across});
        }
    } else if (piece.shape == LabelPiece::Shape::Point &&
               style.marker != katana::entity::LabelMarker::None) {
        const double half = 0.5 * katana::entity::annotationModelSize(style.markerSize, scale);
        const Point2& p = piece.anchor;
        switch (style.marker) {
        case katana::entity::LabelMarker::Cross:
            marks.strokes.push_back({p - Vec2(half, 0.0), p + Vec2(half, 0.0)});
            marks.strokes.push_back({p - Vec2(0.0, half), p + Vec2(0.0, half)});
            break;
        case katana::entity::LabelMarker::Dot:
            marks.fills.push_back(circlePolygon(p, half, 16));
            break;
        case katana::entity::LabelMarker::Circle:
            marks.outlines.push_back(circlePolygon(p, half, 32));
            break;
        case katana::entity::LabelMarker::None:
            break;
        }
    }
    marks.updateExtent();
    return marks;
}

} // namespace

bool convexOverlap(const std::vector<Point2>& a, const std::vector<Point2>& b)
{
    if (a.size() < 3 || b.size() < 3) {
        return false;
    }
    return !separated(a, b) && !separated(b, a);
}

namespace {

void appendPath(const std::vector<Point2>& path, bool closed, std::vector<Segment2>& out,
                std::size_t limit)
{
    for (std::size_t i = 0; i + 1 < path.size() && out.size() < limit; ++i) {
        out.push_back(Segment2{path[i], path[i + 1]});
    }
    if (closed && path.size() > 2 && out.size() < limit) {
        out.push_back(Segment2{path.back(), path.front()});
    }
}

void appendBox(const std::vector<Point2>& box, std::vector<Segment2>& out, std::size_t limit)
{
    appendPath(box, true, out, limit);
    if (box.size() == 4 && out.size() + 2 <= limit) {
        out.push_back(Segment2{box[0], box[2]});
        out.push_back(Segment2{box[1], box[3]});
    }
}

} // namespace

void appendKeepOut(const Drawing& drawing, std::vector<Segment2>& out, std::size_t limit)
{
    for (const auto& stroke : drawing.strokes) {
        appendPath(stroke, false, out, limit);
    }
    for (const auto& outline : drawing.outlines) {
        appendPath(outline, true, out, limit);
    }
    for (const auto& box : drawing.textBoxes) {
        appendBox(box, out, limit);
    }
}

void appendKeepOut(const DimensionDrawing& dimension, std::vector<Segment2>& out, std::size_t limit)
{
    for (const Segment2& line : dimension.extensionLines) {
        if (out.size() < limit) {
            out.push_back(line);
        }
    }
    if (dimension.hasDimensionLine && out.size() < limit) {
        out.push_back(dimension.dimensionLine);
    }
    for (const auto& curve : dimension.curves) {
        appendPath(curve, false, out, limit);
    }
    if (dimension.text.empty() || !(dimension.textHeight > 0.0)) {
        return;
    }
    // The figures' box from the baseline's left end, as the dimension draws
    // them: its estimated width, and a quarter height of room either side.
    const double width = katana::entity::estimatedWidth(dimension.text) * dimension.textHeight;
    const Vec2 along = unit(dimension.textRotation);
    const Vec2 up = along.perpendicular();
    const double margin = 0.25 * dimension.textHeight;
    const Point2 corner = dimension.textAnchor - along * margin - up * margin;
    const Vec2 x = along * (width + 2.0 * margin);
    const Vec2 y = up * (dimension.textHeight + 2.0 * margin);
    appendBox({corner, corner + x, corner + x + y, corner + y}, out, limit);
}

void appendLinework(const katana::entity::Geometry& geometry, std::vector<Segment2>& out,
                    std::size_t limit)
{
    const auto chord = [&](const katana::geometry::Arc2& arc) {
        const int count = std::max(4, static_cast<int>(std::ceil(16.0 * std::abs(arc.sweep) /
                                                                 katana::math::kTwoPi)));
        Point2 previous = arc.pointAt(0.0);
        for (int i = 1; i <= count && out.size() < limit; ++i) {
            const Point2 next = arc.pointAt(static_cast<double>(i) / count);
            out.push_back(Segment2{previous, next});
            previous = next;
        }
    };
    if (const auto* line = std::get_if<Segment2>(&geometry)) {
        if (out.size() < limit) {
            out.push_back(*line);
        }
    } else if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&geometry)) {
        appendPath(polyline->vertices, polyline->closed, out, limit);
    } else if (const auto* arc = std::get_if<katana::geometry::Arc2>(&geometry)) {
        chord(*arc);
    } else if (const auto* circle = std::get_if<katana::geometry::Circle2>(&geometry)) {
        chord(katana::geometry::Arc2{circle->center, circle->radius, 0.0, katana::math::kTwoPi});
    }
}

std::vector<Segment2> labelKeepOut(const katana::entity::Model& model, double scale,
                                   const TextMeasure& measure, std::size_t limit)
{
    std::vector<Segment2> out;
    model.entities.forEach([&](const Entity& entity) {
        if (out.size() >= limit || std::holds_alternative<LabelGeometry>(entity.geometry) ||
            !isDrawn(model, entity, kNoLayerOverrides)) {
            return;
        }
        appendLinework(entity.geometry, out, limit);
        if (const auto* text = std::get_if<katana::entity::TextGeometry>(&entity.geometry)) {
            appendKeepOut(layoutTextEntity(model, *text, scale, measure), out, limit);
        } else if (const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry)) {
            appendKeepOut(buildLeader(model, *leader, scale, measure), out, limit);
        } else if (const auto* dimension =
                       std::get_if<katana::entity::DimensionGeometry>(&entity.geometry)) {
            appendKeepOut(buildDimension(*dimension, resolveDimensionStyle(model, entity), scale),
                          out, limit);
        }
    });
    return out;
}

std::vector<const Entity*> labelEntities(const katana::entity::Model& model)
{
    std::vector<const Entity*> labels;
    model.entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<LabelGeometry>(entity.geometry)) {
            labels.push_back(&entity);
        }
    });
    return labels;
}

std::vector<LabelPiece> labelPiecesOf(const katana::entity::Model& model, const LabelGeometry& label,
                                      const LabelStyle& style)
{
    return katana::entity::labelPieces(model, label, style, codePropertyCandidates());
}

std::string surveyCode(const Entity& entity)
{
    for (const std::string& candidate : codePropertyCandidates()) {
        if (const std::string* code = surveyCodeOf(entity, candidate)) {
            return *code;
        }
    }
    return {};
}

std::string labelText(const LabelGeometry& label, const LabelStyle& style, const LabelPiece& piece)
{
    if (!label.textOverride.empty()) {
        return label.textOverride;
    }
    return katana::entity::formatLabel(style.text, piece.values);
}

std::string shownLabelText(const katana::entity::Model& model, const LabelGeometry& label)
{
    const LabelStyle* style = model.labelStyles.find(label.style);
    if (style == nullptr) {
        return {};
    }
    std::string joined;
    for (const LabelPiece& piece : labelPiecesOf(model, label, *style)) {
        if (piece.tickOnly) {
            continue;
        }
        const std::string text = labelText(label, *style, piece);
        if (text.empty()) {
            continue;
        }
        joined += joined.empty() ? "" : " | ";
        joined += text;
    }
    return joined;
}

LabelLayout layoutLabels(const katana::entity::Model& model, const std::vector<const Entity*>& labels,
                         const LabelLayoutOptions& options)
{
    LabelLayout layout;
    const double scale = options.scale > 0.0 && std::isfinite(options.scale)
                             ? options.scale
                             : katana::entity::kDefaultAnnotationScale;
    const Box2 reach =
        options.visible.empty()
            ? Box2{}
            : options.visible.inflated(katana::entity::annotationModelSize(kReachMillimetres, scale));
    // Horizontal on the sheet: the model direction of the paper's x axis.
    const double horizontal = options.horizontal;

    // 1. Every piece with text, in the placing order.
    std::vector<Pending> pending;
    for (const Entity* entity : labels) {
        const auto* label = entity != nullptr ? std::get_if<LabelGeometry>(&entity->geometry)
                                              : nullptr;
        if (label == nullptr) {
            continue;
        }
        const LabelStyle* style = model.labelStyles.find(label->style);
        if (style == nullptr) {
            ++layout.orphaned;
            continue;
        }
        std::vector<LabelPiece> pieces = labelPiecesOf(model, *label, *style);
        if (pieces.empty()) {
            ++layout.orphaned;
            continue;
        }
        const double minimum = katana::entity::annotationModelSize(style->minimumLength, scale);
        for (std::size_t i = 0; i < pieces.size(); ++i) {
            LabelPiece& piece = pieces[i];
            if (!reach.empty() && !reach.contains(piece.anchor)) {
                continue;
            }
            if ((piece.shape == LabelPiece::Shape::Segment || piece.shape == LabelPiece::Shape::Arc) &&
                piece.length < minimum) {
                continue;
            }
            Pending entry;
            entry.entity = entity;
            entry.label = label;
            entry.style = style;
            entry.pieceIndex = i;
            if (!piece.tickOnly) {
                entry.text = labelText(*label, *style, piece);
            }
            // The dragged position belongs to the label's first piece.
            entry.dragged = label->position.has_value() && i == 0;
            entry.piece = std::move(piece);
            pending.push_back(std::move(entry));
        }
    }
    std::stable_sort(pending.begin(), pending.end(), [](const Pending& a, const Pending& b) {
        if (a.dragged != b.dragged) {
            return a.dragged;
        }
        if (a.style->priority != b.style->priority) {
            return a.style->priority > b.style->priority;
        }
        if (a.entity->id != b.entity->id) {
            return a.entity->id < b.entity->id;
        }
        return a.pieceIndex < b.pieceIndex;
    });

    // 2. The obstacles.
    const double cell = katana::entity::annotationModelSize(kGridCellMillimetres, scale);
    Grid textGrid(cell);
    std::vector<std::vector<Point2>> placedBoxes;
    Grid lineGrid(cell);
    for (std::size_t i = 0; i < options.linework.size(); ++i) {
        lineGrid.insert(options.linework[i].boundingBox(), i);
    }
    const auto collides = [&](const std::vector<Point2>& box, bool onLine,
                              const Pending& entry) -> bool {
        const Box2 bounds = boxOf(box);
        const auto meetsText = [&](std::size_t index) { return convexOverlap(box, placedBoxes[index]); };
        if (textGrid.anyNear(bounds, meetsText)) {
            return true;
        }
        for (const std::size_t index : textGrid.oversized()) {
            if (meetsText(index)) {
                return true;
            }
        }
        if (onLine) {
            return false; // set on its own line, over a mask
        }
        const auto meetsLine = [&](std::size_t index) {
            const Segment2& line = options.linework[index];
            // Its own line does not push a label away: a label beside a
            // segment is placed clear of it by its offset.
            if (entry.piece.shape == LabelPiece::Shape::Segment &&
                ((line.start == entry.piece.from && line.end == entry.piece.to) ||
                 (line.start == entry.piece.to && line.end == entry.piece.from))) {
                return false;
            }
            return segmentMeetsConvex(box, line);
        };
        if (lineGrid.anyNear(bounds, meetsLine)) {
            return true;
        }
        for (const std::size_t index : lineGrid.oversized()) {
            if (meetsLine(index)) {
                return true;
            }
        }
        return false;
    };

    // 3. Place.
    for (const Pending& entry : pending) {
        const LabelStyle& style = *entry.style;
        const std::string textStyleName = style.textStyle.empty()
                                              ? std::string(katana::entity::kDefaultTextStyleName)
                                              : style.textStyle;
        TextAppearance appearance = resolveTextAppearance(
            model, textStyleName, style.paperHeight,
            katana::entity::annotationModelSize(2.5, scale), scale);
        // A label's placement has already made it readable.
        appearance.readable = false;
        if (style.color) {
            appearance.colour = style.color;
        }
        const double height = appearance.height;
        const double gap = katana::entity::annotationModelSize(style.offset, scale);
        Drawing marks = marksOf(entry.piece, style, scale);
        const auto keepMarks = [&] {
            if (!marks.empty()) {
                PlacedLabel only;
                only.label = entry.entity->id;
                only.piece = entry.pieceIndex;
                only.drawing = std::move(marks);
                layout.marksOnly.push_back(std::move(only));
            }
        };
        if (entry.text.empty()) {
            keepMarks();
            continue;
        }
        ++layout.considered;

        std::vector<Candidate> candidates;
        if (entry.dragged) {
            double rotation = horizontal;
            if (style.orientation == katana::entity::LabelOrientation::Aligned &&
                (entry.piece.shape == LabelPiece::Shape::Segment ||
                 entry.piece.shape == LabelPiece::Shape::Arc)) {
                rotation = readingAngle(entry.piece.direction);
            }
            candidates.push_back(Candidate{*entry.label->position, rotation,
                                           TextJustify::MiddleCentre, true, false});
        } else {
            switch (entry.piece.shape) {
            case LabelPiece::Shape::Point: {
                const double marker =
                    style.marker == katana::entity::LabelMarker::None
                        ? 0.0
                        : 0.5 * katana::entity::annotationModelSize(style.markerSize, scale);
                pointCandidates(entry.piece, style, gap + marker, height, horizontal, candidates);
                break;
            }
            case LabelPiece::Shape::Segment:
            case LabelPiece::Shape::Arc:
                lineCandidates(entry.piece, style, gap, height, horizontal, candidates);
                break;
            case LabelPiece::Shape::Area:
                areaCandidates(entry.piece, style, height, horizontal, candidates);
                break;
            case LabelPiece::Shape::Station:
                stationCandidates(entry.piece, style,
                                  katana::entity::annotationModelSize(style.tickLength, scale), gap,
                                  candidates);
                break;
            }
        }

        bool placed = false;
        for (std::size_t c = 0; c < candidates.size(); ++c) {
            const Candidate& candidate = candidates[c];
            TextAppearance chosen = appearance;
            chosen.mask = appearance.mask || candidate.onLine;
            if (candidate.onLine && !appearance.mask) {
                chosen.maskMargin = 0.25 * height;
            }
            Drawing drawing = layoutText(entry.text, candidate.at, candidate.rotation,
                                         candidate.justify, chosen, options.measure);
            if (drawing.textBoxes.empty()) {
                break;
            }
            const std::vector<Point2>& box = drawing.textBoxes.front();
            if (options.avoidCollisions && !entry.dragged && collides(box, candidate.onLine, entry)) {
                continue;
            }
            if (candidate.displaced && style.leader) {
                const Point2 end = nearestOnBoundary(box, entry.piece.anchor);
                if (end.distanceTo(entry.piece.anchor) > tol::kGeometric) {
                    drawing.strokes.push_back({entry.piece.anchor, end});
                }
            }
            drawing.append(marks);
            drawing.updateExtent();
            const std::size_t index = placedBoxes.size();
            placedBoxes.push_back(box);
            textGrid.insert(boxOf(box), index);
            PlacedLabel result;
            result.label = entry.entity->id;
            result.piece = entry.pieceIndex;
            result.drawing = std::move(drawing);
            result.candidate = c;
            result.displaced = candidate.displaced;
            layout.displaced += candidate.displaced ? 1 : 0;
            layout.placed.push_back(std::move(result));
            placed = true;
            break;
        }
        if (!placed) {
            ++layout.suppressed;
            layout.suppressedPieces.push_back({entry.entity->id, entry.pieceIndex});
            keepMarks();
        }
    }
    return layout;
}

} // namespace katana::cad::annotation
