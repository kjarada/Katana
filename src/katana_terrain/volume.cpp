#include "katana/terrain/volume.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <optional>
#include <vector>

#include "cdt_backend.hpp"
#include "point_merge.hpp"
#include "summation.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Triangle2;

namespace {

// Integral of a function that is linear over a triangle, positive and negative
// part kept apart.
//
// With vertex values f0, f1, f2 and plan area A the integral is A * (f0+f1+f2)/3.
// When the signs differ, exactly one vertex p stands alone against q and r. The
// zero line meets edges pq and pr at the fractions tq = fp/(fp-fq) and
// tr = fp/(fp-fr), cutting off the triangle (p, mq, mr) of area A*tq*tr with
// values (fp, 0, 0). The rest is the quadrilateral (mq, q, r, mr), split into the
// triangles (mq, q, r) of area A*(1-tq) and (mq, r, mr) of area A*tq*(1-tr).
// Every term of a part has the same sign, so nothing cancels, and no coordinates
// are constructed.
class SignedIntegral {
  public:
    void add(double area, const std::array<double, 3>& f)
    {
        if (!(area > 0.0)) {
            return;
        }
        const int positives = (f[0] > 0.0) + (f[1] > 0.0) + (f[2] > 0.0);
        const int negatives = (f[0] < 0.0) + (f[1] < 0.0) + (f[2] < 0.0);
        if (negatives == 0 || positives == 0) {
            const double volume = area * ((f[0] + f[1] + f[2]) / 3.0);
            if (positives != 0) {
                positive_.add(volume);
                positiveArea_.add(area);
            } else if (negatives != 0) {
                negative_.add(-volume);
                negativeArea_.add(area);
            }
            return;
        }

        // Mixed signs: the lone vertex is the only positive or the only negative one.
        const bool lonePositive = positives == 1;
        std::size_t p = 0;
        for (std::size_t i = 0; i < 3; ++i) {
            if (lonePositive ? f[i] > 0.0 : f[i] < 0.0) {
                p = i;
            }
        }
        const double fp = std::abs(f[p]);
        const double fq = std::abs(f[(p + 1) % 3]); // the others lie on the opposite
        const double fr = std::abs(f[(p + 2) % 3]); // side or on the zero line
        const double tq = fp / (fp + fq);
        const double tr = fp / (fp + fr);
        const double tipArea = area * tq * tr;
        const double tipVolume = tipArea * fp / 3.0;
        const double restVolume =
            area * (1.0 - tq) * (fq + fr) / 3.0 + area * tq * (1.0 - tr) * fr / 3.0;
        (lonePositive ? positive_ : negative_).add(tipVolume);
        (lonePositive ? positiveArea_ : negativeArea_).add(tipArea);
        (lonePositive ? negative_ : positive_).add(restVolume);
        (lonePositive ? negativeArea_ : positiveArea_).add(area - tipArea);
    }

    [[nodiscard]] double positive() const { return positive_.value(); }
    [[nodiscard]] double negative() const { return negative_.value(); } // as a magnitude
    [[nodiscard]] double positiveArea() const { return positiveArea_.value(); }
    [[nodiscard]] double negativeArea() const { return negativeArea_.value(); }

  private:
    detail::CompensatedSum positive_;
    detail::CompensatedSum negative_;
    detail::CompensatedSum positiveArea_;
    detail::CompensatedSum negativeArea_;
};

// The part of one surface that takes part in an overlay.
struct OverlayPart {
    const TinSurface* surface = nullptr;
    std::vector<std::uint8_t> selected; // per triangle
    std::uint8_t tag = 1;
};

// Triangles whose bounding box reaches into `region`; all others cannot lie in
// the common area.
std::vector<std::uint8_t> selectTriangles(const TinSurface& surface, const Box2& region)
{
    std::vector<std::uint8_t> selected(surface.triangleCount(), std::uint8_t{0});
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        if (surface.planTriangle(t).boundingBox().intersects(region)) {
            selected[t] = 1;
        }
    }
    return selected;
}

// Elevation of the plane of `triangle` at the three given positions. The
// weights are exact at the triangle's own vertices (1, 0, 0), which is what makes
// the comparison of identical surfaces exactly zero.
std::optional<std::array<double, 3>> planeElevations(const TinSurface& surface,
                                                     std::uint32_t triangle,
                                                     const std::array<Point2, 3>& positions)
{
    const Triangle2 plan = surface.planTriangle(triangle);
    const TinTriangle& tri = surface.triangles()[triangle];
    std::array<double, 3> elevations{};
    for (std::size_t i = 0; i < 3; ++i) {
        const auto weights = plan.barycentric(positions[i]);
        if (!weights) {
            return std::nullopt;
        }
        elevations[i] = (*weights)[0] * surface.vertices()[tri[0]].z +
                        (*weights)[1] * surface.vertices()[tri[1]].z +
                        (*weights)[2] * surface.vertices()[tri[2]].z;
    }
    return elevations;
}

// Elevations of `surface` at the corners of an overlay triangle that lies in one
// of its triangles. The triangle is found through the centroid, which is
// strictly inside the overlay triangle and so never ambiguous between
// neighbours; nullopt when the overlay triangle is off the surface.
std::optional<std::array<double, 3>> cornerElevations(const TinSurface& surface,
                                                      const std::array<Point2, 3>& corners)
{
    const Point2 centroid = Triangle2{corners[0], corners[1], corners[2]}.centroid();
    const auto location = surface.locate(centroid);
    if (!location) {
        return std::nullopt;
    }
    if (location->interior) {
        if (const auto elevations = planeElevations(surface, location->triangle, corners)) {
            return elevations;
        }
    }
    // The centroid sits on a sliver or on the rim: no reliable plane. Such an
    // overlay triangle has next to no area; sample its corners individually.
    std::array<double, 3> elevations{};
    for (std::size_t i = 0; i < 3; ++i) {
        const auto z = surface.elevationAt(corners[i]);
        if (!z) {
            return std::nullopt;
        }
        elevations[i] = *z;
    }
    return elevations;
}

} // namespace

Result<DatumVolume> volumeToDatum(const TinSurface& surface, double datum)
{
    if (!std::isfinite(datum)) {
        return makeError(ErrorCode::InvalidArgument, "datum elevation must be finite");
    }
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "surface has no triangles");
    }
    SignedIntegral integral;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const TinTriangle& tri = surface.triangles()[t];
        // Heights above the datum first: subtracting the datum from the summed
        // prism volume instead would cancel most digits for elevations far from 0.
        integral.add(surface.trianglePlanArea(t), {surface.vertices()[tri[0]].z - datum,
                                                   surface.vertices()[tri[1]].z - datum,
                                                   surface.vertices()[tri[2]].z - datum});
    }
    DatumVolume result;
    result.above = integral.positive();
    result.below = integral.negative();
    result.net = result.above - result.below;
    result.planAreaAbove = integral.positiveArea();
    result.planAreaBelow = integral.negativeArea();
    return result;
}

Result<SurfaceComparison> compareSurfaces(const TinSurface& existing, const TinSurface& design)
{
    if (existing.empty() || design.empty()) {
        return makeError(ErrorCode::InvalidArgument, "both surfaces must have triangles");
    }
    SurfaceComparison result;
    if (!existing.bounds().intersects(design.bounds())) {
        return result;
    }
    const Box2 common(Point2(std::max(existing.bounds().min.x, design.bounds().min.x),
                             std::max(existing.bounds().min.y, design.bounds().min.y)),
                      Point2(std::min(existing.bounds().max.x, design.bounds().max.x),
                             std::min(existing.bounds().max.y, design.bounds().max.y)));
    const Box2 region = common.inflated(tol::kGeometric);

    std::array<OverlayPart, 2> parts{OverlayPart{&existing, selectTriangles(existing, region), 1},
                                     OverlayPart{&design, selectTriangles(design, region), 2}};

    // Overlay input: every vertex of the selected triangles (exactly equal plan
    // positions of the two surfaces become one point; nothing is snapped) and
    // every edge once, as a constraint.
    std::size_t capacity = 0;
    for (const OverlayPart& part : parts) {
        capacity += 3 * static_cast<std::size_t>(
                            std::count(part.selected.begin(), part.selected.end(), 1));
    }
    detail::PointMerger merger(capacity, 0.0);
    std::vector<detail::CdtConstraint> constraints;
    for (const OverlayPart& part : parts) {
        const TinSurface& surface = *part.surface;
        std::vector<std::uint32_t> pointOfVertex(surface.vertexCount(), detail::kNoIndex);
        for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
            if (part.selected[t] == 0) {
                continue;
            }
            const TinTriangle& tri = surface.triangles()[t];
            for (const std::uint32_t v : tri) {
                if (pointOfVertex[v] == detail::kNoIndex) {
                    const Point3& p = surface.vertices()[v];
                    pointOfVertex[v] = merger.add(Point2(p.x, p.y)).index;
                }
            }
            for (std::size_t k = 0; k < 3; ++k) {
                const std::uint32_t neighbor = surface.neighbors()[t][k];
                const bool mine = neighbor == kNoTriangle || part.selected[neighbor] == 0 ||
                                  t < neighbor;
                if (mine) {
                    constraints.push_back(
                        {pointOfVertex[tri[k]], pointOfVertex[tri[(k + 1) % 3]], part.tag});
                }
            }
        }
    }
    if (merger.positions().size() < 3) {
        return result;
    }

    auto overlay = detail::triangulate({merger.positions(), constraints, false});
    if (!overlay) {
        return overlay.error();
    }
    const std::size_t inputCount = merger.positions().size();
    const auto position = [&](std::uint32_t vertex) {
        return vertex < inputCount ? merger.positions()[vertex]
                                   : overlay->generatedPoints[vertex - inputCount];
    };

    // Each overlay triangle lies in one triangle of each surface (or off a
    // surface), so zDesign - zExisting is linear over it.
    SignedIntegral integral;
    detail::CompensatedSum planArea;
    for (const auto& tri : overlay->triangles) {
        const std::array<Point2, 3> corners{position(tri[0]), position(tri[1]), position(tri[2])};
        const double area = Triangle2{corners[0], corners[1], corners[2]}.signedArea();
        if (!(area > 0.0)) {
            continue;
        }
        const auto zExisting = cornerElevations(existing, corners);
        if (!zExisting) {
            continue;
        }
        const auto zDesign = cornerElevations(design, corners);
        if (!zDesign) {
            continue;
        }
        integral.add(area, {(*zDesign)[0] - (*zExisting)[0], (*zDesign)[1] - (*zExisting)[1],
                            (*zDesign)[2] - (*zExisting)[2]});
        planArea.add(area);
        ++result.overlayTriangleCount;
    }
    result.fill = integral.positive();
    result.cut = integral.negative();
    result.net = result.fill - result.cut;
    result.planArea = planArea.value();
    result.fillArea = integral.positiveArea();
    result.cutArea = integral.negativeArea();
    return result;
}

} // namespace katana::terrain
