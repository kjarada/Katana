#include "katana/entity/text_block.hpp"

#include <algorithm>

namespace katana::entity {

using katana::geometry::Point2;
using katana::geometry::Vec2;

namespace {

// The justification's column and row as 0, 1, 2 - left, centre, right and
// bottom, middle, top - which is how the enumerators are numbered.
int columnOf(TextJustify justify)
{
    return static_cast<int>(justify) % 3;
}

int rowOf(TextJustify justify)
{
    return static_cast<int>(justify) / 3;
}

} // namespace

std::vector<std::string_view> textLines(std::string_view text)
{
    std::vector<std::string_view> lines;
    std::size_t start = 0;
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (text[i] == '\n') {
            lines.push_back(text.substr(start, i - start));
            start = i + 1;
        }
    }
    lines.push_back(text.substr(start));
    return lines;
}

std::size_t characterCount(std::string_view line)
{
    // A UTF-8 continuation byte is 10xxxxxx; every other byte starts a character.
    return static_cast<std::size_t>(std::count_if(line.begin(), line.end(), [](char c) {
        return (static_cast<unsigned char>(c) & 0xC0u) != 0x80u;
    }));
}

double estimatedWidth(std::string_view line)
{
    return kApproximateGlyphAspect * static_cast<double>(characterCount(line));
}

TextBlockExtent textBlockExtent(double width, std::size_t lineCount, double height,
                                double lineSpacing, TextJustify justify)
{
    const std::size_t lines = std::max<std::size_t>(lineCount, 1);
    const double pitch = kLinePitch * lineSpacing * height;
    const double blockHeight = height + pitch * static_cast<double>(lines - 1);

    TextBlockExtent extent;
    switch (columnOf(justify)) {
    case 0:
        extent.left = 0.0;
        break;
    case 1:
        extent.left = -0.5 * width;
        break;
    default:
        extent.left = -width;
        break;
    }
    extent.right = extent.left + width;
    switch (rowOf(justify)) {
    case 0:
        extent.bottom = 0.0;
        break;
    case 1:
        extent.bottom = -0.5 * blockHeight;
        break;
    default:
        extent.bottom = -blockHeight;
        break;
    }
    extent.top = extent.bottom + blockHeight;
    return extent;
}

Point2 lineOrigin(const TextBlockExtent& block, std::size_t index, double lineWidth, double height,
                  double lineSpacing, TextJustify justify)
{
    const double pitch = kLinePitch * lineSpacing * height;
    const double baseline = block.top - height - pitch * static_cast<double>(index);
    double x = block.left;
    switch (columnOf(justify)) {
    case 0:
        break;
    case 1:
        x = block.left + 0.5 * (block.width() - lineWidth);
        break;
    default:
        x = block.right - lineWidth;
        break;
    }
    return Point2(x, baseline);
}

std::array<Point2, 4> estimatedTextCorners(const TextGeometry& text)
{
    const std::vector<std::string_view> lines = textLines(text.text);
    double widest = 0.0;
    for (const std::string_view line : lines) {
        widest = std::max(widest, estimatedWidth(line));
    }
    const TextBlockExtent block =
        textBlockExtent(widest * text.height, lines.size(), text.height, 1.0, text.justify);
    const auto at = [&](double x, double y) {
        return text.position + Vec2(x, y).rotated(text.rotation);
    };
    return {at(block.left, block.bottom), at(block.right, block.bottom), at(block.right, block.top),
            at(block.left, block.top)};
}

} // namespace katana::entity
