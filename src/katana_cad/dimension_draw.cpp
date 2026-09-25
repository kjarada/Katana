#include "katana/cad/dimension_draw.hpp"

#include <cmath>

#include "katana/entity/annotation.hpp"
#include "katana/entity/dimension_text.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;

using katana::entity::ArrowHead;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionStyle;
using katana::geometry::Vec2;

namespace {

// Advance per character as a fraction of the text height. The viewport draws
// text with a simple stroke font whose glyphs are about six tenths as wide as
// they are tall; the same figure is used to centre the label and to size its
// extent, so the two cannot disagree.
constexpr double kAdvancePerCharacter = 0.6;

// A closed arrowhead is three times as long as it is wide, which is the
// proportion DIMASZ implies and what reads as an arrow rather than a wedge.
constexpr double kArrowWidthFraction = 1.0 / 3.0;

// Sides of the polygon approximating a dot head. Twelve is smooth at any size a
// dimension arrow is drawn, and cheap.
constexpr int kDotSides = 12;

void appendArrow(DimensionDrawing& drawing, const Point2& tip, const Vec2& along,
                 const Vec2& normal, const DimensionStyle& style)
{
    const double size = style.arrowSize;
    switch (style.arrowHead) {
    case ArrowHead::None:
        break;
    case ArrowHead::Tick: {
        // The surveyor's oblique tick: a stroke at 45 degrees through the point,
        // extending equally either side.
        const Vec2 direction = (along + normal).normalized();
        drawing.arrowStrokes.push_back(
            Segment2{tip - direction * (size * 0.5), tip + direction * (size * 0.5)});
        break;
    }
    case ArrowHead::Open: {
        const Point2 back = tip - along * size;
        const Vec2 half = normal * (size * kArrowWidthFraction);
        drawing.arrowStrokes.push_back(Segment2{tip, back + half});
        drawing.arrowStrokes.push_back(Segment2{tip, back - half});
        break;
    }
    case ArrowHead::ClosedFilled: {
        const Point2 back = tip - along * size;
        const Vec2 half = normal * (size * kArrowWidthFraction);
        drawing.arrowFills.push_back({tip, back + half, back - half});
        break;
    }
    case ArrowHead::Dot: {
        const double radius = size * 0.25;
        std::vector<Point2> circle;
        circle.reserve(kDotSides);
        for (int i = 0; i < kDotSides; ++i) {
            const double angle =
                6.283185307179586476925 * static_cast<double>(i) / static_cast<double>(kDotSides);
            circle.emplace_back(tip.x + std::cos(angle) * radius, tip.y + std::sin(angle) * radius);
        }
        drawing.arrowFills.push_back(std::move(circle));
        break;
    }
    }
}

// Sides of the polygon an angular dimension's arc is chorded into per full
// turn: 128 keeps the chord's sagitta under a hundredth of a percent of the
// radius, invisible at any size a dimension arc is drawn.
constexpr int kArcChordsPerTurn = 128;

// The label's width by the same estimate the extent uses, counted in
// characters rather than bytes so the degree and diameter signs count once.
double labelWidth(const std::string& text, double height)
{
    return height * kAdvancePerCharacter * static_cast<double>(katana::entity::characterCount(text));
}

// A direction turned to read left to right: the flip every label makes when
// its line runs from right to left.
Vec2 readable(const Vec2& along)
{
    const double angle = along.angle();
    constexpr double kHalfPi = 1.570796326794896619231;
    return (angle > kHalfPi || angle < -kHalfPi) ? -along : along;
}

// Places the label centred on `middle`, `gap` off the line on the side
// `outward` points to, running along `along` (turned to read), and records
// the text box in the extent.
void placeLabel(DimensionDrawing& drawing, const Point2& middle, const Vec2& along,
                const Vec2& outward, const DimensionStyle& style)
{
    const Vec2 textAlong = readable(along);
    drawing.textRotation = textAlong.angle();
    const double width = labelWidth(drawing.text, style.textHeight);
    // Above the line when `outward` is the text's up, below it (by its own
    // height) when it is the text's down, so the gap is the gap either way.
    const Vec2 up = textAlong.perpendicular();
    const double lift = outward.dot(up) >= 0.0 ? style.textGap : -(style.textGap + style.textHeight);
    drawing.textAnchor = middle + up * lift - textAlong * (width * 0.5);
    drawing.extent.expand(drawing.textAnchor);
    drawing.extent.expand(drawing.textAnchor + textAlong * width);
    drawing.extent.expand(drawing.textAnchor + up * style.textHeight);
    drawing.extent.expand(drawing.textAnchor + textAlong * width + up * style.textHeight);
}

void expandByPieces(DimensionDrawing& drawing)
{
    for (const Segment2& segment : drawing.extensionLines) {
        drawing.extent.expand(segment.start);
        drawing.extent.expand(segment.end);
    }
    if (drawing.hasDimensionLine) {
        drawing.extent.expand(drawing.dimensionLine.start);
        drawing.extent.expand(drawing.dimensionLine.end);
    }
    for (const auto& curve : drawing.curves) {
        for (const Point2& point : curve) {
            drawing.extent.expand(point);
        }
    }
    for (const Segment2& stroke : drawing.arrowStrokes) {
        drawing.extent.expand(stroke.start);
        drawing.extent.expand(stroke.end);
    }
    for (const std::vector<Point2>& fill : drawing.arrowFills) {
        for (const Point2& point : fill) {
            drawing.extent.expand(point);
        }
    }
}

// An extension line from the measured point `from` to `foot` on the
// dimension line, gapped at the point and running past the line. `fallback`
// is the direction when the two coincide (a point already on the line).
Segment2 extensionLine(const Point2& from, const Point2& foot, const Vec2& fallback,
                       const DimensionStyle& style)
{
    const Vec2 towards = foot - from;
    const Vec2 direction = towards.length() > tol::kGeometric ? towards.normalized() : fallback;
    return Segment2{from + direction * style.extensionOffset,
                    foot + direction * style.extensionBeyond};
}

// The Aligned kind: the construction every dimension had before the kinds.
DimensionDrawing buildAligned(const DimensionGeometry& dimension, const DimensionStyle& style)
{
    DimensionDrawing drawing;

    const Vec2 span = dimension.end - dimension.start;
    const double length = span.length();
    if (!(length > tol::kGeometric) || !std::isfinite(dimension.offset)) {
        // No direction to offset along. Returning nothing beats inventing an
        // orientation and drawing a dimension pointing somewhere arbitrary.
        return drawing;
    }
    const Vec2 along = span / length;
    const Vec2 normal = along.perpendicular();

    // Which side the dimension line sits on. Everything that reaches "away from
    // the measured points" is expressed through this, so a negative offset
    // mirrors the whole construction rather than turning it inside out.
    const double side = dimension.offset >= 0.0 ? 1.0 : -1.0;
    const Vec2 outward = normal * side;

    const Point2 a = dimension.start + normal * dimension.offset;
    const Point2 b = dimension.end + normal * dimension.offset;
    drawing.dimensionLine = Segment2{a, b};

    // DIMEXO leaves a gap at the measured point so the extension line does not
    // touch the feature; DIMEXE runs it past the dimension line.
    drawing.extensionLines.push_back(
        Segment2{dimension.start + outward * style.extensionOffset, a + outward * style.extensionBeyond});
    drawing.extensionLines.push_back(
        Segment2{dimension.end + outward * style.extensionOffset, b + outward * style.extensionBeyond});

    // Arrows point OUTWARD from the dimension line's ends, which is what they do
    // when the text fits between them.
    appendArrow(drawing, a, -along, normal, style);
    appendArrow(drawing, b, along, normal, style);

    drawing.text = dimensionText(dimension, style);
    drawing.textHeight = style.textHeight;

    // Rotated with the dimension, and flipped when that would leave it upside
    // down: a label is read left to right whichever way the dimension runs.
    double rotation = along.angle();
    Vec2 textAlong = along;
    constexpr double kHalfPi = 1.570796326794896619231;
    if (rotation > kHalfPi || rotation < -kHalfPi) {
        rotation += 3.141592653589793238463;
        if (rotation > 3.141592653589793238463) {
            rotation -= 6.283185307179586476925;
        }
        textAlong = -along;
    }
    drawing.textRotation = rotation;

    const double width =
        style.textHeight * kAdvancePerCharacter * static_cast<double>(drawing.text.size());
    const Point2 middle = (a + b) * 0.5;
    // Sat off the dimension line by DIMGAP, on the side away from the measured
    // points, and backed up half its width so it reads centred.
    drawing.textAnchor = middle + outward * (style.textGap) - textAlong * (width * 0.5);

    // The extent covers what is DRAWN, not what is measured.
    drawing.extent.expand(dimension.start);
    drawing.extent.expand(dimension.end);
    expandByPieces(drawing);
    // Both corners of the text box, so a rotated label is covered whichever way
    // it runs.
    const Vec2 up = Vec2(-textAlong.y, textAlong.x);
    drawing.extent.expand(drawing.textAnchor);
    drawing.extent.expand(drawing.textAnchor + textAlong * width);
    drawing.extent.expand(drawing.textAnchor + up * style.textHeight);
    drawing.extent.expand(drawing.textAnchor + textAlong * width + up * style.textHeight);

    return drawing;
}

// Linear: horizontal, vertical or rotated. The dimension line is parallel to
// the measuring direction, `offset` to its left through start; each measured
// point is carried to it by its own extension line.
DimensionDrawing buildLinear(const DimensionGeometry& dimension, const DimensionStyle& style)
{
    DimensionDrawing drawing;
    const Vec2 along(std::cos(dimension.angle), std::sin(dimension.angle));
    const Vec2 normal = along.perpendicular();
    const double measured = (dimension.end - dimension.start).dot(along);
    if (!(std::abs(measured) > tol::kGeometric)) {
        return drawing;
    }
    const Point2 a = dimension.start + normal * dimension.offset;
    const Point2 b = a + along * measured;
    drawing.dimensionLine = Segment2{a, b};
    const Vec2 outward = normal * (dimension.offset >= 0.0 ? 1.0 : -1.0);
    drawing.extensionLines.push_back(extensionLine(dimension.start, a, outward, style));
    drawing.extensionLines.push_back(extensionLine(dimension.end, b, outward, style));
    const Vec2 lineAlong = (b - a).normalized();
    appendArrow(drawing, a, -lineAlong, lineAlong.perpendicular(), style);
    appendArrow(drawing, b, lineAlong, lineAlong.perpendicular(), style);
    drawing.text = dimensionText(dimension, style);
    drawing.textHeight = style.textHeight;
    drawing.extent.expand(dimension.start);
    drawing.extent.expand(dimension.end);
    expandByPieces(drawing);
    placeLabel(drawing, (a + b) * 0.5, lineAlong, outward, style);
    return drawing;
}

// Angular: an arc about the vertex from the start ray counter-clockwise to
// the end ray, extension lines along the rays out to the arc when it lies
// beyond them, arrows along the arc at its ends, the value outside it.
DimensionDrawing buildAngular(const DimensionGeometry& dimension, const DimensionStyle& style)
{
    DimensionDrawing drawing;
    drawing.hasDimensionLine = false;
    const Vec2 first = dimension.start - dimension.vertex;
    const Vec2 second = dimension.end - dimension.vertex;
    const double sweep = dimension.measurement();
    if (!(first.length() > tol::kGeometric) || !(second.length() > tol::kGeometric) ||
        !(sweep > tol::kAngular)) {
        return drawing;
    }
    const double radius = dimension.offset > tol::kGeometric
                              ? dimension.offset
                              : std::min(first.length(), second.length());
    const katana::geometry::Arc2 arc{dimension.vertex, radius, first.angle(), sweep};
    const int chords = std::max(
        8, static_cast<int>(std::ceil(kArcChordsPerTurn * sweep / katana::math::kTwoPi)));
    std::vector<Point2> curve;
    curve.reserve(static_cast<std::size_t>(chords) + 1);
    for (int i = 0; i <= chords; ++i) {
        curve.push_back(arc.pointAt(static_cast<double>(i) / chords));
    }
    const Point2 arcStart = curve.front();
    const Point2 arcEnd = curve.back();
    drawing.curves.push_back(std::move(curve));
    for (const auto& [ray, onArc] : {std::pair{first, arcStart}, std::pair{second, arcEnd}}) {
        const Point2 measured = dimension.vertex + ray;
        // The ray reaches the arc already when the arc is inside its length.
        if (radius > ray.length() + tol::kGeometric) {
            drawing.extensionLines.push_back(
                extensionLine(measured, onArc, ray.normalized(), style));
        }
    }
    const Vec2 radialStart = (arcStart - dimension.vertex).normalized();
    const Vec2 radialEnd = (arcEnd - dimension.vertex).normalized();
    // Arrows point out of the arc along its tangents.
    appendArrow(drawing, arcStart, -radialStart.perpendicular(), radialStart, style);
    appendArrow(drawing, arcEnd, radialEnd.perpendicular(), radialEnd, style);
    drawing.text = dimensionText(dimension, style);
    drawing.textHeight = style.textHeight;
    const Point2 middle = arc.pointAt(0.5);
    const Vec2 radialMiddle = (middle - dimension.vertex).normalized();
    expandByPieces(drawing);
    placeLabel(drawing, middle, radialMiddle.perpendicular(), radialMiddle, style);
    return drawing;
}

// Radius and diameter: a line through the centre and the point on the curve,
// continued `offset` beyond it to the value; arrows at the curve pointing
// out to it.
DimensionDrawing buildRadial(const DimensionGeometry& dimension, const DimensionStyle& style)
{
    DimensionDrawing drawing;
    drawing.hasDimensionLine = false;
    const Vec2 radial = dimension.start - dimension.vertex;
    const double radius = radial.length();
    if (!(radius > tol::kGeometric)) {
        return drawing;
    }
    const Vec2 out = radial / radius;
    const bool diameter = dimension.kind == katana::entity::DimensionKind::Diameter;
    const Point2 from = diameter ? dimension.vertex - radial : dimension.vertex;
    const Point2 to = dimension.start + out * std::max(dimension.offset, 0.0);
    drawing.curves.push_back({from, to});
    appendArrow(drawing, dimension.start, out, out.perpendicular(), style);
    if (diameter) {
        appendArrow(drawing, from, -out, out.perpendicular(), style);
    }
    drawing.text = dimensionText(dimension, style);
    drawing.textHeight = style.textHeight;
    expandByPieces(drawing);
    // The value along the part of the line outside the curve, or about the
    // point on the curve when the line stops there.
    const Point2 middle =
        dimension.offset > tol::kGeometric ? dimension.start + out * (0.5 * dimension.offset)
                                           : dimension.vertex + radial * 0.5;
    placeLabel(drawing, middle, out, readable(out).perpendicular(), style);
    return drawing;
}

// Ordinate: a leader from the feature to where the value is written, the
// value along the leader's last stretch.
DimensionDrawing buildOrdinate(const DimensionGeometry& dimension, const DimensionStyle& style)
{
    DimensionDrawing drawing;
    drawing.hasDimensionLine = false;
    const Vec2 leader = dimension.end - dimension.start;
    const double length = leader.length();
    if (!(length > tol::kGeometric)) {
        return drawing;
    }
    const Vec2 along = leader / length;
    const double gap = std::min(style.extensionOffset, 0.5 * length);
    drawing.curves.push_back({dimension.start + along * gap, dimension.end});
    drawing.text = dimensionText(dimension, style);
    drawing.textHeight = style.textHeight;
    expandByPieces(drawing);
    // Beyond the leader's end, reading along it.
    const Vec2 textAlong = readable(along);
    const double width = labelWidth(drawing.text, style.textHeight);
    drawing.textRotation = textAlong.angle();
    const Point2 base = dimension.end + along * style.textGap;
    // From the end outwards: a text that reads back along the leader is set
    // from its far end.
    const Point2 start = textAlong.dot(along) >= 0.0 ? base : base + along * width;
    drawing.textAnchor = start - textAlong.perpendicular() * (0.5 * style.textHeight);
    const Vec2 up = textAlong.perpendicular();
    drawing.extent.expand(drawing.textAnchor);
    drawing.extent.expand(drawing.textAnchor + textAlong * width);
    drawing.extent.expand(drawing.textAnchor + up * style.textHeight);
    drawing.extent.expand(drawing.textAnchor + textAlong * width + up * style.textHeight);
    return drawing;
}

} // namespace

DimensionStyle dimensionStyleAtScale(const DimensionStyle& style, double scale)
{
    if (!style.paperSized) {
        return style;
    }
    DimensionStyle sized = style;
    for (double* size : {&sized.textHeight, &sized.textGap, &sized.extensionOffset,
                         &sized.extensionBeyond, &sized.arrowSize}) {
        *size = katana::entity::annotationModelSize(*size, scale);
    }
    return sized;
}

std::string dimensionText(const DimensionGeometry& dimension, const DimensionStyle& style)
{
    using katana::entity::DimensionKind;
    if (!dimension.textOverride.empty()) {
        return dimension.textOverride;
    }
    switch (dimension.kind) {
    case DimensionKind::Angular:
        // Degrees, minutes and whole seconds: the form an angle on a survey
        // drawing is read in. The style's unit scale and rounding are for
        // lengths and do not apply.
        return style.prefix + katana::entity::formatDms(dimension.measurement(), 0) + style.suffix;
    case DimensionKind::Radius:
    case DimensionKind::Diameter: {
        std::string text = katana::entity::dimensionLabel(dimension.measurement(), {}, style);
        if (style.prefix.empty()) {
            // ISO 129-1 writes a radius "R" and a diameter with the diameter
            // sign; U+00D8 is the form every face has.
            text.insert(0, dimension.kind == DimensionKind::Radius ? "R" : "\u00D8");
        }
        return text;
    }
    case DimensionKind::Aligned:
    case DimensionKind::Linear:
    case DimensionKind::OrdinateX:
    case DimensionKind::OrdinateY:
        break;
    }
    return katana::entity::dimensionLabel(dimension.measurement(), {}, style);
}

DimensionDrawing buildDimension(const DimensionGeometry& dimension, const DimensionStyle& style,
                                double scale)
{
    using katana::entity::DimensionKind;
    const DimensionStyle sized = dimensionStyleAtScale(style, scale);
    switch (dimension.kind) {
    case DimensionKind::Aligned:
        return buildAligned(dimension, sized);
    case DimensionKind::Linear:
        return buildLinear(dimension, sized);
    case DimensionKind::Angular:
        return buildAngular(dimension, sized);
    case DimensionKind::Radius:
    case DimensionKind::Diameter:
        return buildRadial(dimension, sized);
    case DimensionKind::OrdinateX:
    case DimensionKind::OrdinateY:
        return buildOrdinate(dimension, sized);
    }
    return {};
}

const DimensionStyle& resolveDimensionStyle(const katana::entity::Model& model,
                                            const katana::entity::Entity& entity)
{
    // Never null: a renderer with no style has nothing to draw with, and a
    // missing style is not a reason to omit a dimension from the drawing.
    static const DimensionStyle fallback;

    const katana::entity::Layer* layer = model.layers.find(entity.layer);
    if (layer != nullptr && !layer->dimensionStyle.empty()) {
        if (const auto* named = model.dimensionStyles.find(layer->dimensionStyle);
            named != nullptr) {
            return *named;
        }
        // A layer naming a style that is not there resolves to the default, the
        // same way a layer naming a missing linetype draws solid: the drawing
        // still appears, in the document's default style.
    }
    if (const auto* standard =
            model.dimensionStyles.find(katana::entity::kDefaultDimensionStyleName);
        standard != nullptr) {
        return *standard;
    }
    return fallback;
}

} // namespace katana::cad
