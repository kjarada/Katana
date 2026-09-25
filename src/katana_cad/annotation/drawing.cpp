#include "katana/cad/annotation/drawing.hpp"

#include <cmath>

#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

TextMeasure estimatedMeasure()
{
    return [](std::string_view line, const TextFace&) {
        return katana::entity::estimatedWidth(line);
    };
}

void Drawing::updateExtent()
{
    extent = Box2{};
    const auto add = [&](const std::vector<std::vector<Point2>>& lists) {
        for (const auto& list : lists) {
            for (const Point2& point : list) {
                extent.expand(point);
            }
        }
    };
    add(masks);
    add(strokes);
    add(outlines);
    add(fills);
    add(textBoxes);
}

void Drawing::append(const Drawing& other)
{
    masks.insert(masks.end(), other.masks.begin(), other.masks.end());
    strokes.insert(strokes.end(), other.strokes.begin(), other.strokes.end());
    outlines.insert(outlines.end(), other.outlines.begin(), other.outlines.end());
    fills.insert(fills.end(), other.fills.begin(), other.fills.end());
    texts.insert(texts.end(), other.texts.begin(), other.texts.end());
    textBoxes.insert(textBoxes.end(), other.textBoxes.begin(), other.textBoxes.end());
    if (!other.extent.empty()) {
        extent.expand(other.extent.min);
        extent.expand(other.extent.max);
    }
}

std::vector<Point2> circlePolygon(const Point2& centre, double radius, int segments)
{
    std::vector<Point2> points;
    const int count = std::max(segments, 8);
    points.reserve(static_cast<std::size_t>(count));
    for (int i = 0; i < count; ++i) {
        const double angle = katana::math::kTwoPi * static_cast<double>(i) / count;
        points.push_back(centre + Vec2(std::cos(angle), std::sin(angle)) * radius);
    }
    return points;
}

} // namespace katana::cad::annotation
