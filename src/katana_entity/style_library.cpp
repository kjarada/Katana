#include "katana/entity/style_library.hpp"

#include <algorithm>
#include <cmath>
#include <set>

#include "katana/entity/entity.hpp"
#include "validation.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

katana::geometry::Box2 LineStyle::bounds() const
{
    katana::geometry::Box2 box;
    // A circle, an arc, a dot and a text are all drawn ABOUT THE CURRENT
    // POINT and carry no point of their own, so the pen has to be followed to
    // know where they are. Getting this wrong put every circle at the origin.
    katana::geometry::Point2 pen{};
    for (const Stroke& stroke : strokes) {
        if (stroke.op == StrokeOp::Pen) {
            continue; // a colour, not a mark
        }
        if (stroke.op == StrokeOp::Move || stroke.op == StrokeOp::Draw) {
            pen = stroke.point;
        }
        const katana::geometry::Point2 at{pen.x * factor, pen.y * factor};
        box.expand(at);
        if (stroke.op == StrokeOp::Circle || stroke.op == StrokeOp::Arc) {
            // The radius is in the same units as the coordinates, so it is
            // scaled the same way. An arc is bounded by its full circle
            // rather than its sweep: cheap, never too small, and a symbol's
            // box is used to lay a preview out, not to clip.
            const double radius = std::abs(stroke.radius) * factor;
            box.expand({at.x - radius, at.y - radius});
            box.expand({at.x + radius, at.y + radius});
        }
    }
    return box;
}

Status validate(const LineStyle& style)
{
    if (auto status = detail::validateName(style.name, "linestyle"); !status) {
        return status;
    }
    if (!isValidUtf8(style.group)) {
        return makeError(ErrorCode::InvalidArgument, "linestyle group is not valid UTF-8",
                         style.name);
    }
    if (!(std::isfinite(style.length) && style.length >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "linestyle length must be finite and not negative", style.name);
    }
    if (!(std::isfinite(style.factor) && style.factor > 0.0)) {
        // A factor of 0 would collapse every stroke onto the origin, which
        // reads as "the definition is empty" rather than as the mistake it is.
        return makeError(ErrorCode::InvalidArgument,
                         "linestyle factor must be finite and greater than zero", style.name);
    }
    for (const auto& point : {style.origin, style.anchor1, style.anchor2}) {
        if (!(std::isfinite(point.x) && std::isfinite(point.y))) {
            return makeError(ErrorCode::InvalidArgument, "linestyle origin is not finite",
                             style.name);
        }
    }
    for (const Stroke& stroke : style.strokes) {
        if (!(std::isfinite(stroke.point.x) && std::isfinite(stroke.point.y) &&
              std::isfinite(stroke.radius) && std::isfinite(stroke.startAngle) &&
              std::isfinite(stroke.endAngle))) {
            return makeError(ErrorCode::InvalidArgument, "a stroke of this linestyle is not finite",
                             style.name);
        }
        if (stroke.op == StrokeOp::Text && stroke.text >= style.texts.size()) {
            return makeError(ErrorCode::InvalidArgument,
                             "a text stroke of this linestyle names no text", style.name);
        }
        if (!isValidUtf8(stroke.pen)) {
            return makeError(ErrorCode::InvalidArgument, "a pen name is not valid UTF-8",
                             style.name);
        }
    }
    for (const StrokeText& text : style.texts) {
        if (!isValidUtf8(text.text) || !isValidUtf8(text.justify) || !isValidUtf8(text.font)) {
            return makeError(ErrorCode::InvalidArgument, "linestyle text is not valid UTF-8",
                             style.name);
        }
        if (!(std::isfinite(text.angle) && std::isfinite(text.height) &&
              std::isfinite(text.widthFactor))) {
            return makeError(ErrorCode::InvalidArgument, "linestyle text is not finite",
                             style.name);
        }
        if (text.height < 0.0) {
            return makeError(ErrorCode::InvalidArgument, "linestyle text height is negative",
                             style.name);
        }
    }
    return {};
}

Status LineStylePolicy::validate(const LineStyle& style)
{
    return katana::entity::validate(style);
}

Result<bool> addOrReplace(StyleLibrary& library, LineStyle style)
{
    const bool replaced = library.contains(style.name);
    const Status status = replaced ? library.update(style) : library.add(std::move(style));
    if (!status) {
        return status.error();
    }
    return replaced;
}

std::vector<std::string> styleGroups(const StyleLibrary& library)
{
    std::set<std::string> distinct;
    for (const std::string& name : library.names()) {
        const LineStyle* style = library.find(name);
        if (style != nullptr && !style->group.empty()) {
            distinct.insert(style->group);
        }
    }
    return {distinct.begin(), distinct.end()};
}

std::vector<std::string> vertexStyleNames(const StyleLibrary& library)
{
    std::vector<std::string> names;
    for (const std::string& name : library.names()) {
        const LineStyle* style = library.find(name);
        if (style != nullptr && style->atVertices) {
            names.push_back(name);
        }
    }
    return names; // names() is already ascending
}

} // namespace katana::entity
