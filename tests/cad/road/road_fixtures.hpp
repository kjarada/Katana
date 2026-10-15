#pragma once

// What the road model's tests are built on: a straight alignment along +x
// (station s is (s, 0), left is +y), a level design profile, a planar ground
// that a TIN of four corners reproduces exactly, and small templates whose
// sections are worked by hand in the tests.

#include <gtest/gtest.h>

#include <utility>
#include <vector>

#include "katana/cad/road/road_template.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/tin_builder.hpp"

namespace katana::cad::road::testing {

// z = a x + b y + c over [-100, 400] x [-yHalf, yHalf].
inline katana::terrain::TinSurface planeGround(double a, double b, double c, double yHalf = 100.0)
{
    katana::terrain::TinInput input;
    for (const auto& [x, y] : {std::pair{-100.0, -yHalf}, std::pair{400.0, -yHalf},
                               std::pair{400.0, yHalf}, std::pair{-100.0, yHalf}}) {
        input.points.emplace_back(x, y, a * x + b * y + c);
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built->surface;
}

inline katana::geometry::SolvedAlignment straight(double length = 200.0)
{
    katana::geometry::HorizontalAlignment definition;
    definition.pis = {katana::geometry::AlignmentPI{katana::geometry::Point2(0, 0)},
                      katana::geometry::AlignmentPI{katana::geometry::Point2(length, 0)}};
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok());
    return *solved;
}

inline katana::geometry::SolvedProfile level(double elevation, double from = 0.0, double to = 200.0)
{
    katana::geometry::VerticalAlignment definition;
    definition.pvis = {katana::geometry::ProfilePVI{from, elevation},
                       katana::geometry::ProfilePVI{to, elevation}};
    auto solved = katana::geometry::solveProfile(definition);
    EXPECT_TRUE(solved.ok());
    return *solved;
}

// A lane of `width` at `crossfall` percent each side with the given courses,
// then a batter of `cut` and `fill` run per rise.
inline RoadTemplate laneTemplate(double width, double crossfall,
                                 std::vector<PavementCourse> courses, double cut = 2.0,
                                 double fill = 2.0)
{
    RoadTemplate made;
    made.name = "test";
    for (const Side side : {Side::Left, Side::Right}) {
        Component lane = defaultComponent(ComponentKind::Lane, side);
        lane.width = width;
        lane.crossfall = crossfall;
        lane.courses = courses;
        made.components.push_back(lane);
        Component batter = defaultComponent(ComponentKind::Batter, side);
        batter.cutSlope = cut;
        batter.fillSlope = fill;
        made.components.push_back(batter);
    }
    return made;
}

} // namespace katana::cad::road::testing
