#include "katana/cad/style_resolver.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>
#include <type_traits>
#include <utility>
#include <variant>

#include "katana/cad/symbols.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::StyleLibrary;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

// The viewport's plain point mark is four pixels either side of the point
// (kPointMarkerPixels in viewport_widget.cpp); a preview with no symbol size
// of its own draws a built-in shape the same size.
constexpr double kPlainMarkPixels = 4.0;
// Enough chords that the sample's half circle reads as a curve at thumbnail
// size, and few enough that a pattern laid along it is not all corners.
constexpr int kSampleArcChords = 16;

// The built-in shape `name` draws as: itself when it is one of the sixteen,
// else the one its words suggest.
[[nodiscard]] ResolvedSymbol builtInFor(std::string_view name)
{
    ResolvedSymbol resolved;
    if (katana::entity::isBuiltInSymbolName(name)) {
        resolved.kind = SymbolKind::BuiltIn;
        // The name's own storage belongs to the caller; the built-in list's
        // does not move, so the answer points there.
        const auto& names = katana::entity::symbolNames();
        resolved.builtIn = *std::find(names.begin(), names.end(), name);
    } else {
        resolved.kind = SymbolKind::BuiltInFallback;
        resolved.builtIn = katana::entity::builtInSymbolFor(name);
    }
    return resolved;
}

// A built-in shape's strokes as StyleStrokes in the entity's own pen, so one
// painter draws both kinds of symbol.
[[nodiscard]] StyleDrawing builtInDrawing(std::string_view shape, const Point2& at, double size,
                                          double rotation, double fallbackHalfWidth)
{
    // The style's size is the symbol's WIDTH, as 12d's is; the unit shape
    // takes a half-width.
    const double half = size > 0.0 ? 0.5 * size : fallbackHalfWidth;
    StyleDrawing drawing;
    for (Polyline2& stroke : symbolStrokes(shape, at, half, rotation)) {
        drawing.strokes.push_back(StyleStroke{std::move(stroke), {}});
    }
    return drawing;
}

void append(StyleDrawing& into, StyleDrawing&& from)
{
    into.strokes.insert(into.strokes.end(), std::make_move_iterator(from.strokes.begin()),
                        std::make_move_iterator(from.strokes.end()));
    into.texts.insert(into.texts.end(), std::make_move_iterator(from.texts.begin()),
                      std::make_move_iterator(from.texts.end()));
}

} // namespace

ResolvedLinetype resolveLinetype(const katana::entity::Model& model, const StyleLibrary& library,
                                 std::string_view name)
{
    ResolvedLinetype resolved;
    if (name.empty()) {
        return resolved; // no name: nothing to look up, a plain line
    }
    const LineStyle* definition = library.find(name);
    const katana::entity::Linetype* linetype = model.linetypes.find(name);
    resolved.collision = definition != nullptr && linetype != nullptr;
    if (definition != nullptr && !definition->atVertices) {
        resolved.kind = LinetypeKind::LibraryDefinition;
        resolved.definition = definition;
    } else if (linetype != nullptr) {
        resolved.kind = LinetypeKind::ModelLinetype;
        resolved.linetype = linetype;
    }
    return resolved;
}

ResolvedLinetype resolveLinePattern(const katana::entity::Model& model,
                                    const StyleLibrary& library, std::string_view linetype,
                                    std::string_view symbol)
{
    if (!symbol.empty() && linetype == symbol) {
        // The 12da import names a symbol string's style, linetype AND symbol
        // after the symbol. Taken as a linetype, the 147 symbols that are not
        // `mode vertex` were laid along their lines as patterns (D8).
        return {};
    }
    return resolveLinetype(model, library, linetype);
}

ResolvedSymbol resolveSymbol(const StyleLibrary& library, std::string_view name)
{
    if (name.empty()) {
        return {};
    }
    if (const LineStyle* definition = library.find(name); definition != nullptr) {
        ResolvedSymbol resolved;
        resolved.kind = SymbolKind::LibraryDefinition;
        resolved.definition = definition;
        return resolved;
    }
    return builtInFor(name);
}

std::shared_ptr<const FlatDefinition>
DefinitionCache::find(const StyleLibrary& library, std::uint64_t generation, std::string_view name)
{
    if (generation_ != generation) {
        entries_.clear();
        generation_ = generation;
    }
    if (const auto found = entries_.find(name); found != entries_.end()) {
        return found->second;
    }
    std::shared_ptr<const FlatDefinition> flat;
    if (const LineStyle* definition = library.find(name); definition != nullptr) {
        flat = std::make_shared<const FlatDefinition>(flattenDefinition(*definition));
    }
    entries_.emplace(std::string(name), flat);
    return flat;
}

void DefinitionCache::clear()
{
    entries_.clear();
    generation_.reset();
}

StyleDrawing pointSymbolDrawing(const StyleLibrary& library, std::string_view name,
                                const Point2& at, double size, double rotation, double paperScale,
                                double fallbackHalfWidth)
{
    const ResolvedSymbol resolved = resolveSymbol(library, name);
    switch (resolved.kind) {
    case SymbolKind::None:
        return {};
    case SymbolKind::LibraryDefinition:
        return symbolDrawing(*resolved.definition, at, size, rotation, paperScale);
    case SymbolKind::BuiltIn:
    case SymbolKind::BuiltInFallback:
        break;
    }
    return builtInDrawing(resolved.builtIn, at, size, rotation, fallbackHalfWidth);
}

StyleDrawing pointSymbolDrawing(DefinitionCache& cache, const StyleLibrary& library,
                                std::uint64_t generation, std::string_view name, const Point2& at,
                                double size, double rotation, double paperScale,
                                double fallbackHalfWidth)
{
    if (name.empty()) {
        return {};
    }
    if (const auto flat = cache.find(library, generation, name); flat != nullptr) {
        return symbolDrawing(*flat, at, size, rotation, paperScale);
    }
    return builtInDrawing(builtInFor(name).builtIn, at, size, rotation, fallbackHalfWidth);
}

Box2 drawnExtent(const StyleDrawing& drawing)
{
    Box2 box;
    for (const StyleStroke& stroke : drawing.strokes) {
        for (const Point2& point : stroke.path.vertices) {
            box.expand(point);
        }
    }
    for (const StyleTextMark& text : drawing.texts) {
        const double reach =
            std::abs(text.height) * static_cast<double>(std::max<std::size_t>(1, text.text.size()));
        box.expand(Box2{text.at, text.at}.inflated(reach));
    }
    return box;
}

bool belowSymbolDetail(const StyleDrawing& drawing, double viewScale, double minimumPixels)
{
    if (drawing.empty() || !(viewScale > 0.0)) {
        return false;
    }
    const Box2 box = drawnExtent(drawing);
    return std::max(box.width(), box.height()) * viewScale < minimumPixels;
}

std::vector<Point2> symbolVertices(const katana::entity::Geometry& geometry)
{
    return std::visit(
        [](const auto& shape) -> std::vector<Point2> {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, katana::entity::PointGeometry>) {
                return {shape.position};
            } else if constexpr (std::is_same_v<T, katana::geometry::Segment2>) {
                return {shape.start, shape.end};
            } else if constexpr (std::is_same_v<T, katana::geometry::Arc2>) {
                return {shape.startPoint(), shape.endPoint()};
            } else if constexpr (std::is_same_v<T, Polyline2>) {
                return shape.vertices;
            } else {
                return {};
            }
        },
        geometry);
}

StyleSamplePath styleSamplePath(double width)
{
    // The run takes the first 45 % of the width, the corner climbs to the top
    // at 60 %, and the arc dips from there to the bottom of a circle of radius
    // 0.2 w about (0.8 w, 0.25 w) and back up to the right-hand edge.
    const double w = std::max(width, 0.0);
    StyleSamplePath sample;
    const Point2 start(0.0, 0.0);
    const Point2 runEnd(0.45 * w, 0.0);
    const Point2 corner(0.6 * w, 0.25 * w);
    const Point2 centre(0.8 * w, 0.25 * w);
    const double radius = 0.2 * w;
    sample.path.vertices = {start, runEnd, corner};
    for (int i = 1; i <= kSampleArcChords; ++i) {
        // From 180 degrees round through 270 (the bottom) to 360.
        const double angle = std::numbers::pi * (1.0 + static_cast<double>(i) / kSampleArcChords);
        sample.path.vertices.emplace_back(centre.x + radius * std::cos(angle),
                                          centre.y + radius * std::sin(angle));
    }
    sample.vertices = {start, runEnd, corner, sample.path.vertices.back()};
    return sample;
}

StyleDrawing styleSampleDrawing(const katana::entity::Model& model, const StyleLibrary& library,
                                const katana::entity::Style& style, const StyleSamplePath& sample,
                                double paperScale, const DashOptions& dashOptions,
                                double fallbackHalfWidth)
{
    StyleDrawing drawing;
    const Polyline2& path = sample.path;
    const auto solid = [&drawing, &path] {
        if (path.vertices.size() >= 2) {
            drawing.strokes.push_back(StyleStroke{path, {}});
        }
    };
    const ResolvedLinetype pattern =
        resolveLinePattern(model, library, style.linetype, style.symbol);
    switch (pattern.kind) {
    case LinetypeKind::LibraryDefinition: {
        LinestyleOptions options;
        options.paperScale = paperScale;
        LinestyleLayout laid = styleDrawing(flattenDefinition(*pattern.definition), path, options);
        if (laid.outcome == LinestyleLayout::Outcome::Laid) {
            append(drawing, std::move(laid.drawing));
        } else {
            solid(); // the same plain line the viewport draws in its place
        }
        break;
    }
    case LinetypeKind::ModelLinetype: {
        const bool dashed = forEachDash(
            path.vertices, path.closed, *pattern.linetype, dashOptions,
            [&drawing](const Point2& from, const Point2& to) {
                StyleStroke span;
                span.path.vertices = from == to ? std::vector<Point2>{from}
                                                : std::vector<Point2>{from, to};
                drawing.strokes.push_back(std::move(span));
            });
        if (!dashed) {
            solid(); // forEachDash emitted nothing: continuous, too fine or too long
        }
        break;
    }
    case LinetypeKind::Solid:
        solid();
        break;
    }
    if (!style.symbol.empty()) {
        const double half = fallbackHalfWidth > 0.0 ? fallbackHalfWidth
                            : dashOptions.viewScale > 0.0
                                ? kPlainMarkPixels / dashOptions.viewScale
                                : 0.0;
        for (const Point2& vertex : sample.vertices) {
            append(drawing, pointSymbolDrawing(library, style.symbol, vertex, style.symbolSize,
                                               0.0, paperScale, half));
        }
    }
    return drawing;
}

} // namespace katana::cad
