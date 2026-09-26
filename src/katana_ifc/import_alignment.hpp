#pragma once

// IFC 4.3 alignment business logic -> Katana's PI and PVI definitions, and
// the exact geometry of what has no such definition. Internal to katana_ifc;
// import.cpp reads the segments out of the file (in project coordinates,
// metres and radians) and hands them here.

#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/math/vec2.hpp"

namespace katana::ifc::detail {

struct HorizontalSegment {
    std::string type; // IfcAlignmentHorizontalSegmentTypeEnum: "LINE", "CLOTHOID" ...
    katana::math::Vec2 start{};
    double direction = 0.0;      // radians counter-clockwise from +x
    double startCurvature = 0.0; // signed, positive turning left; 1 / StartRadiusOfCurvature
    double endCurvature = 0.0;
    double length = 0.0;
};

struct VerticalSegment {
    std::string type;           // IfcAlignmentVerticalSegmentTypeEnum
    double startDistance = 0.0; // along the horizontal, from its start
    double length = 0.0;        // horizontal length
    double startHeight = 0.0;
    double startGrade = 0.0;
    double endGrade = 0.0;
};

// The PI definition whose solution is these segments, or nullopt with
// `why` said. Only LINE, CIRCULARARC and CLOTHOID have a PI form in Katana;
// zero-length segments are passed over. The caller checks the result by
// solving it (checkHorizontal).
[[nodiscard]] std::optional<geometry::HorizontalAlignment>
reconstructHorizontal(const std::vector<HorizontalSegment>& segments, double startStation,
                      std::string& why);

// The largest distance between where a segment of the file starts and where
// the solved definition puts that station.
[[nodiscard]] double checkHorizontal(const geometry::SolvedAlignment& solved,
                                     const std::vector<HorizontalSegment>& segments);

// The PVI definition whose solution is these segments (CONSTANTGRADIENT and
// PARABOLICARC only), stations counted from `startStation` at distance 0.
[[nodiscard]] std::optional<geometry::VerticalAlignment>
reconstructVertical(const std::vector<VerticalSegment>& segments, double startStation,
                    std::string& why);

[[nodiscard]] double checkVertical(const geometry::SolvedProfile& solved,
                                   const std::vector<VerticalSegment>& segments,
                                   double startStation);

// The horizontal as points no further than `tolerance` from the curve, each
// with its distance along; transitions other than the clothoid are
// integrated from their curvature, which is what defines them.
[[nodiscard]] std::vector<std::pair<double, katana::math::Vec2>>
sampleHorizontal(const std::vector<HorizontalSegment>& segments, double tolerance);

// The profile's height at `distance` along the horizontal; nullopt outside it.
[[nodiscard]] std::optional<double> heightAt(const std::vector<VerticalSegment>& segments,
                                             double distance);

} // namespace katana::ifc::detail
