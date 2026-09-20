#include "katana/cad/section.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <type_traits>
#include <variant>

#include "katana/geometry/intersection.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::geometry::Box2;
using katana::geometry::Segment2;
using katana::terrain::TinSurface;

namespace {

// Cumulative length at each vertex, so a station converts to a position in one
// binary search rather than by walking the polyline.
struct Stationing {
    std::vector<double> atVertex; // atVertex[0] == 0
    double length = 0.0;

    [[nodiscard]] Point2 positionAt(const Polyline2& line, double station) const
    {
        if (atVertex.size() < 2) {
            return line.vertices.empty() ? Point2{} : line.vertices.front();
        }
        const double clamped = std::clamp(station, 0.0, length);
        // upper_bound then step back: the segment containing `clamped` is the
        // one whose start station is the greatest not exceeding it.
        auto it = std::upper_bound(atVertex.begin(), atVertex.end(), clamped);
        std::size_t index = static_cast<std::size_t>(it - atVertex.begin());
        if (index == 0) {
            index = 1;
        }
        if (index >= atVertex.size()) {
            index = atVertex.size() - 1;
        }
        const double from = atVertex[index - 1];
        const double to = atVertex[index];
        const double span = to - from;
        const Point2& a = line.vertices[index - 1];
        const Point2& b = line.vertices[index % line.vertices.size()];
        if (span <= 0.0) {
            return a;
        }
        const double t = (clamped - from) / span;
        return a + (b - a) * t;
    }
};

[[nodiscard]] Stationing stationOf(const Polyline2& line)
{
    Stationing stationing;
    stationing.atVertex.reserve(line.vertices.size() + 1);
    double running = 0.0;
    stationing.atVertex.push_back(0.0);
    for (std::size_t i = 0; i + 1 < line.vertices.size(); ++i) {
        running += line.vertices[i].distanceTo(line.vertices[i + 1]);
        stationing.atVertex.push_back(running);
    }
    if (line.closed && line.vertices.size() > 2) {
        running += line.vertices.back().distanceTo(line.vertices.front());
        stationing.atVertex.push_back(running);
    }
    stationing.length = running;
    return stationing;
}

// The triangle a position sits in, or kNoTriangle. Used only to detect that the
// alignment has crossed from one triangle to another; the elevation itself
// comes from elevationAt.
[[nodiscard]] std::uint32_t triangleAt(const TinSurface& surface, const Point2& position)
{
    const auto location = surface.locate(position);
    return location.has_value() ? location->triangle : katana::terrain::kNoTriangle;
}

struct Candidate {
    double station = 0.0;
    SampleReason reason = SampleReason::Interval;
};

// Bisects [lo, hi] for the station at which the located triangle changes,
// stopping once the bracket is below kGeometric. Returns the station just past
// the change, so that sampling there lands in the NEW triangle.
[[nodiscard]] double findBreak(const TinSurface& surface, const Stationing& stationing,
                               const Polyline2& line, double lo, double hi,
                               std::uint32_t loTriangle)
{
    // Bounded independently of the tolerance so a pathological surface cannot
    // spin here: 60 halvings takes any realistic bracket below 1e-15.
    for (int iteration = 0; iteration < 60 && hi - lo > tol::kGeometric; ++iteration) {
        const double mid = 0.5 * (lo + hi);
        if (triangleAt(surface, stationing.positionAt(line, mid)) == loTriangle) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    return hi;
}

void summarise(SectionSurface& surface)
{
    double low = std::numeric_limits<double>::infinity();
    double high = -std::numeric_limits<double>::infinity();
    bool any = false;
    for (const SectionSample& sample : surface.samples) {
        if (sample.elevation.has_value()) {
            low = std::min(low, *sample.elevation);
            high = std::max(high, *sample.elevation);
            any = true;
        }
    }
    if (any) {
        surface.minElevation = low;
        surface.maxElevation = high;
    }
}

// Exact plan crossings of `piece` with one entity's geometry.
//
// The curve overloads of intersect() are used directly rather than tessellating
// first: an arc crossing the alignment has ONE true station, and a chorded
// approximation of it would put the crossing wherever the chords happened to
// fall. Text, points and dimensions carry no plan curve and cross nothing.
void appendCrossings(const Segment2& piece, const katana::entity::Geometry& geometry,
                     katana::geometry::IntersectionResult& out)
{
    out = std::visit(
        [&piece](const auto& shape) -> katana::geometry::IntersectionResult {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, Segment2> ||
                          std::is_same_v<Shape, katana::geometry::Circle2> ||
                          std::is_same_v<Shape, katana::geometry::Arc2>) {
                return intersect(piece, shape);
            } else {
                return katana::geometry::IntersectionResult{};
            }
        },
        geometry);
}

} // namespace

Box2 Section::extent() const
{
    Box2 box;
    for (const SectionSurface& surface : surfaces) {
        for (const SectionSample& sample : surface.samples) {
            if (sample.elevation.has_value()) {
                box.expand(Point2(sample.station, *sample.elevation));
            }
        }
    }
    for (const SectionCrossing& crossing : crossings) {
        if (crossing.elevation.has_value()) {
            box.expand(Point2(crossing.station, *crossing.elevation));
        }
    }
    return box;
}

Result<Section> extractSection(const Polyline2& alignment,
                               const std::vector<SectionSurfaceInput>& surfaces,
                               const katana::entity::Model* model, const SectionOptions& options)
{
    const Polyline2 cleaned = alignment.withoutDuplicateVertices();
    if (cleaned.vertices.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "a section alignment needs at least two distinct vertices");
    }
    if (!std::isfinite(options.interval) || options.interval <= 0.0) {
        return makeError(ErrorCode::InvalidArgument, "the section interval must be positive");
    }
    for (const SectionSurfaceInput& input : surfaces) {
        if (input.surface == nullptr) {
            return makeError(ErrorCode::InvalidArgument, "null surface in the section request",
                             input.name);
        }
    }

    const Stationing stationing = stationOf(cleaned);
    if (stationing.length <= tol::kGeometric) {
        return makeError(ErrorCode::InvalidArgument, "the section alignment has no length");
    }
    // Checked before anything is allocated: a 20 km alignment at a 1 mm
    // interval is a typo, and it must say so rather than swap the machine out.
    const double wanted = stationing.length / options.interval;
    if (wanted + static_cast<double>(cleaned.vertices.size()) >
        static_cast<double>(options.maximumSamples)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the interval would produce more samples than the limit allows",
                         std::to_string(static_cast<long long>(wanted)) + " > " +
                             std::to_string(options.maximumSamples));
    }

    Section section;
    section.alignment = cleaned;
    section.length = stationing.length;

    // Stations every surface shares: the interval grid plus the alignment's own
    // vertices. Built once because it does not depend on the surface.
    std::vector<Candidate> base;
    base.push_back(Candidate{0.0, SampleReason::Start});
    for (std::size_t i = 1; i + 1 < stationing.atVertex.size(); ++i) {
        base.push_back(Candidate{stationing.atVertex[i], SampleReason::AlignmentVertex});
    }
    for (double station = options.interval; station < stationing.length;
         station += options.interval) {
        base.push_back(Candidate{station, SampleReason::Interval});
    }
    base.push_back(Candidate{stationing.length, SampleReason::End});
    std::stable_sort(base.begin(), base.end(),
                     [](const Candidate& a, const Candidate& b) { return a.station < b.station; });

    for (const SectionSurfaceInput& input : surfaces) {
        const TinSurface& surface = *input.surface;
        SectionSurface out;
        out.name = input.name;

        std::vector<Candidate> stations = base;
        if (options.includeSurfaceBreaks && !surface.empty()) {
            // One pass over consecutive base stations: where the located
            // triangle differs, the alignment crossed an edge somewhere
            // between them, and bisection finds where.
            std::vector<Candidate> breaks;
            std::uint32_t previous =
                triangleAt(surface, stationing.positionAt(cleaned, stations.front().station));
            for (std::size_t i = 1; i < stations.size(); ++i) {
                const std::uint32_t current =
                    triangleAt(surface, stationing.positionAt(cleaned, stations[i].station));
                if (current != previous) {
                    const double at = findBreak(surface, stationing, cleaned,
                                                stations[i - 1].station, stations[i].station,
                                                previous);
                    breaks.push_back(Candidate{at, SampleReason::SurfaceBreak});
                }
                previous = current;
            }
            stations.insert(stations.end(), breaks.begin(), breaks.end());
            std::stable_sort(stations.begin(), stations.end(),
                             [](const Candidate& a, const Candidate& b) {
                                 return a.station < b.station;
                             });
        }

        out.samples.reserve(stations.size());
        out.reasons.reserve(stations.size());
        for (const Candidate& candidate : stations) {
            // Duplicates arise where a break lands on an interval station or a
            // vertex; the earlier reason wins because Start/Vertex say more
            // about the sample than Interval does.
            if (!out.samples.empty() &&
                candidate.station - out.samples.back().station <= tol::kGeometric) {
                continue;
            }
            SectionSample sample;
            sample.station = candidate.station;
            sample.plan = stationing.positionAt(cleaned, candidate.station);
            sample.elevation = surface.elevationAt(sample.plan);
            out.samples.push_back(sample);
            out.reasons.push_back(candidate.reason);
        }
        summarise(out);
        section.surfaces.push_back(std::move(out));
    }

    if (options.includeCrossings && model != nullptr) {
        const auto record = [&](const Entity& entity, std::size_t segmentIndex,
                                const Segment2& along, const Point2& at) {
            SectionCrossing crossing;
            crossing.entity = entity.id;
            crossing.layer = entity.layer;
            crossing.plan = at;
            crossing.station = stationing.atVertex[segmentIndex] + along.start.distanceTo(at);
            for (const SectionSurfaceInput& input : surfaces) {
                crossing.elevation = input.surface->elevationAt(at);
                if (crossing.elevation.has_value()) {
                    break;
                }
            }
            section.crossings.push_back(std::move(crossing));
        };

        katana::geometry::IntersectionResult hit;
        model->entities.forEach([&](const Entity& entity) {
            if (!entity.visible) {
                return; // a hidden entity is not on the section either
            }
            for (std::size_t s = 0; s < cleaned.segmentCount(); ++s) {
                const Segment2 along = cleaned.segment(s);
                if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
                    for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
                        const auto piece = intersect(along, polyline->segment(i));
                        if (piece.kind != katana::geometry::IntersectionKind::Points) {
                            continue;
                        }
                        for (std::size_t k = 0; k < piece.count; ++k) {
                            record(entity, s, along, piece.points[k]);
                        }
                    }
                    continue;
                }
                appendCrossings(along, entity.geometry, hit);
                if (hit.kind != katana::geometry::IntersectionKind::Points) {
                    continue; // an overlap has no single station to report
                }
                for (std::size_t k = 0; k < hit.count; ++k) {
                    record(entity, s, along, hit.points[k]);
                }
            }
        });
        std::stable_sort(section.crossings.begin(), section.crossings.end(),
                         [](const SectionCrossing& a, const SectionCrossing& b) {
                             return a.station < b.station;
                         });
    }

    return section;
}

Result<Polyline2> crossSectionLine(const Polyline2& alignment, double station, double halfWidth)
{
    const Polyline2 cleaned = alignment.withoutDuplicateVertices();
    if (cleaned.vertices.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "a cross section needs an alignment of at least two distinct vertices");
    }
    if (!std::isfinite(halfWidth) || halfWidth <= 0.0) {
        return makeError(ErrorCode::InvalidArgument, "the cross section width must be positive");
    }
    const Stationing stationing = stationOf(cleaned);
    if (!std::isfinite(station) || station < -tol::kGeometric ||
        station > stationing.length + tol::kGeometric) {
        return makeError(ErrorCode::InvalidArgument, "the station is outside the alignment",
                         std::to_string(station) + " of " + std::to_string(stationing.length));
    }

    const double clamped = std::clamp(station, 0.0, stationing.length);
    const Point2 centre = stationing.positionAt(cleaned, clamped);

    // Direction from a short chord about the station rather than from the
    // segment alone, so a station landing exactly on a vertex has a defined
    // direction (the bisector) instead of depending on which side it rounds to.
    const double reach = std::max(stationing.length * 1.0e-6, tol::kGeometric);
    const Point2 behind = stationing.positionAt(cleaned, std::max(0.0, clamped - reach));
    const Point2 ahead = stationing.positionAt(cleaned, std::min(stationing.length, clamped + reach));
    const auto direction = (ahead - behind);
    const double length = direction.length();
    if (length <= tol::kGeometric) {
        return makeError(ErrorCode::InvalidArgument,
                         "the alignment has no direction at that station");
    }
    const katana::geometry::Vec2 unit = direction / length;
    // Left of the direction of travel is (-dy, dx); the line runs left to right
    // so that offsets increase to the right, as a surveyor reads a section.
    const katana::geometry::Vec2 normal(-unit.y, unit.x);

    Polyline2 line;
    line.vertices.push_back(centre + normal * halfWidth);
    line.vertices.push_back(centre - normal * halfWidth);
    return line;
}

Result<std::vector<double>> sectionStations(const Polyline2& alignment, double interval)
{
    const Polyline2 cleaned = alignment.withoutDuplicateVertices();
    if (cleaned.vertices.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "stationing needs an alignment of at least two distinct vertices");
    }
    if (!std::isfinite(interval) || interval <= 0.0) {
        return makeError(ErrorCode::InvalidArgument, "the stationing interval must be positive");
    }
    const double length = stationOf(cleaned).length;
    if (length <= tol::kGeometric) {
        return makeError(ErrorCode::InvalidArgument, "the alignment has no length");
    }
    if (length / interval > 1.0e7) {
        return makeError(ErrorCode::InvalidArgument,
                         "the interval would produce an unreasonable number of stations");
    }

    std::vector<double> stations;
    stations.reserve(static_cast<std::size_t>(length / interval) + 2);
    // Multiplied rather than accumulated, so the last station is not the sum of
    // ten thousand roundings.
    for (std::size_t i = 0;; ++i) {
        const double station = static_cast<double>(i) * interval;
        if (station >= length - tol::kGeometric) {
            break;
        }
        stations.push_back(station);
    }
    stations.push_back(length);
    return stations;
}

} // namespace katana::cad
