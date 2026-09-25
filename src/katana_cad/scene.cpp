#include "katana/cad/scene.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <limits>
#include <optional>
#include <type_traits>
#include <variant>

#include "katana/cad/dashing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/core/cpu_features.hpp"
#include "katana/math/numerics.hpp"

#include "simd/scene_kernels.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;

using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::chordArc;
using katana::geometry::chordCircle;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::AABB;
using katana::render::DrawList;
using katana::render::Rgba;
using katana::render::Vec3;
using katana::render::VertexIndex;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// The kernels read these arrays as arrays of doubles and of indices.
static_assert(std::is_standard_layout_v<Vec3> && sizeof(Vec3) == 3 * sizeof(double) &&
              offsetof(Vec3, x) == 0 && offsetof(Vec3, z) == 2 * sizeof(double));
static_assert(sizeof(katana::terrain::TinTriangle) == 3 * sizeof(std::uint32_t));
static_assert(sizeof(std::array<std::uint32_t, 3>) == 3 * sizeof(std::uint32_t));

// The elevation ramp's palette (elevationRampColor), shared with the kernels'
// parameters so there is one copy of it.
constexpr int kRampStops = 5;
constexpr std::uint8_t kRampR[kRampStops] = {40, 60, 235, 200, 245};
constexpr std::uint8_t kRampG[kRampStops] = {70, 160, 220, 130, 245};
constexpr std::uint8_t kRampB[kRampStops] = {140, 90, 130, 70, 245};
static_assert(kRampStops - 1 == static_cast<int>(simd::kRampSegments));

// Room for `extra` more, growing geometrically: a reserve of exactly what
// one surface or mesh adds would copy the list once per surface or mesh.
template <typename T> void grow(std::vector<T>& list, std::size_t extra)
{
    if (list.capacity() - list.size() < extra) {
        list.reserve(std::max(list.size() + extra, 2 * list.capacity()));
    }
}

#if defined(KATANA_HAVE_AVX2_KERNELS)
[[nodiscard]] bool avx2Active()
{
    return katana::core::activeSimdLevel() == katana::core::SimdLevel::Avx2;
}

// Below these the scalar loop is as quick as the kernel with its setup, so
// small surfaces, meshes and fades take it at every level. Measured with the
// thresholds at 1 (BM_SceneKernelBreakEven*, BM_SceneFadeEdges/1-4 in
// benchmarks/bench_scene.cpp; docs/performance.md): a surface of 4 to 25
// vertices was within the noise either way, a mesh of 2 faces and a fade of
// 10 edge vertices already 2.5x and 3x quicker by kernel.
constexpr std::size_t kSurfaceKernelMinimum = 16; // vertices
constexpr std::size_t kMeshKernelMinimum = 2;     // faces
constexpr std::size_t kFadeKernelMinimum = 8;     // edge vertices
#endif

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

[[nodiscard]] Rgba mix(Rgba a, Rgba b, double t)
{
    const auto channel = [t](std::uint8_t lo, std::uint8_t hi) {
        return static_cast<std::uint8_t>(static_cast<double>(lo) +
                                         (static_cast<double>(hi) - static_cast<double>(lo)) * t +
                                         0.5);
    };
    return katana::render::rgba(channel(katana::render::redOf(a), katana::render::redOf(b)),
                                channel(katana::render::greenOf(a), katana::render::greenOf(b)),
                                channel(katana::render::blueOf(a), katana::render::blueOf(b)),
                                channel(katana::render::alphaOf(a), katana::render::alphaOf(b)));
}

// Through the one resolution chain (entity -> style -> layer), so the 3D view
// agrees with the 2D one. options.defaultColor only applies where the entity
// has no layer at all.
[[nodiscard]] Rgba colorOf(const katana::entity::Model& model, const Entity& entity,
                           const katana::entity::ResolvedDisplay& display,
                           const SceneOptions& options)
{
    if (model.layers.find(entity.layer) == nullptr && !entity.color.has_value()) {
        return options.defaultColor;
    }
    return katana::render::rgba(display.color.r, display.color.g, display.color.b,
                                display.color.a);
}

// The view a scene is built for: null options.layers means the document rule.
[[nodiscard]] const LayerOverrides& viewOf(const SceneOptions& options)
{
    return options.layers != nullptr ? *options.layers : kNoLayerOverrides;
}

// Vertical exaggeration about its datum.
struct Lift {
    double factor = 1.0;
    double datum = 0.0;
    [[nodiscard]] double operator()(double z) const { return datum + (z - datum) * factor; }
};

[[nodiscard]] Lift liftOf(const SceneOptions& options)
{
    Lift lift;
    lift.factor = std::isfinite(options.verticalExaggeration) && options.verticalExaggeration > 0.0
                      ? options.verticalExaggeration
                      : 1.0;
    lift.datum = std::isfinite(options.exaggerationDatum) ? options.exaggerationDatum : 0.0;
    return lift;
}

// The hillshade of SceneOptions, for a UNIT normal.
struct Light {
    Vec3 towardsSun{0.0, 0.0, 1.0};
    bool on = false;
    double sky = 1.0;
    double ground = 1.0;
    double sun = 0.0;

    [[nodiscard]] double intensity(const Vec3& normal) const
    {
        if (!on) {
            return 1.0;
        }
        const double hemisphere = ground + (sky - ground) * (0.5 + 0.5 * normal.z);
        return std::clamp(hemisphere + sun * std::max(0.0, normal.dot(towardsSun)), 0.0, 1.0);
    }
};

[[nodiscard]] Light lightOf(const SceneOptions& options)
{
    Light light;
    const double length = options.lightDirection.length();
    light.on = length > 1.0e-9 && std::isfinite(length);
    if (light.on) {
        light.towardsSun = options.lightDirection / length;
    }
    light.sky = std::clamp(options.skyAmbient, 0.0, 1.0);
    light.ground = std::clamp(options.groundAmbient, 0.0, 1.0);
    light.sun = std::clamp(options.sunStrength, 0.0, 1.0);
    return light;
}

[[nodiscard]] bool drawable(const SceneSurface& item)
{
    return item.visible && item.surface != nullptr && !item.surface->empty() &&
           item.style != SurfaceStyle::Hidden;
}

[[nodiscard]] bool drawable(const SceneMesh& item)
{
    return item.visible && item.mesh != nullptr && !item.mesh->empty() &&
           item.style != SurfaceStyle::Hidden;
}

[[nodiscard]] SurfaceStyle styleOf(const SceneSurface& item)
{
    if (item.style != SurfaceStyle::Automatic) {
        return item.style;
    }
    return item.surface->triangleCount() > kDenseSurfaceTriangles ? SurfaceStyle::Shaded
                                                                  : SurfaceStyle::ShadedWithEdges;
}

struct Ramp {
    double low = 0.0;
    double high = -1.0;
    [[nodiscard]] Rgba at(double z) const
    {
        const double span = high - low;
        return elevationRampColor(span > tol::kGeometric ? (z - low) / span : 0.5);
    }
};

// Lowest and highest of every visible surface coloured by elevation.
[[nodiscard]] Ramp rampOf(const std::vector<SceneSurface>& surfaces)
{
    Ramp ramp;
    ramp.low = std::numeric_limits<double>::infinity();
    ramp.high = -std::numeric_limits<double>::infinity();
    for (const SceneSurface& item : surfaces) {
        if (drawable(item) && item.coloring == SurfaceColoring::Elevation) {
            ramp.low = std::min(ramp.low, item.surface->minElevation());
            ramp.high = std::max(ramp.high, item.surface->maxElevation());
        }
    }
    if (!(ramp.low <= ramp.high)) {
        ramp = Ramp{};
    }
    return ramp;
}

// Lowest visible surface or mesh, unexaggerated; nullopt when there is none.
[[nodiscard]] std::optional<double> terrainFloor(const std::vector<SceneSurface>& surfaces,
                                                 const std::vector<SceneMesh>& meshes)
{
    std::optional<double> lowest;
    const auto take = [&lowest](double z) {
        if (std::isfinite(z)) {
            lowest = lowest ? std::min(*lowest, z) : z;
        }
    };
    for (const SceneSurface& item : surfaces) {
        if (drawable(item)) {
            take(item.surface->minElevation());
        }
    }
    for (const SceneMesh& item : meshes) {
        if (drawable(item)) {
            const AABB space = item.mesh->bounds();
            if (!space.empty()) {
                take(space.min.z);
            }
        }
    }
    return lowest;
}

// ---- draping ---------------------------------------------------------------------

// The visible surfaces linework can be draped on, with their plan boxes so a
// point outside one costs a comparison rather than a locate.
class Drape {
  public:
    explicit Drape(const std::vector<SceneSurface>& surfaces)
    {
        for (const SceneSurface& item : surfaces) {
            if (drawable(item)) {
                surfaces_.push_back(item.surface);
            }
        }
    }

    [[nodiscard]] bool empty() const { return surfaces_.empty(); }

    // The TOP surface at p - the one seen from above - and its elevation.
    [[nodiscard]] std::optional<std::pair<double, std::size_t>> at(const Point2& p) const
    {
        std::optional<std::pair<double, std::size_t>> best;
        for (std::size_t k = 0; k < surfaces_.size(); ++k) {
            if (!surfaces_[k]->bounds().contains(p)) {
                continue;
            }
            const auto z = surfaces_[k]->elevationAt(p);
            if (z && (!best || *z > best->first)) {
                best = std::make_pair(*z, k);
            }
        }
        return best;
    }

    // Appends, in order, where the segment a-b crosses an edge of surface k,
    // with the surface's elevation there: the vertices that make a straight
    // chord lie ON a TIN rather than cutting through its ridges and valleys.
    // Walks triangle to triangle through the neighbour table, so it costs one
    // step per edge crossed; it stops where the surface does.
    void crossings(std::size_t k, const Point2& a, const Point2& b,
                   std::vector<katana::geometry::Point3>& out) const
    {
        const katana::terrain::TinSurface& surface = *surfaces_[k];
        const auto start = surface.locate(a);
        if (!start) {
            return;
        }
        const auto& vertices = surface.vertices();
        const double dx = b.x - a.x;
        const double dy = b.y - a.y;
        std::uint32_t triangle = start->triangle;
        double reached = 0.0;
        // A bound, not an expectation: one step per triangle crossed.
        for (std::size_t step = 0; step < surface.triangleCount(); ++step) {
            const auto& corners = surface.triangles()[triangle];
            double exit = std::numeric_limits<double>::infinity();
            int exitEdge = -1;
            for (int e = 0; e < 3; ++e) {
                const auto& p = vertices[corners[static_cast<std::size_t>(e)]];
                const auto& q = vertices[corners[static_cast<std::size_t>((e + 1) % 3)]];
                const double ex = q.x - p.x;
                const double ey = q.y - p.y;
                // The direction against the edge's outward normal (ey, -ex)
                // of a counter-clockwise triangle: > 0 is on the way out.
                const double outward = dx * ey - dy * ex;
                if (!(outward > 0.0)) {
                    continue;
                }
                const double s = ((p.x - a.x) * ey - (p.y - a.y) * ex) / outward;
                if (s < exit) {
                    exit = s;
                    exitEdge = e;
                }
            }
            // b is in this triangle, or the walk is not getting anywhere
            // (a degenerate triangle): the chord from here is on the surface.
            if (exitEdge < 0 || !(exit < 1.0) || exit < reached - 1.0e-9) {
                return;
            }
            const auto& p = vertices[corners[static_cast<std::size_t>(exitEdge)]];
            const auto& q = vertices[corners[static_cast<std::size_t>((exitEdge + 1) % 3)]];
            const double cx = a.x + dx * exit;
            const double cy = a.y + dy * exit;
            if (exit > reached + 1.0e-12) {
                // On the edge, so the height is the edge's: the same from the
                // triangles either side of it.
                const double ex = q.x - p.x;
                const double ey = q.y - p.y;
                const double length2 = ex * ex + ey * ey;
                const double u =
                    length2 > 0.0 ? std::clamp(((cx - p.x) * ex + (cy - p.y) * ey) / length2, 0.0, 1.0)
                                  : 0.0;
                out.emplace_back(cx, cy, p.z + (q.z - p.z) * u);
                reached = exit;
            }
            const std::uint32_t next = surface.neighbors()[triangle][static_cast<std::size_t>(exitEdge)];
            if (next == katana::terrain::kNoTriangle) {
                return; // off the edge of the surface
            }
            triangle = next;
        }
    }

  private:
    std::vector<const katana::terrain::TinSurface*> surfaces_;
};

// Puts drawing vertices in z by the options' rule (LineworkHeights). A vertex
// whose z is left NaN goes on the datum, which is known only once every entity
// has been seen when there is no terrain; the caller fills those in.
class Heights {
  public:
    Heights(const std::vector<SceneSurface>& surfaces, const SceneOptions& options)
        : drape_(surfaces), mode_(options.linework), lift_(liftOf(options))
    {
    }

    [[nodiscard]] bool usesOwnHeights() const
    {
        return mode_ == LineworkHeights::Heights || mode_ == LineworkHeights::HeightsThenDrape;
    }
    [[nodiscard]] bool drapes() const
    {
        return !drape_.empty() &&
               (mode_ == LineworkHeights::Drape || mode_ == LineworkHeights::HeightsThenDrape);
    }

    // A path of plan points with their own heights (nullopt where a vertex has
    // none) as 3D points, draped and densified where the rule says so.
    void path(const std::vector<Point2>& points, const std::vector<std::optional<double>>& own,
              bool closed, std::vector<katana::geometry::Point3>& out, double* lowest) const
    {
        out.clear();
        const std::size_t count = points.size();
        std::optional<std::size_t> previousSurface;
        for (std::size_t i = 0; i <= count; ++i) {
            if (i == count && !(closed && count > 2)) {
                break;
            }
            const std::size_t index = i % count;
            const Point2& p = points[index];
            const std::optional<double> height =
                usesOwnHeights() && index < own.size() ? own[index] : std::nullopt;
            std::optional<std::size_t> surface;
            double z = kNaN;
            if (height && std::isfinite(*height)) {
                z = lift_(*height);
                if (lowest != nullptr) {
                    *lowest = std::min(*lowest, *height);
                }
            } else if (drapes()) {
                if (const auto top = drape_.at(p)) {
                    z = lift_(top->first);
                    surface = top->second;
                }
            }
            // Densify a chord whose two ends are both draped on one surface.
            if (i > 0 && surface && previousSurface && *surface == *previousSurface) {
                scratch_.clear();
                drape_.crossings(*surface, points[(i - 1) % count], p, scratch_);
                for (const auto& crossing : scratch_) {
                    out.emplace_back(crossing.x, crossing.y, lift_(crossing.z));
                }
            }
            out.emplace_back(p.x, p.y, z);
            previousSurface = surface;
        }
    }

    // One point: its own height, the surface under it, or NaN for the datum.
    [[nodiscard]] double at(const Point2& p, std::optional<double> own, double* lowest) const
    {
        if (usesOwnHeights() && own && std::isfinite(*own)) {
            if (lowest != nullptr) {
                *lowest = std::min(*lowest, *own);
            }
            return lift_(*own);
        }
        if (drapes()) {
            if (const auto top = drape_.at(p)) {
                return lift_(top->first);
            }
        }
        return kNaN;
    }

  private:
    Drape drape_;
    LineworkHeights mode_;
    Lift lift_;
    mutable std::vector<katana::geometry::Point3> scratch_;
};

// A 1-2-5 step at least `wanted`: 1, 2, 5, 10, 20, 50...
[[nodiscard]] double niceStep(double wanted)
{
    if (!(wanted > 0.0) || !std::isfinite(wanted)) {
        return 10.0;
    }
    const double decade = std::pow(10.0, std::floor(std::log10(wanted)));
    for (const double m : {1.0, 2.0, 5.0, 10.0}) {
        if (m * decade >= wanted * (1.0 - 1.0e-12)) {
            return m * decade;
        }
    }
    return 10.0 * decade;
}

} // namespace

Rgba elevationRampColor(double t)
{
    // Linear ramp through a small fixed palette. Chosen over a single hue so
    // that adjacent contour bands are actually distinguishable, which is the
    // whole point of colouring by elevation.
    static constexpr int kStops = kRampStops;
    static constexpr const std::uint8_t* kR = kRampR;
    static constexpr const std::uint8_t* kG = kRampG;
    static constexpr const std::uint8_t* kB = kRampB;

    const double clamped = std::clamp(std::isfinite(t) ? t : 0.0, 0.0, 1.0) * (kStops - 1);
    const int lower = std::min(static_cast<int>(clamped), kStops - 2);
    const double f = clamped - static_cast<double>(lower);
    const auto blend = [f](std::uint8_t a, std::uint8_t b) {
        return static_cast<std::uint8_t>(static_cast<double>(a) +
                                         (static_cast<double>(b) - static_cast<double>(a)) * f +
                                         0.5);
    };
    return katana::render::rgba(blend(kR[lower], kR[lower + 1]), blend(kG[lower], kG[lower + 1]),
                                blend(kB[lower], kB[lower + 1]));
}

// ---- surfaces -------------------------------------------------------------------

namespace {

// What the kernels are handed: the scalar code's constants, laid out as
// the kParam* indices of simd/scene_kernels.hpp say.
[[nodiscard]] std::array<double, simd::kParamCount> kernelParams(const Lift& lift,
                                                                 const Light& light,
                                                                 const Ramp& ramp)
{
    std::array<double, simd::kParamCount> params{};
    params[simd::kParamLiftFactor] = lift.factor;
    params[simd::kParamLiftDatum] = lift.datum;
    params[simd::kParamRampLow] = ramp.low;
    params[simd::kParamRampSpan] = ramp.high - ramp.low;
    params[simd::kParamLightOn] = light.on ? 1.0 : 0.0;
    params[simd::kParamSunX] = light.towardsSun.x;
    params[simd::kParamSunY] = light.towardsSun.y;
    params[simd::kParamSunZ] = light.towardsSun.z;
    params[simd::kParamGround] = light.ground;
    params[simd::kParamSkyMinusGround] = light.sky - light.ground;
    params[simd::kParamSun] = light.sun;
    const std::uint8_t* const palette[3] = {kRampR, kRampG, kRampB};
    for (std::size_t channel = 0; channel < 3; ++channel) {
        for (std::size_t i = 0; i < simd::kRampSegments; ++i) {
            const double stop = static_cast<double>(palette[channel][i]);
            params[simd::kParamRampTable + 2 * channel * simd::kRampSegments + i] = stop;
            params[simd::kParamRampTable + (2 * channel + 1) * simd::kRampSegments + i] =
                static_cast<double>(palette[channel][i + 1]) - stop;
        }
    }
    return params;
}

// The scratch emitSurface reuses across surfaces and builds.
struct SurfaceScratch {
    std::vector<Vec3>& normals;
    std::vector<double>& lifted;
    std::vector<double>& summed;
};

// One surface into `out`, and its fading edges into `edges` (or into `out` at
// full strength when `edges` is null). Shared by appendSurfaces and
// buildTerrain.
void emitSurface(const SceneSurface& item, const SceneOptions& options, const Ramp& ramp,
                 SurfaceScratch scratch, DrawList& out, SceneLayers* edges)
{
    const katana::terrain::TinSurface& surface = *item.surface;
    const Lift lift = liftOf(options);
    const Light light = lightOf(options);
    const SurfaceStyle style = styleOf(item);
    const bool shaded = style == SurfaceStyle::Shaded || style == SurfaceStyle::ShadedWithEdges;
    const bool wire = style == SurfaceStyle::Wireframe;
    const bool fading = style == SurfaceStyle::ShadedWithEdges;
    const bool slope = item.coloring == SurfaceColoring::Slope;
    const auto& vertices = surface.vertices();
    const auto& triangles = surface.triangles();
    const Rgba wireColor = katana::render::rgba(190, 190, 190);
    const float scale = std::max(options.pixelScale, 0.1f);

    // Per-vertex normals for smooth hillshade: the area-weighted sum of the
    // faces round the vertex, each turned to face UP. A TIN is a height field,
    // so up is its outside; the old abs() did the same for the light but let a
    // face turned AWAY from the sun look as lit as one facing it.
    const bool smooth = shaded && light.on && !slope;
    const VertexIndex base = static_cast<VertexIndex>(out.positions.size());
    const std::size_t count = vertices.size();
    // Sized once and written in place: resize grows the arrays geometrically
    // where a reserve per surface would copy them once per surface.
    out.positions.resize(base + count);
    out.colors.resize(base + count);
    Vec3* const positions = out.positions.data() + base;
    Rgba* const colors = out.colors.data() + base;
#if defined(KATANA_HAVE_AVX2_KERNELS)
    if (count >= kSurfaceKernelMinimum && avx2Active()) {
        const auto params = kernelParams(lift, light, ramp);
        scratch.lifted.resize(4 * count);
        katana_avx2_scene_lift(&vertices.data()->x, count, params.data(), scratch.lifted.data());
        if (smooth) {
            scratch.summed.assign(4 * count, 0.0);
            katana_avx2_scene_surface_normals(scratch.lifted.data(), triangles.data()->data(),
                                              triangles.size(), scratch.summed.data());
        }
        // The ramp's degenerate case (one elevation) is one colour: flat.
        const bool ramped = !wire && item.coloring == SurfaceColoring::Elevation &&
                            ramp.high - ramp.low > tol::kGeometric;
        const Rgba flat = wire ? wireColor
                               : (item.coloring == SurfaceColoring::Elevation ? ramp.at(ramp.low)
                                                                              : item.flatColor);
        katana_avx2_scene_surface_vertices(scratch.lifted.data(), count,
                                           smooth ? scratch.summed.data() : nullptr,
                                           params.data(), flat, ramped ? 1 : 0, &positions->x,
                                           colors);
    } else
#endif
    {
        // The scalar reference, which the kernels equal to the bit.
        //
        // Per-vertex normals for smooth hillshade: the area-weighted sum of
        // the faces round the vertex, each turned to face UP. A TIN is a
        // height field, so up is its outside; the old abs() did the same for
        // the light but let a face turned AWAY from the sun look as lit as one
        // facing it.
        std::vector<Vec3>& normals = scratch.normals;
        if (smooth) {
            normals.assign(count, Vec3(0.0, 0.0, 0.0));
            for (const auto& t : triangles) {
                const auto& a = vertices[t[0]];
                const auto& b = vertices[t[1]];
                const auto& c = vertices[t[2]];
                const Vec3 pa(a.x, a.y, lift(a.z));
                Vec3 n = (Vec3(b.x, b.y, lift(b.z)) - pa).cross(Vec3(c.x, c.y, lift(c.z)) - pa);
                if (n.z < 0.0) {
                    n = n * -1.0;
                }
                normals[t[0]] = normals[t[0]] + n;
                normals[t[1]] = normals[t[1]] + n;
                normals[t[2]] = normals[t[2]] + n;
            }
        }
        // One vertex per TIN vertex, shared by every triangle that uses it:
        // one matrix multiply per vertex rather than three per triangle.
        // Lighting per vertex is what makes the sharing possible; it used to
        // be per facet, on private copies, six times the vertices.
        for (std::size_t i = 0; i < count; ++i) {
            const auto& vertex = vertices[i];
            Rgba color = item.flatColor;
            if (wire) {
                color = wireColor;
            } else if (item.coloring == SurfaceColoring::Elevation) {
                color = ramp.at(vertex.z);
            }
            if (smooth) {
                const double length = normals[i].length();
                if (length > 0.0) {
                    color = katana::render::shade(color, light.intensity(normals[i] / length));
                }
            }
            positions[i] = Vec3(vertex.x, vertex.y, lift(vertex.z));
            colors[i] = color;
        }
    }

    if (shaded && !slope) {
        const std::size_t first = out.triangles.size();
        out.triangles.resize(first + triangles.size());
        katana::render::DrawTriangle* const shared = out.triangles.data() + first;
        for (std::size_t t = 0; t < triangles.size(); ++t) {
            shared[t] = {base + triangles[t][0], base + triangles[t][1], base + triangles[t][2]};
        }
    } else if (shaded) {
        grow(out.triangles, triangles.size());
        grow(out.positions, 3 * triangles.size());
        grow(out.colors, 3 * triangles.size());
        for (std::size_t t = 0; t < triangles.size(); ++t) {
            const VertexIndex a = base + triangles[t][0];
            const VertexIndex b = base + triangles[t][1];
            const VertexIndex c = base + triangles[t][2];
            // Slope is a property of the FACET: private copies, or sharing
            // would smear it across the surface. Lit by the facet normal.
            const auto aspect = surface.triangleSlopeAspect(t);
            Rgba color = slopeRamp(aspect.has_value() ? aspect->slope : 0.0);
            Vec3 n = (out.positions[b] - out.positions[a]).cross(out.positions[c] - out.positions[a]);
            if (n.z < 0.0) {
                n = n * -1.0;
            }
            const double length = n.length();
            if (length > 1.0e-12) {
                color = katana::render::shade(color, light.intensity(n / length));
            }
            const VertexIndex a2 = out.addVertex(out.positions[a], color);
            const VertexIndex b2 = out.addVertex(out.positions[b], color);
            const VertexIndex c2 = out.addVertex(out.positions[c], color);
            out.addTriangle(a2, b2, c2);
        }
    }

    if (!wire && !fading) {
        return;
    }
    // Faded edges go to their own list; the rest into `out`.
    DrawList& target = fading && edges != nullptr ? edges->edges : out;
    const VertexIndex runStart = static_cast<VertexIndex>(target.positions.size());
    const auto& neighbours = surface.neighbors();
    const auto drawnHere = [&neighbours](std::size_t t, std::size_t e) {
        return neighbours[t][e] == katana::terrain::kNoTriangle ||
               neighbours[t][e] >= static_cast<std::uint32_t>(t);
    };
    // Reserved once for the lot: these lists hold every surface's edges, and
    // growing them edge by edge cost more than drawing them.
    std::size_t drawn = 0;
    for (std::size_t t = 0; t < triangles.size(); ++t) {
        for (std::size_t e = 0; e < 3; ++e) {
            drawn += drawnHere(t, e) ? 1 : 0;
        }
    }
    grow(target.lines, drawn);
    grow(target.positions, 2 * drawn);
    grow(target.colors, 2 * drawn);
    if (fading && edges != nullptr) {
        grow(edges->edgeBase, 2 * drawn);
        grow(edges->edgeInk, 2 * drawn);
    }
    // An ordinary edge's ink at each vertex: the same shade() of the same
    // colour for every edge meeting there, so worked out once.
    std::vector<Rgba> ink;
    if (!wire) {
        ink.resize(count);
        for (std::size_t i = 0; i < count; ++i) {
            ink[i] = katana::render::shade(out.colors[base + i], 0.55); // not colors: grown since
        }
    }
    for (std::size_t t = 0; t < triangles.size(); ++t) {
        for (std::size_t e = 0; e < 3; ++e) {
            // Each interior edge is shared by two triangles; drawing it from
            // the lower-indexed one only halves the line count.
            if (!drawnHere(t, e)) {
                continue;
            }
            const VertexIndex a = base + triangles[t][e];
            const VertexIndex b = base + triangles[t][(e + 1) % 3];
            // A breakline or boundary edge is drawn heavier: on a survey
            // surface those are the edges that mean something.
            const bool constrained = surface.isEdgeConstrained(t, e);
            if (wire && !constrained) {
                target.lines.push_back({a, b, scale, 0.0f}); // the surface's own grey vertices
                continue;
            }
            const float width = (constrained ? 2.0f : 1.0f) * scale;
            const float bias = wire ? 0.0f : options.edgeDepthBias;
            const Rgba strong = katana::render::rgba(250, 210, 120);
            // An ordinary edge is a darker shade of the surface under it, so
            // it fades INTO the surface rather than into black.
            const Rgba inkA = constrained ? strong : ink[a - base];
            const Rgba inkB = constrained ? strong : ink[b - base];
            const VertexIndex a2 = static_cast<VertexIndex>(target.positions.size());
            target.positions.push_back(out.positions[a]);
            target.positions.push_back(out.positions[b]);
            target.colors.push_back(inkA);
            target.colors.push_back(inkB);
            target.lines.push_back({a2, a2 + 1, width, bias});
            if (fading && edges != nullptr) {
                edges->edgeBase.push_back(out.colors[a]);
                edges->edgeBase.push_back(out.colors[b]);
                edges->edgeInk.push_back(inkA);
                edges->edgeInk.push_back(inkB);
            }
        }
    }
    if (fading && edges != nullptr) {
        SceneLayers::EdgeRun run;
        run.first = runStart;
        run.count = static_cast<VertexIndex>(target.positions.size()) - runStart;
        // The side of an equilateral triangle of the mean plan area: what a
        // typical edge is, whatever the TIN's regularity.
        const double meanArea = surface.planArea() / static_cast<double>(surface.triangleCount());
        run.typicalEdge = std::sqrt(std::max(meanArea, 0.0) * 4.0 / 1.7320508075688772);
        run.applied = 1.0f; // built at full strength (the ink)
        edges->edgeRuns.push_back(run);
    }
}

} // namespace

void SceneBuilder::appendSurfaces(const std::vector<SceneSurface>& surfaces,
                                  const SceneOptions& options, DrawList& out)
{
    const Ramp ramp = rampOf(surfaces);
    for (const SceneSurface& item : surfaces) {
        if (drawable(item)) {
            emitSurface(item, options, ramp, {normals_, lifted_, summed_}, out, nullptr);
        }
    }
}

// A mesh is drawn like a surface, with two differences that follow from what
// it is: no elevation ramp (a closed mesh has no "the height here"), and its
// edges are drawn from every face rather than once per shared edge, because a
// TriangleMesh carries no neighbour table.
//
// Lit per FACE by the face's own winding, and never abs(): a wall turned away
// from the sun is darker than one facing it, and a face turned down is lit
// only by the ground's ambient. That is what makes a mesh's shape readable.
void SceneBuilder::appendMeshes(const std::vector<SceneMesh>& meshes, const SceneOptions& options,
                                DrawList& out)
{
    const Lift lift = liftOf(options);
    const Light light = lightOf(options);
    const float scale = std::max(options.pixelScale, 0.1f);

    // Room for every mesh at once: a trimesh archive is hundreds of small
    // meshes, and growing the lists face by face was most of their cost.
    std::size_t vertexCount = 0;
    std::size_t triangleCount = 0;
    for (const SceneMesh& item : meshes) {
        if (drawable(item)) {
            vertexCount += 6 * item.mesh->triangleCount(); // shaded and edge copies at most
            triangleCount += item.mesh->triangleCount();
        }
    }
    grow(out.positions, vertexCount);
    grow(out.colors, vertexCount);
    grow(out.triangles, triangleCount);
#if defined(KATANA_HAVE_AVX2_KERNELS)
    const bool kernels = avx2Active();
    const auto params = kernelParams(lift, light, Ramp{});
#endif

    for (const SceneMesh& item : meshes) {
        if (!drawable(item)) {
            continue;
        }
        const katana::geometry::TriangleMesh& mesh = *item.mesh;
        const bool shaded =
            item.style == SurfaceStyle::Shaded || item.style == SurfaceStyle::ShadedWithEdges ||
            item.style == SurfaceStyle::Automatic;
        const bool edges =
            item.style == SurfaceStyle::Wireframe || item.style == SurfaceStyle::ShadedWithEdges;
        const Rgba edgeColor =
            shaded ? katana::render::rgba(40, 40, 45) : katana::render::rgba(190, 190, 190);
        const float bias = shaded ? options.edgeDepthBias : 0.0f;

#if defined(KATANA_HAVE_AVX2_KERNELS)
        // Shaded without edges - how a mesh is drawn unless asked otherwise -
        // is three private vertices and a triangle a face: the kernel's shape.
        // It takes each run of faces that name real vertices; a face that
        // does not is skipped, as triangle() refuses it below.
        if (kernels && shaded && !edges && mesh.triangleCount() >= kMeshKernelMinimum) {
            const std::size_t faces = mesh.triangleCount();
            const std::size_t points = mesh.vertices.size();
            const auto valid = [&](std::size_t f) {
                const auto& face = mesh.faces[f];
                return face[0] < points && face[1] < points && face[2] < points;
            };
            std::size_t f = 0;
            while (f < faces) {
                if (!valid(f)) {
                    ++f;
                    continue;
                }
                std::size_t end = f + 1;
                while (end < faces && valid(end)) {
                    ++end;
                }
                const std::size_t run = end - f;
                const std::size_t first = out.positions.size();
                out.positions.resize(first + 3 * run);
                out.colors.resize(first + 3 * run);
                katana_avx2_scene_mesh_faces(&mesh.vertices.data()->x, points,
                                             mesh.faces.data()->data(), f, run, params.data(),
                                             item.faceColors.data(), item.faceColors.size(),
                                             item.flatColor, &out.positions[first].x,
                                             &out.colors[first]);
                const std::size_t triangle = out.triangles.size();
                out.triangles.resize(triangle + run);
                for (std::size_t k = 0; k < run; ++k) {
                    const auto a = static_cast<VertexIndex>(first + 3 * k);
                    out.triangles[triangle + k] = {a + 2, a + 1, a}; // corners c, b, a
                }
                f = end;
            }
            continue;
        }
#endif
        for (std::size_t f = 0; f < mesh.triangleCount(); ++f) {
            // triangle() refuses a face that names a vertex which does not
            // exist, so an unvalidated mesh cannot be read past its end here.
            const auto face = mesh.triangle(f);
            if (!face) {
                continue;
            }
            const Rgba base = f < item.faceColors.size() ? item.faceColors[f] : item.flatColor;
            const Vec3 a(face->a.x, face->a.y, lift(face->a.z));
            const Vec3 b(face->b.x, face->b.y, lift(face->b.z));
            const Vec3 c(face->c.x, face->c.y, lift(face->c.z));
            if (shaded) {
                Rgba color = base;
                const Vec3 normal = (b - a).cross(c - a);
                const double area = normal.length();
                if (area > 1.0e-12) {
                    color = katana::render::shade(base, light.intensity(normal / area));
                }
                // The last corner first: the order these lists have always
                // had, from when the three addVertex calls were addTriangle's
                // arguments and GCC evaluated them right to left. Spelt out
                // so it no longer rests on the compiler; the kernel matches.
                const VertexIndex vc = out.addVertex(c, color);
                const VertexIndex vb = out.addVertex(b, color);
                const VertexIndex va = out.addVertex(a, color);
                out.addTriangle(va, vb, vc);
            }
            if (edges) {
                const VertexIndex ea = out.addVertex(a, edgeColor);
                const VertexIndex eb = out.addVertex(b, edgeColor);
                const VertexIndex ec = out.addVertex(c, edgeColor);
                out.addLine(ea, eb, scale, bias);
                out.addLine(eb, ec, scale, bias);
                out.addLine(ec, ea, scale, bias);
            }
        }
    }
}

// ---- entities -------------------------------------------------------------------

void SceneBuilder::appendEntities(const Document& document, const SceneOptions& options,
                                  DrawList& out)
{
    emitEntities(document, {}, options, Selected::Styled, 0.0, false, out, nullptr);
}

void SceneBuilder::emitEntities(const Document& document,
                                const std::vector<SceneSurface>& surfaces,
                                const SceneOptions& options, Selected which, double datumHint,
                                bool datumKnown, DrawList& out, double* lowestHeight)
{
    const SelectionSet& selection = document.selection();
    const Heights heights(surfaces, options);
    const float scale = std::max(options.pixelScale, 0.1f);
    const VertexIndex firstVertex = static_cast<VertexIndex>(out.positions.size());
    double lowest = std::numeric_limits<double>::infinity();
    std::vector<katana::geometry::Point3> path;
    std::vector<std::optional<double>> own;

    const float baseBias =
        which == Selected::Only ? options.selectionDepthBias : options.entityDepthBias;

    // EVERY drawn path goes through this one emitter, including a plain
    // Segment2. The tempting shortcut - a Segment2 adding its DrawLine
    // directly - would have left lines solid in 3D while polylines, arcs and
    // circles dashed: the most confusing possible half-feature.
    const auto emitPath = [&](const std::vector<Point2>& points, bool closed, Rgba color,
                              float width, const katana::entity::Linetype* linetype) {
        if (points.empty()) {
            return;
        }
        heights.path(points, own, closed, path, &lowest);
        if (points.size() == 1) {
            const VertexIndex v = out.addVertex(path.front(), color);
            out.addPoint(v, options.pointSize * scale, baseBias);
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
            // The dashes are laid along the plan of the DRAPED path, so each
            // end takes its height from the 3D segment it falls on. forEachDash
            // walks the path in order, so one cursor serves the whole path.
            chord_.clear();
            for (const auto& p : path) {
                chord_.emplace_back(p.x, p.y);
            }
            std::size_t cursor = 0;
            const auto zAt = [&](const Point2& q) {
                while (cursor + 1 < path.size()) {
                    const auto& p0 = path[cursor];
                    const auto& p1 = path[cursor + 1];
                    const double ex = p1.x - p0.x;
                    const double ey = p1.y - p0.y;
                    const double length2 = ex * ex + ey * ey;
                    const double t =
                        length2 > 0.0 ? ((q.x - p0.x) * ex + (q.y - p0.y) * ey) / length2 : 1.0;
                    if (t <= 1.0 + 1.0e-9 || cursor + 2 >= path.size()) {
                        return p0.z + (p1.z - p0.z) * std::clamp(t, 0.0, 1.0);
                    }
                    ++cursor;
                }
                return path.back().z;
            };
            // The draped path already holds the closing vertex when closed.
            const bool laid =
                forEachDash(chord_, false, *linetype, dash, [&](const Point2& a, const Point2& b) {
                    const VertexIndex from = out.addVertex(Vec3(a.x, a.y, zAt(a)), color);
                    if (a.distanceTo(b) <= 0.0) {
                        out.addPoint(from, options.pointSize * 0.6f * scale, baseBias);
                        return;
                    }
                    const VertexIndex to = out.addVertex(Vec3(b.x, b.y, zAt(b)), color);
                    out.addLine(from, to, width, baseBias);
                });
            if (laid) {
                return;
            }
        }

        VertexIndex previous = out.addVertex(path[0], color);
        for (std::size_t i = 1; i < path.size(); ++i) {
            const VertexIndex current = out.addVertex(path[i], color);
            out.addLine(previous, current, width, baseBias);
            previous = current;
        }
    };

    document.model().entities.forEach([&](const Entity& entity) {
        // The plan view's rule, so a layer switched off disappears here too.
        if (!isDrawn(document.model(), entity, viewOf(options))) {
            return;
        }
        const bool selected = selection.contains(entity.id);
        if (which == Selected::Only && !selected) {
            return;
        }
        const bool styled = selected && which != Selected::AsDrawn;
        const auto display = katana::entity::resolveDisplay(document.model(), entity);
        const Rgba color =
            styled ? options.selectionColor : colorOf(document.model(), entity, display, options);
        const float width =
            (styled ? options.selectedLineWidth : options.entityLineWidth) * scale;
        // A selected entity is drawn in the selection style, dashes and all.
        const katana::entity::Linetype* linetype =
            styled ? nullptr : document.model().linetypes.find(display.linetype);
        const auto ownHeights = [&](std::size_t count) {
            own.clear();
            if (heights.usesOwnHeights()) {
                own = katana::entity::heightsOf(entity.properties, count);
            }
        };

        std::visit(
            [&](const auto& shape) {
                using Shape = std::decay_t<decltype(shape)>;
                if constexpr (std::is_same_v<Shape, katana::entity::PointGeometry>) {
                    ownHeights(1);
                    chord_ = {shape.position};
                    emitPath(chord_, false, color, width, nullptr);
                } else if constexpr (std::is_same_v<Shape, Segment2>) {
                    ownHeights(2);
                    chord_ = {shape.start, shape.end};
                    emitPath(chord_, false, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, Arc2>) {
                    // An arc's heights are its two ends (setHeights at import):
                    // not per chord vertex, so the chords drape instead.
                    own.clear();
                    chord_ = chordArc(shape, options.chordTolerance);
                    emitPath(chord_, false, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, Circle2>) {
                    own.clear();
                    if (heights.usesOwnHeights()) {
                        // One height, the centre's, for the whole circle.
                        const auto centre = katana::entity::heightsOf(entity.properties, 1);
                        chord_ = chordCircle(shape, options.chordTolerance);
                        own.assign(chord_.size(), centre.front());
                    } else {
                        chord_ = chordCircle(shape, options.chordTolerance);
                    }
                    emitPath(chord_, true, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, Polyline2>) {
                    ownHeights(shape.vertices.size());
                    emitPath(shape.vertices, shape.closed, color, width, linetype);
                } else if constexpr (std::is_same_v<Shape, katana::entity::TextGeometry>) {
                    // Text is not rendered in 3D yet: a glyph outline needs a
                    // font, and drawing a marker where the text sits is more
                    // honest than drawing nothing (the 2D view shows the text).
                    ownHeights(1);
                    chord_ = {shape.position};
                    emitPath(chord_, false, color, width, nullptr);
                } else if constexpr (std::is_same_v<Shape, katana::entity::DimensionGeometry>) {
                    // Drawn through the same builder the 2D viewport uses, so
                    // the two views cannot disagree about what a dimension
                    // looks like. The label is not rendered here - a glyph
                    // outline needs a font, which the 3D path does not have.
                    const auto drawn =
                        buildDimension(shape, resolveDimensionStyle(document.model(), entity));
                    if (drawn.empty()) {
                        return;
                    }
                    own.clear();
                    const auto stroke = [&](const Point2& from, const Point2& to) {
                        chord_ = {from, to};
                        emitPath(chord_, false, color, width, nullptr);
                    };
                    for (const auto& segment : drawn.extensionLines) {
                        stroke(segment.start, segment.end);
                    }
                    if (drawn.hasDimensionLine) {
                        stroke(drawn.dimensionLine.start, drawn.dimensionLine.end);
                    }
                    for (const auto& curve : drawn.curves) {
                        for (std::size_t i = 0; i + 1 < curve.size(); ++i) {
                            stroke(curve[i], curve[i + 1]);
                        }
                    }
                    for (const auto& segment : drawn.arrowStrokes) {
                        stroke(segment.start, segment.end);
                    }
                    for (const auto& fill : drawn.arrowFills) {
                        // Outlined rather than filled: DrawList fills only
                        // triangles, and an arrowhead is small enough that an
                        // outline reads correctly.
                        for (std::size_t i = 0; i < fill.size(); ++i) {
                            stroke(fill[i], fill[(i + 1) % fill.size()]);
                        }
                    }
                } else if constexpr (std::is_same_v<Shape, katana::entity::LabelGeometry>) {
                    // As a text is: a marker where the label attaches. Its
                    // words are worked out at a scale the 3D view does not
                    // have, and set in a font it does not either.
                    own.clear();
                    chord_ = {shape.anchor};
                    emitPath(chord_, false, color, width, nullptr);
                } else if constexpr (std::is_same_v<Shape, katana::entity::LeaderGeometry>) {
                    // The line; the arrow and the note are paper-sized.
                    own.clear();
                    emitPath(shape.vertices, false, color, width, nullptr);
                } else {
                    static_assert(false, "emitEntities has no case for this geometry kind");
                }
            },
            entity.geometry);
    });

    // What had no height and nothing under it goes on the datum, which only
    // now is known when the terrain did not decide it.
    double datum = datumHint;
    if (!datumKnown) {
        datum = liftOf(options)(std::isfinite(lowest)
                                    ? lowest
                                    : (std::isfinite(options.entityElevation) ? options.entityElevation
                                                                              : 0.0));
    }
    for (std::size_t i = firstVertex; i < out.positions.size(); ++i) {
        if (std::isnan(out.positions[i].z)) {
            out.positions[i].z = datum;
        }
    }
    if (lowestHeight != nullptr) {
        *lowestHeight = lowest;
    }
}

// ---- grid -----------------------------------------------------------------------

void SceneBuilder::appendGrid(const SceneOptions& options, const AABB& around, DrawList& out)
{
    if (!options.drawGrid) {
        return;
    }
    const double z = liftOf(options)(std::isfinite(options.entityElevation) ? options.entityElevation
                                                                            : 0.0);
    emitGrid(options, around, z, out);
}

// A grid sized to the scene, not a fixed 800 m patch: on a 12 km corridor that
// was a stray square in empty space, and on a 20 m building it was a sea of
// lines. Each line is emitted cell by cell. A full-length line's quad touches
// every tile its bounding box does, and the fixed grid's 162 long diagonals
// cost 70 to 95 ms a frame - 95% of a small scene's time; cut into cells the
// same grid measured about 3 ms.
void SceneBuilder::emitGrid(const SceneOptions& options, const AABB& around, double z,
                            DrawList& out)
{
    AABB box = around;
    if (box.empty() || !box.min.isFinite() || !box.max.isFinite()) {
        box = AABB(Vec3(-50.0, -50.0, 0.0), Vec3(50.0, 50.0, 0.0));
    }
    const double spanX = box.max.x - box.min.x;
    const double spanY = box.max.y - box.min.y;
    const double span = std::max({spanX, spanY, 1.0});
    const int across = std::clamp(options.gridCellsAcross, 2, 100);
    double spacing = std::isfinite(options.gridSpacing) && options.gridSpacing > 0.0
                         ? options.gridSpacing
                         : niceStep(span / static_cast<double>(across));
    // A reach of 15% past the scene on every side, so the model stands on the
    // grid rather than on its edge.
    const double reach = 0.15 * span;
    // Bounded however the spacing was chosen: at most 200 cells a side.
    while ((spanX + 2.0 * reach) / spacing > 200.0 || (spanY + 2.0 * reach) / spacing > 200.0) {
        spacing *= 2.0;
    }
    const long long i0 = static_cast<long long>(std::floor((box.min.x - reach) / spacing));
    const long long i1 = static_cast<long long>(std::ceil((box.max.x + reach) / spacing));
    const long long j0 = static_cast<long long>(std::floor((box.min.y - reach) / spacing));
    const long long j1 = static_cast<long long>(std::ceil((box.max.y + reach) / spacing));
    const double cx = 0.5 * (static_cast<double>(i0 + i1) * spacing);
    const double cy = 0.5 * (static_cast<double>(j0 + j1) * spacing);
    const double radius =
        0.5 * std::hypot(static_cast<double>(i1 - i0) * spacing, static_cast<double>(j1 - j0) * spacing);
    const float scale = std::max(options.pixelScale, 0.1f);

    // Faded towards the view's background with distance from the middle, so
    // the grid has no hard rim to compete with the model's edge.
    const auto faded = [&](Rgba color, double x, double y) {
        const double r = std::hypot(x - cx, y - cy) / std::max(radius, 1.0e-9);
        return mix(color, options.gridFadeColor, std::clamp((r - 0.45) / 0.55, 0.0, 1.0));
    };
    const auto cellLine = [&](double xa, double ya, double xb, double yb, Rgba color, float width) {
        const VertexIndex a = out.addVertex(Vec3(xa, ya, z), faded(color, xa, ya));
        const VertexIndex b = out.addVertex(Vec3(xb, yb, z), faded(color, xb, yb));
        out.addLine(a, b, width * scale, 0.0f);
    };
    // Only where the REAL origin is in range: survey data at MGA coordinates
    // is kilometres from it, and the old axes through the scene's centre
    // looked like axes but meant nothing.
    const auto lineStyle = [&](long long index, Rgba axis) {
        if (index == 0) {
            return std::make_pair(axis, 2.0f);
        }
        return std::make_pair(index % 5 == 0 ? options.gridMajorColor : options.gridColor, 1.0f);
    };
    // The grid writes no depth (renderLayers), so where two of its lines
    // cross, the one drawn later covers the other: the plain lines first,
    // then every fifth, then the axes, so that no plain line breaks an axis.
    const auto rankOf = [](long long index) { return index == 0 ? 2 : (index % 5 == 0 ? 1 : 0); };
    for (int rank = 0; rank < 3; ++rank) {
        for (long long i = i0; i <= i1; ++i) {
            if (rankOf(i) != rank) {
                continue;
            }
            const double x = static_cast<double>(i) * spacing;
            const auto [color, width] = lineStyle(i, options.axisColorY);
            for (long long j = j0; j < j1; ++j) {
                cellLine(x, static_cast<double>(j) * spacing, x,
                         static_cast<double>(j + 1) * spacing, color, width);
            }
        }
        for (long long j = j0; j <= j1; ++j) {
            if (rankOf(j) != rank) {
                continue;
            }
            const double y = static_cast<double>(j) * spacing;
            const auto [color, width] = lineStyle(j, options.axisColorX);
            for (long long i = i0; i < i1; ++i) {
                cellLine(static_cast<double>(i) * spacing, y, static_cast<double>(i + 1) * spacing,
                         y, color, width);
            }
        }
    }
}

// ---- layers ---------------------------------------------------------------------

void SceneBuilder::buildTerrain(const std::vector<SceneSurface>& surfaces,
                                const std::vector<SceneMesh>& meshes, const SceneOptions& options,
                                SceneLayers& layers)
{
    layers.terrain.clear();
    layers.edges.clear();
    layers.edgeRuns.clear();
    layers.edgeBase.clear();
    layers.edgeInk.clear();
    const Ramp ramp = rampOf(surfaces);
    layers.rampLow = ramp.low;
    layers.rampHigh = ramp.high;
    // Where each surface's edges begin in edges.lines.
    std::vector<std::size_t> edgeStarts;
    for (const SceneSurface& item : surfaces) {
        if (drawable(item)) {
            edgeStarts.push_back(layers.edges.lines.size());
            emitSurface(item, options, ramp, {normals_, lifted_, summed_}, layers.terrain,
                        &layers);
        }
    }
    // The edges are drawn writing no depth (renderLayers), so where two
    // surfaces coincide - a design repeating the existing triangles outside
    // the works - the one drawn later covers the other. The terrain goes the
    // other way: at a tie the surface drawn FIRST is the one seen. So the
    // edges go surface by surface in reverse, and the edges on top are those
    // of the surface seen, as they were while edges wrote depth.
    if (edgeStarts.size() > 1) {
        std::vector<katana::render::DrawLine> reversed;
        reversed.reserve(layers.edges.lines.size());
        std::size_t end = layers.edges.lines.size();
        for (auto start = edgeStarts.rbegin(); start != edgeStarts.rend(); ++start) {
            reversed.insert(reversed.end(),
                            layers.edges.lines.begin() + static_cast<std::ptrdiff_t>(*start),
                            layers.edges.lines.begin() + static_cast<std::ptrdiff_t>(end));
            end = *start;
        }
        layers.edges.lines = std::move(reversed);
    }
    appendMeshes(meshes, options, layers.terrain);
    const auto floor = terrainFloor(surfaces, meshes);
    layers.terrainDatum = floor.has_value();
    layers.datum = liftOf(options)(floor ? *floor
                                         : (std::isfinite(options.entityElevation)
                                                ? options.entityElevation
                                                : 0.0));
    layers.terrainBounds = layers.terrain.bounds(used_);
    layers.terrainBounds.expand(layers.edges.bounds(used_));
    layers.bounds = layers.terrainBounds;
}

void SceneBuilder::buildEntities(const Document& document,
                                 const std::vector<SceneSurface>& surfaces,
                                 const SceneOptions& options, SceneLayers& layers)
{
    layers.entities.clear();
    if (!options.drawEntities) {
        return;
    }
    double lowest = std::numeric_limits<double>::infinity();
    emitEntities(document, surfaces, options, Selected::AsDrawn, layers.datum, layers.terrainDatum,
                 layers.entities, &lowest);
    if (!layers.terrainDatum) {
        layers.datum = liftOf(options)(std::isfinite(lowest)
                                           ? lowest
                                           : (std::isfinite(options.entityElevation)
                                                  ? options.entityElevation
                                                  : 0.0));
    }
    layers.bounds = layers.terrainBounds;
    layers.bounds.expand(layers.entities.bounds(used_));
}

void SceneBuilder::buildSelection(const Document& document,
                                  const std::vector<SceneSurface>& surfaces,
                                  const SceneOptions& options, SceneLayers& layers)
{
    layers.selection.clear();
    if (!options.drawEntities || document.selection().empty()) {
        return;
    }
    emitEntities(document, surfaces, options, Selected::Only, layers.datum, true,
                 layers.selection, nullptr);
}

void SceneBuilder::buildGrid(const SceneOptions& options, SceneLayers& layers)
{
    layers.grid.clear();
    if (options.drawGrid) {
        emitGrid(options, layers.bounds, layers.datum, layers.grid);
    }
}

bool SceneBuilder::fadeEdges(SceneLayers& layers, const katana::render::Camera& camera)
{
    bool any = false;
    // The size of a pixel where the camera looks: one number per frame, so
    // the whole surface fades together rather than in bands by distance.
    const double pixel = camera.worldPerPixelAt(camera.distance());
    for (SceneLayers::EdgeRun& run : layers.edgeRuns) {
        const double pixels = pixel > 0.0 ? run.typicalEdge / pixel : 0.0;
        // Nothing below 4 px - the triangles are then texture, not structure
        // - and full strength from 12; in eighths, so orbiting does not
        // recolour a million vertices a frame for an invisible change.
        const double t = std::clamp((pixels - 4.0) / 8.0, 0.0, 1.0);
        const float strength = static_cast<float>(std::round(t * t * (3.0 - 2.0 * t) * 8.0) / 8.0);
        if (strength != run.applied) {
            const std::size_t end = std::min<std::size_t>(run.first + run.count,
                                                          layers.edges.colors.size());
#if defined(KATANA_HAVE_AVX2_KERNELS)
            if (end >= run.first + kFadeKernelMinimum && end <= layers.edgeBase.size() &&
                end <= layers.edgeInk.size() && avx2Active()) {
                // strength is a whole number of eighths (above), exactly.
                katana_avx2_scene_fade(layers.edgeBase.data() + run.first,
                                       layers.edgeInk.data() + run.first, end - run.first,
                                       static_cast<int>(strength * 8.0f),
                                       layers.edges.colors.data() + run.first);
                run.applied = strength;
                any = any || strength > 0.0f;
                continue;
            }
#endif
            for (std::size_t i = run.first; i < end; ++i) {
                layers.edges.colors[i] = mix(layers.edgeBase[i], layers.edgeInk[i], strength);
            }
            run.applied = strength;
        }
        any = any || strength > 0.0f;
    }
    return any;
}

void SceneBuilder::build(const Document& document, const std::vector<SceneSurface>& surfaces,
                         const SceneOptions& options, DrawList& out,
                         const std::vector<SceneMesh>& meshes)
{
    out.clear();
    SceneLayers layers;
    buildTerrain(surfaces, meshes, options, layers);
    DrawList entities;
    double lowest = std::numeric_limits<double>::infinity();
    if (options.drawEntities) {
        emitEntities(document, surfaces, options, Selected::Styled, layers.datum,
                     layers.terrainDatum, entities, &lowest);
        if (!layers.terrainDatum && std::isfinite(lowest)) {
            layers.datum = liftOf(options)(lowest);
        }
        layers.bounds.expand(entities.bounds(used_));
    }
    const auto append = [&out](const DrawList& from) {
        const VertexIndex base = static_cast<VertexIndex>(out.positions.size());
        out.positions.insert(out.positions.end(), from.positions.begin(), from.positions.end());
        out.colors.insert(out.colors.end(), from.colors.begin(), from.colors.end());
        for (const auto& t : from.triangles) {
            out.addTriangle(base + t.a, base + t.b, base + t.c);
        }
        for (const auto& l : from.lines) {
            out.addLine(base + l.a, base + l.b, l.width, l.depthBias);
        }
        for (const auto& p : from.points) {
            out.addPoint(base + p.a, p.size, p.depthBias);
        }
    };
    if (options.drawGrid) {
        DrawList grid;
        emitGrid(options, layers.bounds, layers.datum, grid);
        append(grid);
    }
    append(layers.terrain);
    append(layers.edges);
    append(entities);
}

// ---- one frame ------------------------------------------------------------------

katana::core::Result<katana::render::RenderStats>
renderLayers(SceneLayers& layers, katana::render::Camera& camera,
             katana::render::Rasterizer& rasterizer, katana::render::Framebuffer& target,
             katana::render::RenderOptions options)
{
    // The depth range is fitted to what is drawn EVERY frame: after an orbit,
    // a pan or a zoom it was left where frame() put it, and eight wheel
    // notches out pushed the model past the far plane.
    AABB depthBox = layers.bounds;
    depthBox.expand(layers.grid.bounds());
    camera.fitDepthRange(depthBox);
    const bool drawEdges = SceneBuilder::fadeEdges(layers, camera);

    // In this order into one depth buffer, so equal depths resolve the same
    // way every frame (the first drawn wins, Rule 7). Why the grid and the
    // edges write no depth: scene.hpp, renderLayers.
    struct Pass {
        const DrawList* list;
        bool depthWrite;
    };
    const Pass passes[] = {{&layers.grid, false},
                           {&layers.terrain, true},
                           {drawEdges ? &layers.edges : nullptr, false},
                           {&layers.entities, true},
                           {&layers.selection, true}};
    katana::render::RenderStats total;
    for (const Pass& pass : passes) {
        // An empty list still clears when it comes first.
        if (pass.list == nullptr || (pass.list->empty() && !options.clear)) {
            continue;
        }
        options.depthWrite = pass.depthWrite;
        auto result = rasterizer.render(*pass.list, camera, target, options);
        if (!result) {
            return result;
        }
        total.vertices += result->vertices;
        total.trianglesSubmitted += result->trianglesSubmitted;
        total.trianglesRasterised += result->trianglesRasterised;
        total.linesSubmitted += result->linesSubmitted;
        total.pointsSubmitted += result->pointsSubmitted;
        total.fragments += result->fragments;
        total.binEntries += result->binEntries;
        total.tiles = result->tiles;
        options.clear = false;
    }
    return total;
}

AABB sceneBounds(const Document& document, const std::vector<SceneSurface>& surfaces,
                 const SceneOptions& options, const std::vector<SceneMesh>& meshes)
{
    const Lift lift = liftOf(options);
    AABB box;
    double terrainLow = std::numeric_limits<double>::infinity();
    double terrainHigh = -std::numeric_limits<double>::infinity();
    for (const SceneSurface& item : surfaces) {
        if (!drawable(item)) {
            continue;
        }
        const auto& plan = item.surface->bounds();
        if (plan.empty()) {
            continue;
        }
        box.expand(Vec3(plan.min.x, plan.min.y, lift(item.surface->minElevation())));
        box.expand(Vec3(plan.max.x, plan.max.y, lift(item.surface->maxElevation())));
        terrainLow = std::min(terrainLow, item.surface->minElevation());
        terrainHigh = std::max(terrainHigh, item.surface->maxElevation());
    }
    for (const SceneMesh& item : meshes) {
        if (!drawable(item)) {
            continue;
        }
        const AABB space = item.mesh->bounds();
        if (space.empty()) {
            continue;
        }
        box.expand(Vec3(space.min.x, space.min.y, lift(space.min.z)));
        box.expand(Vec3(space.max.x, space.max.y, lift(space.max.z)));
        terrainLow = std::min(terrainLow, space.min.z);
        terrainHigh = std::max(terrainHigh, space.max.z);
    }
    if (options.drawEntities) {
        // What is DRAWN, not every entity: a hidden stray far away used to
        // frame an almost empty view (audit REN-03). The same box a plan
        // view's Zoom Extents frames, from the one function that computes it.
        const katana::geometry::Box2 plan = drawnExtent(document.model(), viewOf(options));
        if (!plan.empty()) {
            // In z the linework spans what the build can put it at: its own
            // heights, the surfaces it drapes on, and the datum.
            double low = terrainLow;
            double high = terrainHigh;
            if (options.linework == LineworkHeights::Heights ||
                options.linework == LineworkHeights::HeightsThenDrape) {
                document.model().entities.forEach([&](const Entity& entity) {
                    if (!isDrawn(document.model(), entity, viewOf(options))) {
                        return;
                    }
                    // As many heights as emitEntities reads for the kind:
                    // one for a point, text or circle, one per vertex of a
                    // line or polyline; an arc and a dimension are draped.
                    std::size_t count = 1;
                    if (const auto* line = std::get_if<Polyline2>(&entity.geometry)) {
                        count = line->vertices.size();
                    } else if (std::holds_alternative<Segment2>(entity.geometry)) {
                        count = 2;
                    } else if (std::holds_alternative<Arc2>(entity.geometry) ||
                               std::holds_alternative<katana::entity::DimensionGeometry>(
                                   entity.geometry)) {
                        count = 0;
                    }
                    for (const auto& h : katana::entity::heightsOf(entity.properties, count)) {
                        if (h && std::isfinite(*h)) {
                            low = std::min(low, *h);
                            high = std::max(high, *h);
                        }
                    }
                });
            }
            if (!std::isfinite(low)) {
                low = std::isfinite(options.entityElevation) ? options.entityElevation : 0.0;
            }
            if (!std::isfinite(high)) {
                high = low;
            }
            box.expand(Vec3(plan.min.x, plan.min.y, lift(low)));
            box.expand(Vec3(plan.max.x, plan.max.y, lift(high)));
        }
    }
    return box;
}

} // namespace katana::cad
