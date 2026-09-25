#include "katana/cad/annotation/text_layout.hpp"

#include <algorithm>
#include <cmath>
#include <vector>

#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

using katana::entity::TextJustify;
namespace tol = katana::math::tolerance;

const katana::entity::TextStyle* findTextStyle(const katana::entity::Model& model,
                                               std::string_view name)
{
    if (!name.empty()) {
        if (const auto* style = model.textStyles.find(name); style != nullptr) {
            return style;
        }
    }
    return model.textStyles.find(katana::entity::kDefaultTextStyleName);
}

TextAppearance resolveTextAppearance(const katana::entity::Model& model, std::string_view styleName,
                                     double paperHeight, double modelHeight, double scale)
{
    TextAppearance appearance;
    const katana::entity::TextStyle* style =
        styleName.empty() ? nullptr : findTextStyle(model, styleName);
    const double stylePaper = style != nullptr ? style->paperHeight : 0.0;
    if (paperHeight > 0.0) {
        appearance.height = katana::entity::annotationModelSize(paperHeight, scale);
    } else if (stylePaper > 0.0) {
        appearance.height = katana::entity::annotationModelSize(stylePaper, scale);
    } else {
        appearance.height = modelHeight;
    }
    if (style != nullptr) {
        appearance.face = TextFace{style->fontFamily, style->bold, style->italic};
        appearance.widthFactor = style->widthFactor;
        appearance.oblique = style->oblique;
        appearance.colour = style->color;
        appearance.mask = style->mask;
        appearance.maskMargin = katana::entity::annotationModelSize(style->maskMargin, scale);
        appearance.readable = style->readable;
        appearance.lineSpacing = style->lineSpacing;
    }
    return appearance;
}

TextAppearance resolveTextStyle(const katana::entity::Model& model,
                                const katana::entity::TextGeometry& text, double scale)
{
    return resolveTextAppearance(model, text.style, text.paperHeight, text.height, scale);
}

bool isPaperSized(const katana::entity::Model& model, const katana::entity::TextGeometry& text)
{
    if (text.paperHeight > 0.0) {
        return true;
    }
    if (text.style.empty()) {
        return false;
    }
    const auto* style = findTextStyle(model, text.style);
    return style != nullptr && style->paperHeight > 0.0;
}

void fitModelHeight(const katana::entity::Model& model, double scale,
                    katana::entity::TextGeometry& text)
{
    if (isPaperSized(model, text)) {
        text.height = resolveTextStyle(model, text, scale).height;
    }
}

Readable readableRotation(double rotation, TextJustify justify)
{
    const double angle = katana::math::normalizeAngle(rotation);
    const double quarter = 0.5 * katana::math::kPi;
    Readable result{rotation, justify, false};
    if (angle > quarter + tol::kAngular && angle <= 3.0 * quarter + tol::kAngular) {
        result.rotation = rotation + katana::math::kPi;
        // Column 0 <-> 2 and row 0 <-> 2: the block turned about the point
        // covers the same ground when its anchor moves to the opposite corner.
        const int value = static_cast<int>(justify);
        const int column = 2 - value % 3;
        const int row = 2 - value / 3;
        result.justify = static_cast<TextJustify>(row * 3 + column);
        result.turned = true;
    }
    return result;
}

Drawing layoutText(std::string_view text, const Point2& position, double rotation,
                   TextJustify justify, const TextAppearance& appearance,
                   const TextMeasure& measure)
{
    Drawing drawing;
    drawing.colour = appearance.colour;
    if (!(appearance.height > 0.0) || !std::isfinite(appearance.height)) {
        return drawing;
    }
    if (appearance.readable) {
        const Readable readable = readableRotation(rotation, justify);
        rotation = readable.rotation;
        justify = readable.justify;
    }
    const double height = appearance.height;
    const std::vector<std::string_view> lines = katana::entity::textLines(text);
    std::vector<double> widths;
    widths.reserve(lines.size());
    double widest = 0.0;
    for (const std::string_view line : lines) {
        const double width = measure(line, appearance.face) * height * appearance.widthFactor;
        widths.push_back(width);
        widest = std::max(widest, width);
    }
    const katana::entity::TextBlockExtent block = katana::entity::textBlockExtent(
        widest, lines.size(), height, appearance.lineSpacing, justify);
    const auto at = [&](double x, double y) {
        return position + Vec2(x, y).rotated(rotation);
    };
    for (std::size_t i = 0; i < lines.size(); ++i) {
        if (lines[i].empty()) {
            continue;
        }
        const Point2 local = katana::entity::lineOrigin(block, i, widths[i], height,
                                                        appearance.lineSpacing, justify);
        TextRun run;
        run.text = std::string(lines[i]);
        run.origin = at(local.x, local.y);
        run.height = height;
        run.rotation = rotation;
        run.widthFactor = appearance.widthFactor;
        run.oblique = appearance.oblique;
        run.face = appearance.face;
        drawing.texts.push_back(std::move(run));
    }
    // The descenders hang a fifth of the height below the last baseline in
    // a typical face (the em box's descent); the box and the mask include
    // them, so a mask does not cut the tail off a g.
    const double descent = 0.2 * height;
    drawing.textBoxes.push_back({at(block.left, block.bottom - descent),
                                 at(block.right, block.bottom - descent), at(block.right, block.top),
                                 at(block.left, block.top)});
    if (appearance.mask) {
        const double m = appearance.maskMargin;
        drawing.masks.push_back({at(block.left - m, block.bottom - descent - m),
                                 at(block.right + m, block.bottom - descent - m),
                                 at(block.right + m, block.top + m),
                                 at(block.left - m, block.top + m)});
    }
    drawing.updateExtent();
    return drawing;
}

Drawing layoutTextEntity(const katana::entity::Model& model,
                         const katana::entity::TextGeometry& text, double scale,
                         const TextMeasure& measure)
{
    return layoutText(text.text, text.position, text.rotation, text.justify,
                      resolveTextStyle(model, text, scale), measure);
}

} // namespace katana::cad::annotation
