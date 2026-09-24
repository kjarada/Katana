#pragma once

// A generated survey drawing, standing in for the owner's real archives in
// benchmarks that have to be run at their scale.
//
// The real drawing these numbers are about is a 12 km survey corridor of
// 27,886 entities: about half of them coded points drawn as symbols, the rest
// strings of a dozen vertices, a few of them long. Its archive is 33 MB and is
// not the repository's to ship, so this reproduces its SHAPE - the count, the
// point-to-string mix, the vertex count per string, the extent and the spread
// of string lengths - deterministically, with no RNG, so every machine builds
// the same drawing and the numbers stay comparable.

#include <cmath>
#include <cstddef>
#include <string>
#include <utility>
#include <vector>

#include "katana/entity/entity.hpp"

namespace katana::bench {

// The entity count of the real corridor drawing.
inline constexpr std::size_t kCorridorEntityCount = 27886;

inline std::vector<katana::entity::Entity> generatedSurveyDrawing(std::size_t count)
{
    std::vector<katana::entity::Entity> entities;
    entities.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        const double t = static_cast<double>(i);
        // Scattered over 10 km x 8 km, which is the real corridor's extent.
        const double x = 300000.0 + std::fmod(t * 377.0, 10000.0);
        const double y = 6250000.0 + std::fmod(t * 911.0, 8000.0);
        katana::entity::Entity entity;
        entity.layer = "SURVEY";
        if (i % 2 == 0) {
            // A coded point with its height, as a survey import writes one.
            entity.geometry = katana::entity::PointGeometry{katana::geometry::Point2(x, y)};
            entity.properties.emplace(std::string(katana::entity::kElevationProperty),
                                      20.0 + std::fmod(t * 0.37, 15.0));
            entity.properties.emplace("code", std::string(i % 4 == 0 ? "TREE" : "PIT"));
        } else {
            // A string of 12 vertices, mostly tens of metres long; one in
            // sixteen is a long feature (a kerb, a fence) hundreds of metres
            // long, which is what puts boxes past a small cell size.
            const double step = (0.25 + std::fmod(t * 0.731, 4.0)) * (i % 32 == 1 ? 25.0 : 1.0);
            katana::geometry::Polyline2 polyline;
            polyline.vertices.reserve(12);
            for (int v = 0; v < 12; ++v) {
                const double s = static_cast<double>(v) * step;
                polyline.vertices.emplace_back(x + s, y + std::sin(t + v) * step * 0.5);
            }
            entity.geometry = std::move(polyline);
            entity.properties.emplace("code", std::string(i % 3 == 0 ? "KERB" : "FENCE"));
        }
        entities.push_back(std::move(entity));
    }
    return entities;
}

} // namespace katana::bench
