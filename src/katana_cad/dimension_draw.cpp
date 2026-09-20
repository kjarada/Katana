#include "katana/cad/dimension_draw.hpp"

#include <cmath>

#include "katana/entity/dimension_text.hpp"
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

} // namespace

DimensionDrawing buildDimension(const DimensionGeometry& dimension, const DimensionStyle& style)
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

    drawing.text = katana::entity::dimensionLabel(dimension.measurement(),
                                                  dimension.textOverride, style);
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
    for (const Segment2& segment : drawing.extensionLines) {
        drawing.extent.expand(segment.start);
        drawing.extent.expand(segment.end);
    }
    drawing.extent.expand(drawing.dimensionLine.start);
    drawing.extent.expand(drawing.dimensionLine.end);
    for (const Segment2& stroke : drawing.arrowStrokes) {
        drawing.extent.expand(stroke.start);
        drawing.extent.expand(stroke.end);
    }
    for (const std::vector<Point2>& fill : drawing.arrowFills) {
        for (const Point2& point : fill) {
            drawing.extent.expand(point);
        }
    }
    // Both corners of the text box, so a rotated label is covered whichever way
    // it runs.
    const Vec2 up = Vec2(-textAlong.y, textAlong.x);
    drawing.extent.expand(drawing.textAnchor);
    drawing.extent.expand(drawing.textAnchor + textAlong * width);
    drawing.extent.expand(drawing.textAnchor + up * style.textHeight);
    drawing.extent.expand(drawing.textAnchor + textAlong * width + up * style.textHeight);

    return drawing;
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
