#include "katana/cad/annotation/export_annotation.hpp"

#include <variant>

#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/annotation/leader_draw.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

using katana::entity::Entity;
using katana::entity::Geometry;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
namespace tol = katana::math::tolerance;

namespace {

// As much of the keep-out as the plan view gathers (plan_painter.cpp's
// kMaximumLinework), so a file is judged as the screen is.
constexpr std::size_t kKeepOutLimit = 200000;

// A path as one shape: a segment for two points, a polyline for more, and
// nothing for one that has no length.
void appendPath(const std::vector<Point2>& path, bool closed, std::vector<Geometry>& out)
{
    if (path.size() < 2) {
        return;
    }
    if (path.size() == 2 && !closed) {
        if (path[0].distanceTo(path[1]) > tol::kGeometric) {
            out.emplace_back(Segment2{path[0], path[1]});
        }
        return;
    }
    Polyline2 polyline{path, closed && path.size() > 2};
    if (polyline.length() > tol::kGeometric) {
        out.emplace_back(std::move(polyline));
    }
}

void appendText(const std::string& text, const Point2& at, double height, double rotation,
                std::vector<Geometry>& out)
{
    if (text.empty() || !(height > 0.0)) {
        return;
    }
    katana::entity::TextGeometry line;
    line.position = at;
    line.text = text;
    line.height = height;
    line.rotation = rotation;
    out.emplace_back(std::move(line));
}

} // namespace

std::vector<Geometry> plainShapes(const Drawing& drawing)
{
    std::vector<Geometry> out;
    for (const auto& stroke : drawing.strokes) {
        appendPath(stroke, false, out);
    }
    for (const auto& outline : drawing.outlines) {
        appendPath(outline, true, out);
    }
    for (const auto& fill : drawing.fills) {
        appendPath(fill, true, out);
    }
    for (const TextRun& run : drawing.texts) {
        appendText(run.text, run.origin, run.height, run.rotation, out);
    }
    return out;
}

std::vector<Geometry> plainShapes(const DimensionDrawing& drawing)
{
    std::vector<Geometry> out;
    for (const Segment2& line : drawing.extensionLines) {
        appendPath({line.start, line.end}, false, out);
    }
    if (drawing.hasDimensionLine) {
        appendPath({drawing.dimensionLine.start, drawing.dimensionLine.end}, false, out);
    }
    for (const auto& curve : drawing.curves) {
        appendPath(curve, false, out);
    }
    for (const Segment2& stroke : drawing.arrowStrokes) {
        appendPath({stroke.start, stroke.end}, false, out);
    }
    for (const auto& fill : drawing.arrowFills) {
        appendPath(fill, true, out);
    }
    appendText(drawing.text, drawing.textAnchor, drawing.textHeight, drawing.textRotation, out);
    return out;
}

DrawnAnnotation drawAnnotationForExport(const katana::entity::Model& model, double scale,
                                        const TextMeasure& measure)
{
    DrawnAnnotation drawn;
    model.entities.forEach([&](const Entity& entity) {
        if (const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry)) {
            drawn[entity.id] = plainShapes(buildLeader(model, *leader, scale, measure));
        } else if (const auto* dimension =
                       std::get_if<katana::entity::DimensionGeometry>(&entity.geometry);
                   dimension != nullptr && dimension->kind != katana::entity::DimensionKind::Aligned) {
            drawn[entity.id] =
                plainShapes(buildDimension(*dimension, resolveDimensionStyle(model, entity), scale));
        } else if (std::holds_alternative<katana::entity::LabelGeometry>(entity.geometry)) {
            drawn[entity.id]; // nothing unless the placer puts it somewhere
        }
    });

    // The labels, placed together as the plan view places them.
    LabelLayoutOptions options;
    options.scale = scale;
    options.measure = measure;
    options.linework = labelKeepOut(model, scale, measure, kKeepOutLimit);
    const LabelLayout layout = layoutLabels(model, labelEntities(model), options);
    for (const auto* placed : {&layout.placed, &layout.marksOnly}) {
        for (const PlacedLabel& piece : *placed) {
            std::vector<Geometry> shapes = plainShapes(piece.drawing);
            auto& into = drawn[piece.label];
            into.insert(into.end(), std::make_move_iterator(shapes.begin()),
                        std::make_move_iterator(shapes.end()));
        }
    }
    return drawn;
}

} // namespace katana::cad::annotation
