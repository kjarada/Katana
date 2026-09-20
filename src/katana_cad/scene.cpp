#include "katana/cad/scene.hpp"

#include <algorithm>
#include <cmath>
#include <variant>

#include "katana/cad/dashing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/entity/display.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;

using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::AABB;
using katana::render::DrawList;
using katana::render::Rgba;
using katana::render::Vec3;
using katana::render::VertexIndex;

namespace {

constexpr double kTwoPi = 6.283185307179586476925;

// Linear ramp through a small fixed palette. Chosen over a single hue so that
// adjacent contour bands are actually distinguishable, which is the whole
// point of colouring by elevation.
[[nodiscard]] Rgba elevationRamp(double t)
{
    static constexpr int kStops = 5;
    static constexpr std::uint8_t kR[kStops] = {40, 60, 235, 200, 245};
    static constexpr std::uint8_t kG[kStops] = {70, 160, 220, 130, 245};
    static constexpr std::uint8_t kB[kStops] = {140, 90, 130, 70, 245};

    const double clamped = std::clamp(t, 0.0, 1.0) * (kStops - 1);
    const int lower = std::min(static_cast<int>(clamped), kStops - 2);
    const double f = clamped - static_cast<double>(lower);
    const auto mix = [f](std::uint8_t a, std::uint8_t b) {
        return static_cast<std::uint8_t>(static_cast<double>(a) +
                                         (static_cast<double>(b) - static_cast<double>(a)) * f +
                                         0.5);
    };
    return katana::render::rgba(mix(kR[lower], kR[lower + 1]), mix(kG[lower], kG[lower + 1]),
                                mix(kB[lower], kB[lower + 1]));
}

// Green (flat) through yellow to red (steep). 1:1 (45 degrees) is the red end,
// which is where a batter stops being trafficable.
[[nodiscard]] Rgba slopeRamp(double slope)
{
    const double t = std::clamp(slope, 0.0, 1.0);
    const auto channel = [](double v) {
        return static_cast<std::uint8_t>(std::clamp(v, 0.0, 1.0) * 255.0 + 0.5);
    };
    return katana::render::rgba(channel(t * 2.0), channel(2.0 - t * 2.0), channel(0.15));
}

// Through the one resolution chain (entity -> style -> layer), so the 3D view
// agrees with the 2D one. It previously read the entity's own colour or a fixed
// default, which meant an entity on a red layer drew grey - ByLayer is the
// DEFAULT, so that was the common case, not the corner one.
//
// options.defaultColor now only applies where the entity has no layer at all.
[[nodiscard]] Rgba colorOf(const katana::entity::Model& model, const Entity& entity,
                           const SceneOptions& options)
{
    if (model.layers.find(entity.layer) == nullptr && !entity.color.has_value()) {
        return options.defaultColor;
    }
    const auto display = katana::entity::resolveDisplay(model, entity);
    return katana::render::rgba(display.color.r, display.color.g, display.color.b,
                                display.color.a);
}

} // namespace

std::vector<Point2> chordArc(const Arc2& arc, double tolerance)
{
    std::vector<Point2> out;
    if (!(arc.radius > 0.0) || !std::isfinite(arc.sweep) || std::abs(arc.sweep) <= tol::kAngular) {
        if (arc.radius > 0.0) {
            out.push_back(arc.startPoint());
            out.push_back(arc.endPoint());
        }
        return out;
    }

    // Sagitta: for a chord subtending angle phi on radius r the greatest
    // deviation is r * (1 - cos(phi / 2)), so holding that at `tolerance`
    // gives phi = 2 * acos(1 - tolerance / r). A tolerance at or beyond the
    // radius would ask for acos of something <= 0, so it is capped.
    const double ratio = std::clamp(tolerance / arc.radius, 1.0e-12, 0.5);
    const double step = 2.0 * std::acos(1.0 - ratio);
    const double sweep = std::abs(arc.sweep);
    // At least one segment, and capped so that a hairline tolerance on a large
    // radius cannot ask for a million chords.
    const auto count = static_cast<std::size_t>(
        std::clamp(std::ceil(sweep / std::max(step, 1.0e-9)), 1.0, 8192.0));

    out.reserve(count + 1);
    for (std::size_t i = 0; i <= count; ++i) {
        out.push_back(arc.pointAt(static_cast<double>(i) / static_cast<double>(count)));
    }
    return out;
}

std::vector<Point2> chordCircle(const Circle2& circle, double tolerance)
{
    const Arc2 full{circle.center, circle.radius, 0.0, kTwoPi};
    std::vector<Point2> out = chordArc(full, tolerance);
    if (!out.empty()) {
        out.pop_back(); // the closing point duplicates the first
    }
    return out;
}

// ---- surfaces -------------------------------------------------------------------

void SceneBuilder::appendSurfaces(const std::vector<SceneSurface>& surfaces,
                                  const SceneOptions& options, DrawList& out)
{
    const double exaggeration =
        std::isfinite(options.verticalExaggeration) && options.verticalExaggeration > 0.0
            ? options.verticalExaggeration
            : 1.0;
    const double datum = std::isfinite(options.exaggerationDatum) ? options.exaggerationDatum : 0.0;
    const auto liftZ = [exaggeration, datum](double z) {
        return datum + (z - datum) * exaggeration;
    };

    Vec3 light = options.lightDirection;
    const double lightLength = light.length();
    const bool lit = lightLength > 1.0e-9;
    if (lit) {
        light = light / lightLength;
    }
    const double ambient = std::clamp(options.ambient, 0.0, 1.0);

    for (const SceneSurface& item : surfaces) {
        if (!item.visible || item.surface == nullptr || item.surface->empty() ||
            item.style == SurfaceStyle::Hidden) {
            continue;
        }
        const katana::terrain::TinSurface& surface = *item.surface;
        const double low = surface.minElevation();
        const double high = surface.maxElevation();
        const double span = high - low;

        const bool shaded =
            item.style == SurfaceStyle::Shaded || item.style == SurfaceStyle::ShadedWithEdges;
        const bool edges =
            item.style == SurfaceStyle::Wireframe || item.style == SurfaceStyle::ShadedWithEdges;

        // One vertex per TIN vertex, shared by every triangle that uses it:
        // the transform stage then does one matrix multiply per vertex rather
        // than three per triangle, which on a 500k-triangle surface is the
        // difference between 1.5 million and 250 thousand.
        const VertexIndex base = static_cast<VertexIndex>(out.positions.size());
        out.positions.reserve(out.positions.size() + surface.vertexCount());
        out.colors.reserve(out.colors.size() + surface.vertexCount());
        for (const auto& vertex : surface.vertices()) {
            Rgba color = item.flatColor;
            if (item.coloring == SurfaceColoring::Elevation && span > tol::kGeometric) {
                color = elevationRamp((vertex.z - low) / span);
            }
            out.addVertex(Vec3(vertex.x, vertex.y, liftZ(vertex.z)), color);
        }

        if (shaded) {
            out.triangles.reserve(out.triangles.size() + surface.triangleCount());
            for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
                const auto& triangle = surface.triangles()[t];
                const VertexIndex a = base + triangle[0];
                const VertexIndex b = base + triangle[1];
                const VertexIndex c = base + triangle[2];

                if (item.coloring == SurfaceColoring::Slope) {
                    // Per triangle, so the vertices of THIS triangle get their
                    // own copies: slope is a property of the facet, and sharing
                    // vertices would smear it across the surface.
                    const auto slope = surface.triangleSlopeAspect(t);
                    const Rgba color = slopeRamp(slope.has_value() ? slope->slope : 0.0);
                    const VertexIndex a2 = out.addVertex(out.positions[a], color);
                    const VertexIndex b2 = out.addVertex(out.positions[b], color);
                    const VertexIndex c2 = out.addVertex(out.positions[c], color);
                    out.addTriangle(a2, b2, c2);
                    continue;
                }

                if (lit) {
                    // Flat shading baked into a private copy of the three
                    // vertices. Shared vertices cannot carry per-facet light.
                    const Vec3 normal =
                        (out.positions[b] - out.positions[a]).cross(out.positions[c] -
                                                                    out.positions[a]);
                    const double area = normal.length();
                    double intensity = 1.0;
                    if (area > 1.0e-12) {
                        // Absolute value: a surface is lit from whichever side
                        // it is seen, so looking up at a TIN from below does
                        // not show a black sheet.
                        intensity = ambient + (1.0 - ambient) *
                                                  std::abs((normal / area).dot(light));
                    }
                    const VertexIndex a2 =
                        out.addVertex(out.positions[a], katana::render::shade(out.colors[a], intensity));
                    const VertexIndex b2 =
                        out.addVertex(out.positions[b], katana::render::shade(out.colors[b], intensity));
                    const VertexIndex c2 =
                        out.addVertex(out.positions[c], katana::render::shade(out.colors[c], intensity));
                    out.addTriangle(a2, b2, c2);
                    continue;
                }
                out.addTriangle(a, b, c);
            }
        }

        if (edges) {
            const Rgba edgeColor =
                shaded ? katana::render::rgba(40, 40, 45) : katana::render::rgba(190, 190, 190);
            const float bias = shaded ? 3.0e-4f : 0.0f;
            for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
                const auto& triangle = surface.triangles()[t];
                const auto& neighbours = surface.neighbors()[t];
                for (std::size_t e = 0; e < 3; ++e) {
                    // Each interior edge is shared by two triangles; drawing it
                    // from the lower-indexed one only halves the line count.
                    if (neighbours[e] != katana::terrain::kNoTriangle &&
                        neighbours[e] < static_cast<std::uint32_t>(t)) {
                        continue;
                    }
                    const VertexIndex a = base + triangle[e];
                    const VertexIndex b = base + triangle[(e + 1) % 3];
                    // A breakline or boundary edge is drawn heavier: on a
                    // survey surface those are the edges that mean something.
                    const bool constrained = surface.isEdgeConstrained(t, e);
                    if (constrained) {
                        const Rgba strong = katana::render::rgba(250, 210, 120);
                        const VertexIndex a2 = out.addVertex(out.positions[a], strong);
                        const VertexIndex b2 = out.addVertex(out.positions[b], strong);
                        out.addLine(a2, b2, 2.0f, bias * 2.0f);
                    } else {
                        const VertexIndex a2 = out.addVertex(out.positions[a], edgeColor);
                        const VertexIndex b2 = out.addVertex(out.positions[b], edgeColor);
                        out.addLine(a2, b2, 1.0f, bias);
                    }
                }
            }
        }
    }
}

// ---- entities -------------------------------------------------------------------

void SceneBuilder::appendEntities(const Document& document, const SceneOptions& options,
                                  DrawList& out)
{
    const double z = std::isfinite(options.entityElevation) ? options.entityElevation : 0.0;
    const SelectionSet& selection = document.selection();

    // EVERY drawn path goes through this one emitter, including a plain
    // Segment2. The tempting shortcut - a Segment2 adding its DrawLine
    // directly - would have left lines solid in 3D while polylines, arcs and
    // circles dashed: the most confusing possible half-feature, and one no
    // compiler complains about.
    const auto emitPolyline = [&](const std::vector<Point2>& points, bool closed, Rgba color,
                                  float width, const katana::entity::Linetype* linetype) {
        if (points.size() < 2) {
            if (points.size() == 1) {
                const VertexIndex v = out.addVertex(Vec3(points[0].x, points[0].y, z), color);
                out.addPoint(v, options.pointSize, options.entityDepthBias);
            }
            return;
        }

        if (options.drawLinetypes && linetype != nullptr && !linetype->isContinuous()) {
            DashOptions dash;
            dash.patternScale = options.linetypeScale;
            // A 3D view has no single pixels-per-model-unit, so resolvability
            // cannot be judged here; the span budget bounds the work instead.
            dash.viewScale = 1.0;
            dash.minimumElementPixels = 0.0;
            dash.maximumSpans = options.maximumDashSpans;

            // All or nothing: forEachDash emits nothing when the budget would
            // be exceeded, so a long dashed line is drawn whole and solid
            // rather than silently cut short.
            const bool laid = forEachDash(points, closed, *linetype, dash,
                                          [&](const Point2& a, const Point2& b) {
                                              const VertexIndex from =
                                                  out.addVertex(Vec3(a.x, a.y, z), color);
                                              if (a.distanceTo(b) <= 0.0) {
                                                  out.addPoint(from, options.pointSize * 0.6,
                                                               options.entityDepthBias);
                                                  return;
                                              }
                                              const VertexIndex to =
                                                  out.addVertex(Vec3(b.x, b.y, z), color);
                                              out.addLine(from, to, width,
                                                          options.entityDepthBias);
                                          });
            if (laid) {
                return;
            }
        }

        VertexIndex previous = out.addVertex(Vec3(points[0].x, points[0].y, z), color);
        const VertexIndex first = previous;
        for (std::size_t i = 1; i < points.size(); ++i) {
            const VertexIndex current =
                out.addVertex(Vec3(points[i].x, points[i].y, z), color);
            out.addLine(previous, current, width, options.entityDepthBias);
            previous = current;
        }
        if (closed) {
            out.addLine(previous, first, width, options.entityDepthBias);
        }
    };

    document.model().entities.forEach([&](const Entity& entity) {
        if (!entity.visible) {
            return;
        }
        const bool selected = selection.contains(entity.id);
        const Rgba color =
            selected ? options.selectionColor : colorOf(document.model(), entity, options);
        const float width = selected ? options.selectedLineWidth : options.entityLineWidth;
        const auto display = katana::entity::resolveDisplay(document.model(), entity);
        // A selected entity is drawn in the selection style, dashes and all.
        const katana::entity::Linetype* linetype =
            selected ? nullptr : document.model().linetypes.find(display.linetype);

        std::visit(
            [&](const auto& shape) {
                using Shape = std::decay_t<decltype(shape)>;
                if constexpr (std::is_same_v<Shape, katana::entity::PointGeometry>) {
                    const VertexIndex v = out.addVertex(
                        Vec3(shape.position.x, shape.position.y, z), color);
                    out.addPoint(v, options.pointSize, options.entityDepthBias);
                } else if constexpr (std::is_same_v<Shape, Segment2>) {
                    chord_ = {shape.start, shape.end};
                    emitPolyline(chord_, false, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, Arc2>) {
                    chord_ = chordArc(shape, options.chordTolerance);
                    emitPolyline(chord_, false, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, Circle2>) {
                    chord_ = chordCircle(shape, options.chordTolerance);
                    emitPolyline(chord_, true, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, Polyline2>) {
                    emitPolyline(shape.vertices, shape.closed, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, katana::entity::TextGeometry>) {
                    // Text is not rendered in 3D yet: a glyph outline needs a
                    // font, and drawing a marker where the text sits is more
                    // honest than drawing nothing (the 2D view shows the text).
                    const VertexIndex v =
                        out.addVertex(Vec3(shape.position.x, shape.position.y, z), color);
                    out.addPoint(v, options.pointSize, options.entityDepthBias);
                } else if constexpr (std::is_same_v<Shape, katana::entity::DimensionGeometry>) {
                    // Drawn through the same builder the 2D viewport uses, so
                    // the two views cannot disagree about what a dimension
                    // looks like. The text itself is not rendered here - a
                    // glyph outline needs a font, which the 3D path does not
                    // have - so the lines and arrows appear and the label does
                    // not. That is stated rather than left to look like a bug.
                    const auto drawn = buildDimension(shape, resolveDimensionStyle(
                                                                 document.model(), entity));
                    if (!drawn.empty()) {
                        const auto stroke = [&](const katana::geometry::Segment2& segment) {
                            out.addSegment(Vec3(segment.start.x, segment.start.y, z),
                                           Vec3(segment.end.x, segment.end.y, z), color, width,
                                           options.entityDepthBias);
                        };
                        for (const auto& segment : drawn.extensionLines) {
                            stroke(segment);
                        }
                        stroke(drawn.dimensionLine);
                        for (const auto& segment : drawn.arrowStrokes) {
                            stroke(segment);
                        }
                        for (const auto& fill : drawn.arrowFills) {
                            // Outlined rather than filled: DrawList fills only
                            // triangles, and an arrowhead is small enough that
                            // an outline reads correctly.
                            for (std::size_t i = 0; i < fill.size(); ++i) {
                                const auto& from = fill[i];
                                const auto& to = fill[(i + 1) % fill.size()];
                                out.addSegment(Vec3(from.x, from.y, z), Vec3(to.x, to.y, z),
                                               color, width, options.entityDepthBias);
                            }
                        }
                    }
                } else {
                    static_assert(false, "appendEntities has no case for this geometry kind");
                }
            },
            entity.geometry);
    });
}

// ---- grid -----------------------------------------------------------------------

void SceneBuilder::appendGrid(const SceneOptions& options, const AABB& around, DrawList& out)
{
    if (!options.drawGrid || !(options.gridSpacing > 0.0) || !std::isfinite(options.gridSpacing)) {
        return;
    }
    const int lines = std::clamp(options.gridLines, 1, 400);
    const double spacing = options.gridSpacing;
    const double z = std::isfinite(options.entityElevation) ? options.entityElevation : 0.0;

    // Snapped to the grid so it does not slide about under a pan.
    const Vec3 centre = around.empty() ? Vec3{} : around.center();
    const double cx = std::round(centre.x / spacing) * spacing;
    const double cy = std::round(centre.y / spacing) * spacing;
    const double reach = spacing * static_cast<double>(lines);

    for (int i = -lines; i <= lines; ++i) {
        const double offset = static_cast<double>(i) * spacing;
        const bool axis = i == 0;
        const Rgba colorX = axis ? options.axisColorY : options.gridColor;
        const Rgba colorY = axis ? options.axisColorX : options.gridColor;
        out.addSegment(Vec3(cx + offset, cy - reach, z), Vec3(cx + offset, cy + reach, z), colorX,
                       axis ? 2.0f : 1.0f);
        out.addSegment(Vec3(cx - reach, cy + offset, z), Vec3(cx + reach, cy + offset, z), colorY,
                       axis ? 2.0f : 1.0f);
    }
}

void SceneBuilder::build(const Document& document, const std::vector<SceneSurface>& surfaces,
                         const SceneOptions& options, DrawList& out)
{
    out.clear();
    if (options.drawGrid) {
        appendGrid(options, sceneBounds(document, surfaces, options), out);
    }
    appendSurfaces(surfaces, options, out);
    if (options.drawEntities) {
        appendEntities(document, options, out);
    }
}

AABB sceneBounds(const Document& document, const std::vector<SceneSurface>& surfaces,
                 const SceneOptions& options)
{
    const double exaggeration =
        std::isfinite(options.verticalExaggeration) && options.verticalExaggeration > 0.0
            ? options.verticalExaggeration
            : 1.0;
    const double datum = std::isfinite(options.exaggerationDatum) ? options.exaggerationDatum : 0.0;

    AABB box;
    for (const SceneSurface& item : surfaces) {
        if (!item.visible || item.surface == nullptr || item.surface->empty() ||
            item.style == SurfaceStyle::Hidden) {
            continue;
        }
        const auto& plan = item.surface->bounds();
        if (plan.empty()) {
            continue;
        }
        box.expand(Vec3(plan.min.x, plan.min.y,
                        datum + (item.surface->minElevation() - datum) * exaggeration));
        box.expand(Vec3(plan.max.x, plan.max.y,
                        datum + (item.surface->maxElevation() - datum) * exaggeration));
    }
    if (options.drawEntities) {
        const auto plan = document.model().entities.bounds();
        if (!plan.empty()) {
            const double z = std::isfinite(options.entityElevation) ? options.entityElevation : 0.0;
            box.expand(Vec3(plan.min.x, plan.min.y, z));
            box.expand(Vec3(plan.max.x, plan.max.y, z));
        }
    }
    return box;
}

} // namespace katana::cad
