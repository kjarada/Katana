// Corridor cross sections and earthwork quantities (include/katana/cad/corridor.hpp).
//
// Every area here was worked by hand from the trapezoids and triangles the
// template and the ground make, and confirmed independently by a fine
// numerical integration of ground minus design in Python - nothing shared
// with the trapezoid split in corridor.cpp. The ground is a plane, so the TIN
// reproduces it exactly and the only tolerance is the daylight bisection.

#include <gtest/gtest.h>

#include <algorithm>
#include <string>
#include <vector>

#include "katana/cad/corridor.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/terrain/volume.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::geometry::AlignmentPI;
using katana::geometry::HorizontalAlignment;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::ProfilePVI;
using katana::geometry::SolvedAlignment;
using katana::geometry::SolvedProfile;
using katana::geometry::VerticalAlignment;
using katana::terrain::TinSurface;

namespace {

// A planar ground z = a x + b y + c over [-100, 300] x [-yHalf, yHalf]. Four
// coplanar corners make two triangles that reproduce the plane exactly.
TinSurface planeGround(double a, double b, double c, double yHalf = 100.0)
{
    katana::terrain::TinInput input;
    for (const auto& [x, y] : {std::pair{-100.0, -yHalf}, std::pair{300.0, -yHalf},
                               std::pair{300.0, yHalf}, std::pair{-100.0, yHalf}}) {
        input.points.emplace_back(x, y, a * x + b * y + c);
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built->surface;
}

// 200 m straight along +x from the origin: station s is (s, 0), left is +y.
SolvedAlignment straight200()
{
    HorizontalAlignment definition;
    definition.pis = {AlignmentPI{Point2(0, 0)}, AlignmentPI{Point2(200, 0)}};
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok());
    return *solved;
}

SolvedProfile level(double elevation, double from = 0.0, double to = 200.0)
{
    VerticalAlignment definition;
    definition.pvis = {ProfilePVI{from, elevation}, ProfilePVI{to, elevation}};
    auto solved = katana::geometry::solveProfile(definition);
    EXPECT_TRUE(solved.ok());
    return *solved;
}

Assembly assembly(double halfWidth, double crossfall, double cutBatter, double fillBatter)
{
    Assembly a;
    a.halfWidth = halfWidth;
    a.crossfall = crossfall;
    a.cutBatter = cutBatter;
    a.fillBatter = fillBatter;
    return a;
}

} // namespace

// ---- one section, against hand-worked areas -------------------------------------

TEST(Corridor, ACutSectionOnFlatGroundIsTheTextbookTrapezoid)
{
    // Level design 1 m below flat ground, 8 m wide, 2:1 cut batters: each
    // batter runs 2 m out to daylight at +-6, and the cut is the trapezoid
    // (12 + 8) / 2 * 1 = 10 m^2.
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto section = corridorSection(straight200(), level(-1.0), assembly(4.0, 0.0, 2.0, 2.0),
                                         ground, 100.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_TRUE(section->complete);
    EXPECT_NEAR(section->cutArea, 10.0, 1e-9);
    EXPECT_NEAR(section->fillArea, 0.0, 1e-9);
    ASSERT_EQ(section->design.size(), 5u);
    EXPECT_NEAR(section->design.front().offset, 6.0, 1e-9);  // left daylight
    EXPECT_NEAR(section->design.front().elevation, 0.0, 1e-9);
    EXPECT_NEAR(section->design.back().offset, -6.0, 1e-9);  // right daylight
    EXPECT_NEAR(section->centre.x, 100.0, 1e-12);
    EXPECT_NEAR(section->designElevation, -1.0, 1e-12);
}

TEST(Corridor, AFillSectionMirrorsTheCut)
{
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto section = corridorSection(straight200(), level(1.0), assembly(4.0, 0.0, 2.0, 2.0),
                                         ground, 100.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_NEAR(section->fillArea, 10.0, 1e-9);
    EXPECT_NEAR(section->cutArea, 0.0, 1e-9);
    EXPECT_NEAR(section->design.front().offset, 6.0, 1e-9);
}

TEST(Corridor, CrossfallDropsTheEdgesAndWidensTheCut)
{
    // 5% crossfall over 4 m drops each edge 0.2 m to -1.2; the 2:1 batter
    // then needs 2.4 m to daylight at +-6.4. Batters 2 * (2.4 * 1.2 / 2) =
    // 2.88, carriageway 2 * (4 + 0.05 * 8) = 8.8, total 11.68 m^2.
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto section = corridorSection(straight200(), level(-1.0), assembly(4.0, 0.05, 2.0, 2.0),
                                         ground, 50.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_NEAR(section->cutArea, 11.68, 1e-9);
    EXPECT_NEAR(section->design.front().offset, 6.4, 1e-9);
    EXPECT_NEAR(section->design[1].elevation, -1.2, 1e-12); // the left edge
}

TEST(Corridor, SlopingGroundDaylightsAtADifferentWidthOnEachSide)
{
    // Ground rising 10% to the left (z = 0.1 y). Left edge is 1.4 m under
    // ground: a 2:1 batter rising 0.5/m against ground rising 0.1/m meets it
    // 3.5 m out, at offset 7.5, z = 0.75. Right edge is 0.6 m under: the
    // batter rises 0.5/m against ground FALLING 0.1/m, meeting 1 m out at
    // offset -5, z = -0.5. Area 0.3 + 8.0 + 2.45 = 10.75 m^2, all cut.
    const TinSurface ground = planeGround(0.0, 0.1, 0.0);
    const auto section = corridorSection(straight200(), level(-1.0), assembly(4.0, 0.0, 2.0, 2.0),
                                         ground, 100.0);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_TRUE(section->complete);
    EXPECT_NEAR(section->design.front().offset, 7.5, 1e-9);
    EXPECT_NEAR(section->design.front().elevation, 0.75, 1e-9);
    EXPECT_NEAR(section->design.back().offset, -5.0, 1e-9);
    EXPECT_NEAR(section->design.back().elevation, -0.5, 1e-9);
    EXPECT_NEAR(section->cutArea, 10.75, 1e-9);
    EXPECT_NEAR(section->fillArea, 0.0, 1e-9);
}

// ---- quantities along the alignment ----------------------------------------------

TEST(Corridor, AConstantSectionAlongAStraightGivesAreaTimesLengthExactly)
{
    // The end-area method is exact when the area does not change: 10 m^2
    // over 200 m is 2000 m^3 of cut, whatever the interval.
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    for (double interval : {200.0, 50.0, 7.0}) {
        const auto quantities = corridorQuantities(straight200(), level(-1.0),
                                                   assembly(4.0, 0.0, 2.0, 2.0), ground, interval);
        ASSERT_TRUE(quantities.ok()) << quantities.error().describe();
        EXPECT_NEAR(quantities->cut, 2000.0, 1e-6) << "interval " << interval;
        EXPECT_NEAR(quantities->fill, 0.0, 1e-9);
        EXPECT_NEAR(quantities->net, -2000.0, 1e-6) << "cut means material to remove";
        EXPECT_EQ(quantities->incompleteSections, 0u);
        EXPECT_EQ(quantities->startStation, 0.0);
        EXPECT_EQ(quantities->endStation, 200.0);
        EXPECT_GE(quantities->sections.size(), 2u);
        EXPECT_TRUE(std::is_sorted(quantities->sections.begin(), quantities->sections.end(),
                                   [](const auto& a, const auto& b) { return a.station < b.station; }));
    }
}

TEST(Corridor, TheProfileKeyStationsAreAlwaysSectioned)
{
    // A 40 m sag at 100: PVC 80 and PVT 120 are not multiples of the 50 m
    // interval and must be sectioned anyway, because the quantities change
    // character there and a report that stepped over them could not be
    // checked against the design.
    VerticalAlignment definition;
    definition.pvis = {ProfilePVI{0.0, -1.0}, ProfilePVI{100.0, -1.5, 40.0}, ProfilePVI{200.0, -1.0}};
    const auto profile = katana::geometry::solveProfile(definition);
    ASSERT_TRUE(profile.ok());
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto quantities = corridorQuantities(straight200(), *profile, assembly(4.0, 0.0, 2.0, 2.0),
                                               ground, 50.0);
    ASSERT_TRUE(quantities.ok()) << quantities.error().describe();
    std::vector<double> stations;
    for (const CorridorSection& section : quantities->sections) {
        stations.push_back(section.station);
    }
    for (double expected : {0.0, 50.0, 80.0, 100.0, 120.0, 150.0, 200.0}) {
        EXPECT_NE(std::find(stations.begin(), stations.end(), expected), stations.end()) << expected;
    }
    EXPECT_EQ(std::adjacent_find(stations.begin(), stations.end()), stations.end());
    // Deeper in the sag, so more cut than the level case.
    EXPECT_GT(quantities->cut, 2000.0);
}

TEST(Corridor, ASectionThatCannotReachTheGroundIsIncompleteAndExcludedFromTheTotal)
{
    // Ground only 3 m either side of the centreline: the batters need 6 m,
    // so every section runs off the surface. Nothing is invented for it - the
    // sections are marked, counted, and contribute no volume.
    const TinSurface ground = planeGround(0.0, 0.0, 0.0, 3.0);
    const auto quantities = corridorQuantities(straight200(), level(-1.0),
                                               assembly(4.0, 0.0, 2.0, 2.0), ground, 50.0);
    ASSERT_TRUE(quantities.ok()) << quantities.error().describe();
    EXPECT_EQ(quantities->incompleteSections, quantities->sections.size());
    EXPECT_GT(quantities->incompleteSections, 0u);
    EXPECT_EQ(quantities->cut, 0.0);
    EXPECT_EQ(quantities->fill, 0.0);
    for (const CorridorSection& section : quantities->sections) {
        EXPECT_FALSE(section.complete);
        EXPECT_EQ(section.cutArea, 0.0);
    }
}

TEST(Corridor, RefusesNonsenseAndNamesIt)
{
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto alignment = straight200();
    const auto profile = level(-1.0);
    const Assembly ok = assembly(4.0, 0.0, 2.0, 2.0);
    EXPECT_EQ(corridorQuantities(alignment, profile, ok, ground, 0.0).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(corridorSection(alignment, profile, ok, ground, 250.0).error().code,
              ErrorCode::InvalidArgument); // off the alignment
    EXPECT_EQ(corridorSection(alignment, level(-1.0, 0.0, 50.0), ok, ground, 100.0).error().code,
              ErrorCode::InvalidArgument); // off the profile
    EXPECT_EQ(corridorSection(alignment, profile, assembly(0.0, 0.0, 2.0, 2.0), ground, 100.0)
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(corridorSection(alignment, profile, assembly(4.0, 0.0, 0.0, 2.0), ground, 100.0)
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    // A profile entirely beyond the alignment: nothing overlaps.
    EXPECT_EQ(corridorQuantities(alignment, level(-1.0, 300.0, 400.0), ok, ground, 10.0)
                  .error()
                  .code,
              ErrorCode::InvalidGeometry);
}

// ---- the area arithmetic on its own ----------------------------------------------

TEST(Corridor, AreaBetweenSplitsAtACrossingAndIgnoresUncoveredIntervals)
{
    // Upper falls from +1 to -1 across a 2 m span over a flat lower line at 0:
    // half a square metre above, half below, split exactly at the crossing.
    const std::vector<SectionVertex> upper{{0.0, 1.0}, {2.0, -1.0}};
    const std::vector<SectionVertex> lower{{0.0, 0.0}, {2.0, 0.0}};
    const SplitArea split = areaBetween(upper, lower);
    EXPECT_NEAR(split.above, 0.5, 1e-12);
    EXPECT_NEAR(split.below, 0.5, 1e-12);

    // A constant 1 m gap over 10 m is 10 m^2, whichever way the vertices run.
    const std::vector<SectionVertex> high{{10.0, 1.0}, {0.0, 1.0}}; // right to left
    const std::vector<SectionVertex> low{{0.0, 0.0}, {10.0, 0.0}};
    EXPECT_NEAR(areaBetween(high, low).above, 10.0, 1e-12);
    EXPECT_NEAR(areaBetween(high, low).below, 0.0, 1e-12);

    // Where only one line reaches, nothing is counted rather than guessed.
    const std::vector<SectionVertex> shortLine{{0.0, 1.0}, {5.0, 1.0}};
    EXPECT_NEAR(areaBetween(shortLine, low).above, 5.0, 1e-12);
    EXPECT_EQ(areaBetween(std::vector<SectionVertex>{{0.0, 1.0}}, low).above, 0.0);
}

// ---- the corridor as a surface ---------------------------------------------------

TEST(Corridor, TheCorridorSurfaceAgreesWithTheEndAreaQuantities)
{
    // Two independent methods for the same volume, both exact on a straight
    // with a level design: end area over the sections, and the exact overlay
    // of the built corridor surface against the ground in compareSurfaces.
    // 2000 m^3 of cut over a 12 m by 200 m footprint, from both.
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto corridor = corridorSurface(straight200(), level(-1.0), assembly(4.0, 0.0, 2.0, 2.0),
                                          ground, 10.0);
    ASSERT_TRUE(corridor.ok()) << corridor.error().describe();
    EXPECT_EQ(corridor->incompleteSections, 0u);
    EXPECT_EQ(corridor->sections, 21u); // 0, 10, ..., 200
    EXPECT_GT(corridor->surface.triangleCount(), 0u);

    const auto comparison = katana::terrain::compareSurfaces(ground, corridor->surface);
    ASSERT_TRUE(comparison.ok()) << comparison.error().describe();
    EXPECT_NEAR(comparison->cut, 2000.0, 1e-6);  // ground above design
    EXPECT_NEAR(comparison->fill, 0.0, 1e-9);
    EXPECT_NEAR(comparison->planArea, 12.0 * 200.0, 1e-6);

    const auto quantities = corridorQuantities(straight200(), level(-1.0),
                                               assembly(4.0, 0.0, 2.0, 2.0), ground, 10.0);
    ASSERT_TRUE(quantities.ok());
    EXPECT_NEAR(quantities->cut, comparison->cut, 1e-6)
        << "end area and the exact overlay must agree where both are exact";
}

TEST(Corridor, TheCorridorSurfaceStaysWithinItsDaylightLines)
{
    // The daylight lines are the boundary, so the surface reaches exactly
    // +-6 m off the centreline and no further - nothing is hulled outside.
    const TinSurface ground = planeGround(0.0, 0.0, 0.0);
    const auto corridor = corridorSurface(straight200(), level(-1.0), assembly(4.0, 0.0, 2.0, 2.0),
                                          ground, 10.0);
    ASSERT_TRUE(corridor.ok());
    const auto& bounds = corridor->surface.bounds();
    EXPECT_NEAR(bounds.min.x, 0.0, 1e-9);
    EXPECT_NEAR(bounds.max.x, 200.0, 1e-9);
    EXPECT_NEAR(bounds.min.y, -6.0, 1e-9);
    EXPECT_NEAR(bounds.max.y, 6.0, 1e-9);
    EXPECT_NEAR(corridor->surface.minElevation(), -1.0, 1e-12);
    EXPECT_NEAR(corridor->surface.maxElevation(), 0.0, 1e-9);
}

TEST(Corridor, NoSurfaceIsBuiltWhenNoSectionReachesTheGround)
{
    const TinSurface ground = planeGround(0.0, 0.0, 0.0, 3.0);
    const auto corridor = corridorSurface(straight200(), level(-1.0), assembly(4.0, 0.0, 2.0, 2.0),
                                          ground, 50.0);
    ASSERT_FALSE(corridor.ok());
    EXPECT_EQ(corridor.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(corridor.error().describe().find("incomplete"), std::string::npos);
}

