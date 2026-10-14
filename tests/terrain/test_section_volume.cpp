// Volumes from cross sections along an alignment (section_volume.hpp): the
// surface read along a segment, the area between two surfaces on a section,
// and the average end area and prismoidal volumes. The expected values are
// worked by hand in the comments; the wedge's are also the exact overlay's.

#include <gtest/gtest.h>

#include <algorithm>
#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/terrain/section_volume.hpp"
#include "katana/terrain/volume.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::geometry::AlignmentPI;
using katana::geometry::HorizontalAlignment;
using katana::geometry::SolvedAlignment;
using katana::test::Random;

namespace {

SolvedAlignment straight(Point2 from, Point2 to)
{
    HorizontalAlignment definition;
    definition.pis = {AlignmentPI{from}, AlignmentPI{to}};
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok()) << solved.error().describe();
    return std::move(solved).value();
}

TinSurface planeSurface(const Plane& plane, double size, std::size_t interior, std::uint64_t seed,
                        double x0 = 0.0, double y0 = 0.0)
{
    Random random(seed);
    return buildFromPoints(planePoints(plane, size, interior, random, x0, y0));
}

// A wedge whose cross section grows in both width and depth along the
// alignment, so its area is quadratic in station: the ground z = 0 over
// [-10, 110] x [-20, 20], and over it the single triangle (0, 0), (100, -10),
// (100, 10) of the plane z = 0.1 x. At station s the wedge is s / 5 wide and
// 0.1 s deep: A(s) = s^2 / 50.
struct Wedge {
    TinSurface ground;
    TinSurface top;
};

Wedge wedge()
{
    Wedge w;
    w.ground = buildFromPoints({Point3(-10, -20, 0), Point3(110, -20, 0), Point3(110, 20, 0),
                                Point3(-10, 20, 0)});
    w.top = buildFromPoints({Point3(0, 0, 0), Point3(100, -10, 10), Point3(100, 10, 10)});
    return w;
}

bool sameBits(double a, double b)
{
    return std::bit_cast<std::uint64_t>(a) == std::bit_cast<std::uint64_t>(b);
}

} // namespace

// ---- along a segment ----------------------------------------------------------------------

TEST(SegmentProfiler, APlaneReadAlongASegmentIsThePlaneOnEveryPieceAndThePiecesMeetExactly)
{
    const Plane plane{0.3, -0.2, 50.0};
    const TinSurface surface = planeSurface(plane, 100.0, 400, 3);
    const SegmentProfiler profiler(surface);
    const Point2 from(3.0, 7.0);
    const Point2 to(96.0, 81.0);
    const auto pieces = profiler.along(from, to);
    ASSERT_GT(pieces.size(), 5u);
    EXPECT_EQ(pieces.front().t0, 0.0);
    EXPECT_EQ(pieces.back().t1, 1.0);
    for (std::size_t i = 0; i < pieces.size(); ++i) {
        const SegmentPiece& piece = pieces[i];
        EXPECT_LT(piece.t0, piece.t1);
        const auto at = [&](double t) {
            return plane.at(from.x + t * (to.x - from.x), from.y + t * (to.y - from.y));
        };
        // Heights to 80, read off planes of a plane: a few ulps.
        EXPECT_NEAR(piece.z0, at(piece.t0), 1e-11);
        EXPECT_NEAR(piece.z1, at(piece.t1), 1e-11);
        if (i + 1 < pieces.size()) {
            // The crossing of an edge is computed from the edge, so the
            // pieces either side of it meet at the same bits.
            EXPECT_TRUE(sameBits(piece.t1, pieces[i + 1].t0)) << i;
            EXPECT_TRUE(sameBits(piece.z1, pieces[i + 1].z0)) << i;
        }
    }
}

TEST(SegmentProfiler, OffTheSurfaceThereAreNoPiecesAndPartlyOnItOnlyThePartOn)
{
    const TinSurface surface = planeSurface(Plane{0, 0, 1}, 10.0, 10, 4);
    const SegmentProfiler profiler(surface);
    EXPECT_TRUE(profiler.along(Point2(20, 0), Point2(30, 5)).empty());
    EXPECT_TRUE(profiler.along(Point2(5, 5), Point2(5, 5)).empty()); // no length
    // From x = -10 to x = 10 along y = 5: on the surface from the middle on.
    const auto half = profiler.along(Point2(-10, 5), Point2(10, 5));
    ASSERT_FALSE(half.empty());
    EXPECT_NEAR(half.front().t0, 0.5, 1e-15);
    EXPECT_EQ(half.back().t1, 1.0);
}

// ---- one section ---------------------------------------------------------------------------

TEST(SignedTrapezoid, SplitsAtTheZeroWhereTheSidesDifferInSign)
{
    // Width 2: sides 1 and 3 make (1 + 3) / 2 x 2 = 4 positive; -1 and -3 as
    // much negative; 2 and -2 cross in the middle, two triangles of
    // 2 x 1 / 2 = 1 each.
    EXPECT_DOUBLE_EQ(signedTrapezoid(1, 3, 2).positive, 4.0);
    EXPECT_EQ(signedTrapezoid(1, 3, 2).negative, 0.0);
    EXPECT_DOUBLE_EQ(signedTrapezoid(-1, -3, 2).negative, 4.0);
    EXPECT_DOUBLE_EQ(signedTrapezoid(2, -2, 2).positive, 1.0);
    EXPECT_DOUBLE_EQ(signedTrapezoid(2, -2, 2).negative, 1.0);
    EXPECT_EQ(signedTrapezoid(2, 2, 0).positive, 0.0);
}

TEST(SectionAreas, TwoParallelPlanesOneMetreApartAcrossThirtyMetresAreThirtySquareMetres)
{
    const TinSurface base = planeSurface(Plane{0.02, 0.01, 10}, 100.0, 150, 5);
    const TinSurface top = planeSurface(Plane{0.02, 0.01, 11}, 100.0, 120, 6);
    const SegmentProfiler under(base);
    const SegmentProfiler over(top);
    const Point2 from(50, 20);
    const Point2 to(50, 50);
    const SectionAreas areas = sectionAreas(under.along(from, to), over.along(from, to), 30.0);
    EXPECT_NEAR(areas.fill, 30.0, 1e-11);
    EXPECT_NEAR(areas.cut, 0.0, 1e-12);
    EXPECT_NEAR(areas.width, 30.0, 1e-12);
}

TEST(SectionAreas, WhereOnlyOneSurfaceHasAHeightNothingIsMeasured)
{
    // The top covers x up to 10 of a section 20 long: half of it.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 20.0, 10, 7);
    const TinSurface top = planeSurface(Plane{0, 0, 2}, 10.0, 10, 8);
    const SegmentProfiler under(base);
    const SegmentProfiler over(top);
    const SectionAreas areas =
        sectionAreas(under.along(Point2(0, 5), Point2(20, 5)), over.along(Point2(0, 5), Point2(20, 5)),
                     20.0);
    EXPECT_NEAR(areas.width, 10.0, 1e-12);
    EXPECT_NEAR(areas.fill, 20.0, 1e-11);
}

// ---- along an alignment ------------------------------------------------------------------------

TEST(VolumesAlongAlignment, OnAWedgeThePrismoidalFormulaIsExactAndTheEndAreaOverstatesIt)
{
    // A(s) = s^2 / 50 from 0 to 100 (see wedge()). Exactly, V = 100^3 / 150 =
    // 6666.667 m^3 - which the exact overlay also gives: 0.1 x the triangle's
    // area 1000 x its centroid's x 200 / 3. On sections 10 apart:
    //   prismoidal: Simpson's rule on a quadratic, exact - 20 000 / 3;
    //   average end area: 10 sum (A_i + A_i+1) / 2 = 10 sum (i^2 + (i+1)^2)
    //   for i = 0..9 = 10 (285 + 385) = 6700, the trapezoid rule's
    //   (b - a) h^2 A'' / 12 = 100 x 100 x (1/25) / 12 = 33.333 over.
    const Wedge w = wedge();
    const SolvedAlignment alignment = straight(Point2(0, 0), Point2(100, 0));
    AlignmentVolumeOptions options;
    options.interval = 10.0;
    options.left = 20.0;
    options.right = 20.0;
    options.prismoidal = true;
    const auto volumes = volumesAlongAlignment(w.ground, w.top, alignment, options);
    ASSERT_TRUE(volumes.ok()) << volumes.error().describe();
    ASSERT_EQ(volumes->sections.size(), 11u);
    ASSERT_EQ(volumes->panels.size(), 10u);
    for (std::size_t i = 0; i < volumes->sections.size(); ++i) {
        const StationSection& section = volumes->sections[i];
        const double s = 10.0 * static_cast<double>(i);
        EXPECT_NEAR(section.station, s, 1e-12);
        EXPECT_NEAR(section.fillArea, s * s / 50.0, 1e-10) << s;
        EXPECT_NEAR(section.width, s / 5.0, 1e-12) << s;
        EXPECT_EQ(section.cutArea, 0.0);
    }
    EXPECT_NEAR(volumes->endAreaFill, 6700.0, 1e-9);
    EXPECT_NEAR(volumes->prismoidalFill, 20000.0 / 3.0, 1e-9);
    EXPECT_EQ(volumes->endAreaCut, 0.0);
    // The middle section of the first panel, at 5: 25 / 50.
    ASSERT_TRUE(volumes->panels.front().middle.has_value());
    EXPECT_NEAR(volumes->panels.front().middle->fillArea, 0.5, 1e-12);

    const auto exact = compareSurfaces(w.ground, w.top);
    ASSERT_TRUE(exact.ok());
    EXPECT_NEAR(exact->fill, 20000.0 / 3.0, 1e-9);
    EXPECT_NEAR(volumes->prismoidalFill, exact->fill, 1e-9);
}

TEST(VolumesAlongAlignment, TwoParallelPlanesGiveTheSlabByEitherMethod)
{
    // 1 m apart, sections 20 m across, 100 m: 2000 m^3, and with a constant
    // area the two formulas agree exactly.
    const TinSurface base = planeSurface(Plane{0.02, -0.01, 30}, 140.0, 200, 9, -20.0, -70.0);
    const TinSurface top = planeSurface(Plane{0.02, -0.01, 31}, 140.0, 180, 10, -20.0, -70.0);
    const SolvedAlignment alignment = straight(Point2(0, 0), Point2(100, 0));
    AlignmentVolumeOptions options;
    options.interval = 25.0;
    options.left = 10.0;
    options.right = 10.0;
    options.prismoidal = true;
    const auto volumes = volumesAlongAlignment(base, top, alignment, options);
    ASSERT_TRUE(volumes.ok()) << volumes.error().describe();
    EXPECT_EQ(volumes->sections.size(), 5u);
    EXPECT_NEAR(volumes->endAreaFill, 2000.0, 1e-9);
    EXPECT_NEAR(volumes->prismoidalFill, 2000.0, 1e-9);
    EXPECT_NEAR(volumes->endAreaCut, 0.0, 1e-12);
}

TEST(VolumesAlongAlignment, SectionsAreCutAtTheAlignmentsKeyStationsToo)
{
    // A curve of radius 30 at the middle PI: its tangent points are key
    // stations, and every one of them inside the range has a section, marked
    // key, besides the round tens.
    HorizontalAlignment definition;
    definition.pis = {AlignmentPI{Point2(0, 0)}, AlignmentPI{Point2(60, 0), 30.0},
                      AlignmentPI{Point2(60, 60)}};
    const auto alignment = katana::geometry::solveAlignment(definition);
    ASSERT_TRUE(alignment.ok()) << alignment.error().describe();
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 200.0, 20, 11, -50.0, -50.0);
    const TinSurface top = planeSurface(Plane{0, 0, 1}, 200.0, 20, 12, -50.0, -50.0);
    AlignmentVolumeOptions options;
    options.left = 5.0;
    options.right = 5.0;
    const auto volumes = volumesAlongAlignment(base, top, *alignment, options);
    ASSERT_TRUE(volumes.ok()) << volumes.error().describe();
    for (const double key : alignment->keyStations()) {
        const bool found = std::any_of(volumes->sections.begin(), volumes->sections.end(),
                                       [&](const StationSection& section) {
                                           return std::abs(section.station - key) < 1e-9 &&
                                                  section.key;
                                       });
        EXPECT_TRUE(found) << "key station " << key;
    }
    // A slab 1 deep and 10 wide all along the centreline: 10 m^2 on every
    // section, and on a curve's inside and outside the sections still have
    // that area, so end areas give 10 x the length.
    for (const StationSection& section : volumes->sections) {
        EXPECT_NEAR(section.fillArea, 10.0, 1e-9) << section.station;
    }
    EXPECT_NEAR(volumes->endAreaFill, 10.0 * alignment->length(), 1e-7);
}

TEST(VolumesAlongAlignment, AFromAndToRangeIsMeasuredAlone)
{
    const Wedge w = wedge();
    const SolvedAlignment alignment = straight(Point2(0, 0), Point2(100, 0));
    AlignmentVolumeOptions options;
    options.interval = 10.0;
    options.from = 50.0;
    options.to = 100.0;
    options.left = 20.0;
    options.right = 20.0;
    options.prismoidal = true;
    const auto volumes = volumesAlongAlignment(w.ground, w.top, alignment, options);
    ASSERT_TRUE(volumes.ok()) << volumes.error().describe();
    // The integral of s^2 / 50 from 50 to 100: (100^3 - 50^3) / 150 = 5833.333.
    EXPECT_NEAR(volumes->prismoidalFill, (1e6 - 125000.0) / 150.0, 1e-9);
    EXPECT_NEAR(volumes->sections.front().station, 50.0, 1e-12);
    EXPECT_TRUE(volumes->sections.front().key);
}

TEST(VolumesAlongAlignment, RefusesWhatCannotBeMeasured)
{
    const Wedge w = wedge();
    const SolvedAlignment alignment = straight(Point2(0, 0), Point2(100, 0));
    const auto refused = [&](AlignmentVolumeOptions options, ErrorCode code) {
        const auto result = volumesAlongAlignment(w.ground, w.top, alignment, options);
        ASSERT_FALSE(result.ok());
        EXPECT_EQ(result.error().code, code) << result.error().describe();
    };
    AlignmentVolumeOptions zero;
    zero.interval = 0.0;
    refused(zero, ErrorCode::InvalidArgument);
    AlignmentVolumeOptions backwards;
    backwards.from = 80.0;
    backwards.to = 20.0;
    refused(backwards, ErrorCode::InvalidArgument);
    AlignmentVolumeOptions beyond;
    beyond.to = 150.0;
    refused(beyond, ErrorCode::InvalidArgument);
    AlignmentVolumeOptions negative;
    negative.left = -1.0;
    refused(negative, ErrorCode::InvalidArgument);
    AlignmentVolumeOptions none;
    none.left = 0.0;
    none.right = 0.0;
    refused(none, ErrorCode::InvalidArgument);
    AlignmentVolumeOptions tooMany;
    tooMany.interval = 1e-4; // 100 m at 0.1 mm: a million sections
    refused(tooMany, ErrorCode::InvalidArgument);

    const TinSurface far = planeSurface(Plane{0, 0, 0}, 10.0, 5, 13, 500.0, 500.0);
    const auto apart = volumesAlongAlignment(w.ground, far, alignment, AlignmentVolumeOptions{});
    ASSERT_FALSE(apart.ok());
    EXPECT_EQ(apart.error().code, ErrorCode::InvalidGeometry);
    const auto empty =
        volumesAlongAlignment(w.ground, TinSurface{}, alignment, AlignmentVolumeOptions{});
    ASSERT_FALSE(empty.ok());
    EXPECT_EQ(empty.error().code, ErrorCode::InvalidArgument);
}

TEST(VolumesAlongAlignment, GivesTheSameBitsOnOneThreadAsOnMany)
{
    Random random(91);
    TinInput rough;
    for (int i = 0; i < 2000; ++i) {
        rough.points.emplace_back(random.real(-50, 150), random.real(-50, 50), random.real(0, 4));
    }
    const TinSurface base = build(rough).surface;
    const TinSurface top = planeSurface(Plane{0, 0, 2}, 200.0, 50, 92, -50.0, -100.0);
    const SolvedAlignment alignment = straight(Point2(0, 0), Point2(100, 0));
    AlignmentVolumeOptions options;
    options.interval = 2.0;
    options.left = 30.0;
    options.right = 30.0;
    options.prismoidal = true;
    katana::core::TaskPool serial(0);
    const auto one = volumesAlongAlignment(base, top, alignment, options, &serial);
    ASSERT_TRUE(one.ok()) << one.error().describe();
    for (const std::size_t workers : {1u, 4u}) {
        katana::core::TaskPool pool(workers);
        const auto many = volumesAlongAlignment(base, top, alignment, options, &pool);
        ASSERT_TRUE(many.ok());
        EXPECT_TRUE(sameBits(one->endAreaCut, many->endAreaCut));
        EXPECT_TRUE(sameBits(one->endAreaFill, many->endAreaFill));
        EXPECT_TRUE(sameBits(one->prismoidalCut, many->prismoidalCut));
        EXPECT_TRUE(sameBits(one->prismoidalFill, many->prismoidalFill));
    }
}
