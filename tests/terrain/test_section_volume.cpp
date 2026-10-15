// Volumes from cross sections along an alignment (section_volume.hpp): the
// surface read along a segment, the area between two surfaces on a section,
// and the average end area and prismoidal volumes. The expected values are
// worked by hand in the comments; the wedge's are also the exact overlay's.
// Sections on a surface's boundary and sections that cross are at the end.

#include <gtest/gtest.h>

#include <algorithm>
#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <string>
#include <utility>
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

// The square [x0, x0 + size] x [y0, y0 + size] flat at height z: two triangles.
TinSurface slab(double x0, double y0, double size, double z)
{
    return buildFromPoints({Point3(x0, y0, z), Point3(x0 + size, y0, z),
                            Point3(x0 + size, y0 + size, z), Point3(x0, y0 + size, z)});
}

// An alignment through the points, each with a radius (0: a corner).
SolvedAlignment through(const std::vector<std::pair<Point2, double>>& pis)
{
    HorizontalAlignment definition;
    for (const auto& [at, radius] : pis) {
        definition.pis.push_back(AlignmentPI{at, radius});
    }
    auto solved = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(solved.ok()) << solved.error().describe();
    return std::move(solved).value();
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

// ---- sections that lie on a surface's boundary ---------------------------------------------

TEST(SegmentProfiler, ASegmentAlongABoundaryEdgeIsOnTheSurfaceWholeToWithinTheGeometricTolerance)
{
    // The square [0, 100]^2; a segment from x = -10 to x = 110 has 100 of its 120 on it.
    const TinSurface surface = slab(0.0, 0.0, 100.0, 5.0);
    const SegmentProfiler profiler(surface);
    const auto covered = [&](Point2 from, Point2 to) {
        double sum = 0.0;
        for (const SegmentPiece& piece : profiler.along(from, to)) {
            sum += piece.t1 - piece.t0;
        }
        return sum;
    };
    // Along the south edge y = 0 with the ends 5e-15 either side of it, as the
    // rounding of a bearing leaves them (cos(pi/2) = 6e-17 times a reach of
    // 141 m is 9e-15): the tilt crosses the edge at the middle, and half of the
    // segment used to be outside.
    EXPECT_NEAR(covered(Point2(-10, 5e-15), Point2(110, -5e-15)), 100.0 / 120.0, 1e-12);
    EXPECT_NEAR(covered(Point2(-10, -5e-15), Point2(110, 5e-15)), 100.0 / 120.0, 1e-12);
    // The same along the north edge y = 100, run the other way.
    EXPECT_NEAR(covered(Point2(110, 100.0 + 5e-14), Point2(-10, 100.0 - 5e-14)), 100.0 / 120.0,
                1e-12);
    // And the two edges that run up the page.
    EXPECT_NEAR(covered(Point2(5e-15, -10), Point2(-5e-15, 110)), 100.0 / 120.0, 1e-12);
    EXPECT_NEAR(covered(Point2(100.0 - 5e-14, 110), Point2(100.0 + 5e-14, -10)), 100.0 / 120.0,
                1e-12);
    // The tolerance is kGeometric, 1e-7: 5e-8 outside the edge is the edge, 5e-7
    // outside is off the surface, and 5e-7 inside is on it, as it always was.
    // Within it, not exactly: the covered piece may run past each end of the edge by up to
    // kGeometric, so 100 m +- 2e-7 of the segment's 120 is 100/120 +- 1.7e-9.
    EXPECT_NEAR(covered(Point2(-10, -5e-8), Point2(110, -5e-8)), 100.0 / 120.0, 2e-7 / 120.0);
    EXPECT_EQ(covered(Point2(-10, -5e-7), Point2(110, -5e-7)), 0.0);
    EXPECT_NEAR(covered(Point2(-10, 5e-7), Point2(110, 5e-7)), 100.0 / 120.0, 1e-12);
    // The neighbours of the case: a short segment wholly on the edge is wholly
    // on the surface, and one that only meets the corner has no length on it.
    EXPECT_NEAR(covered(Point2(40, 5e-15), Point2(60, -5e-15)), 1.0, 1e-12);
    EXPECT_NEAR(covered(Point2(-10, -10), Point2(0, 0)), 0.0, 1e-12);
}

TEST(VolumesAlongAlignment, ASlabMeasuredAlongAnyCardinalAlignmentIsWholeWhereItsEndSectionsLieOnItsBoundary)
{
    // A 1 m slab over [0, 100]^2 holds 100 x 100 x 1 = 10 000 m3, and every
    // section across it, 100 m wide, is 100 m2: the first and last lie ON the
    // slab's edges. The default reach (the diagonal, 141.4 m) takes each section
    // out past the slab on both sides. East has no tilt (sin 0 = 0); north,
    // south and west used to lose half of an end section, 500 m3 in all.
    const TinSurface base = slab(0.0, 0.0, 100.0, 10.0);
    const TinSurface top = slab(0.0, 0.0, 100.0, 11.0);
    const auto exact = compareSurfaces(base, top);
    ASSERT_TRUE(exact.ok());
    EXPECT_NEAR(exact->fill, 10000.0, 1e-9);
    struct Run {
        const char* name;
        Point2 from;
        Point2 to;
    };
    for (const Run& run : {Run{"north", Point2(50, 0), Point2(50, 100)},
                           Run{"south", Point2(50, 100), Point2(50, 0)},
                           Run{"east", Point2(0, 50), Point2(100, 50)},
                           Run{"west", Point2(100, 50), Point2(0, 50)}}) {
        AlignmentVolumeOptions options;
        options.interval = 10.0;
        const auto volumes = volumesAlongAlignment(base, top, straight(run.from, run.to), options);
        ASSERT_TRUE(volumes.ok()) << run.name << ": " << volumes.error().describe();
        ASSERT_EQ(volumes->sections.size(), 11u) << run.name;
        for (const StationSection& section : volumes->sections) {
            EXPECT_NEAR(section.width, 100.0, 1e-9) << run.name << " at " << section.station;
            EXPECT_NEAR(section.fillArea, 100.0, 1e-9) << run.name << " at " << section.station;
            EXPECT_EQ(section.cutArea, 0.0) << run.name;
        }
        EXPECT_NEAR(volumes->endAreaFill, 10000.0, 1e-6) << run.name;
        EXPECT_EQ(volumes->endAreaCut, 0.0) << run.name;
        EXPECT_NEAR(volumes->endAreaFill, exact->fill, 1e-6) << run.name;
    }
}

TEST(VolumesAlongAlignment, ASlabWhoseEndsAreCutByTheAlignmentItselfIsWholeAtEveryBearing)
{
    // The slab a corridor leaves: 80 m along an alignment of any bearing and 50
    // across, its four corners placed by the alignment's own pointAtStationOffset
    // as a corridor's are, so its end edges are the first and last sections to
    // rounding and not to the bit. 1 m deep: 80 x 50 x 1 = 4000 m3, by sections
    // of 50 m2 every 20 m. Every fifteenth degree, east, north, west and south
    // among them.
    for (int degrees = 0; degrees < 360; degrees += 15) {
        const double angle = static_cast<double>(degrees) * katana::math::kDegToRad;
        const Point2 start(30.0, 40.0);
        const SolvedAlignment alignment = straight(
            start, Point2(start.x + 80.0 * std::cos(angle), start.y + 80.0 * std::sin(angle)));
        const double length = alignment.length();
        const auto corner = [&](double station, double offset, double z) {
            const auto at = alignment.pointAtStationOffset(station, offset);
            return Point3(at->x, at->y, z);
        };
        const auto slabAt = [&](double z) {
            return buildFromPoints({corner(0.0, 25.0, z), corner(0.0, -25.0, z),
                                    corner(length, -25.0, z), corner(length, 25.0, z)});
        };
        AlignmentVolumeOptions options;
        options.interval = 20.0;
        options.left = 40.0;
        options.right = 40.0;
        const auto volumes = volumesAlongAlignment(slabAt(10.0), slabAt(11.0), alignment, options);
        ASSERT_TRUE(volumes.ok()) << degrees << ": " << volumes.error().describe();
        ASSERT_EQ(volumes->sections.size(), 5u) << degrees;
        for (const StationSection& section : volumes->sections) {
            EXPECT_NEAR(section.width, 50.0, 1e-9) << degrees << " at " << section.station;
            EXPECT_NEAR(section.fillArea, 50.0, 1e-9) << degrees << " at " << section.station;
        }
        EXPECT_NEAR(volumes->endAreaFill, 50.0 * length, 1e-6) << degrees;
    }
}

TEST(VolumesAlongAlignment, ASlabAtSurveyCoordinatesIsWholeOnTheCardinalBearingsToo)
{
    // UTM-sized coordinates, where the tilt of a section is below half an ulp
    // (9e-10 at 6e6) and the tolerance must neither lose ground nor invent it:
    // the slab [500000, 500100] x [6000000, 6000100] is 10 000 m3.
    const TinSurface base = slab(500000.0, 6000000.0, 100.0, 10.0);
    const TinSurface top = slab(500000.0, 6000000.0, 100.0, 11.0);
    struct Run {
        const char* name;
        Point2 from;
        Point2 to;
    };
    for (const Run& run : {Run{"north", Point2(500050, 6000000), Point2(500050, 6000100)},
                           Run{"south", Point2(500050, 6000100), Point2(500050, 6000000)},
                           Run{"east", Point2(500000, 6000050), Point2(500100, 6000050)},
                           Run{"west", Point2(500100, 6000050), Point2(500000, 6000050)}}) {
        const auto volumes =
            volumesAlongAlignment(base, top, straight(run.from, run.to), AlignmentVolumeOptions{});
        ASSERT_TRUE(volumes.ok()) << run.name << ": " << volumes.error().describe();
        EXPECT_NEAR(volumes->endAreaFill, 10000.0, 1e-5) << run.name;
        for (const StationSection& section : volumes->sections) {
            EXPECT_NEAR(section.width, 100.0, 1e-6) << run.name << " at " << section.station;
        }
    }
}

// ---- sections that cross ---------------------------------------------------------------------

TEST(VolumesAlongAlignment, SectionsAcrossACurveDeeperThanItsRadiusAreRefusedNotCountedTwice)
{
    // A 1 m slab over [0, 200]^2 is 40 000 m3 in all. The alignment (20, 20) to
    // (180, 20) to (180, 180) with a curve of radius 40 at the corner has its
    // tangent points 40 from the corner: 120 + (pi/2) 40 + 120 = 240 + 20 pi
    // = 302.832 m. With the default reach (the diagonal, 282.8 m) the sections
    // of the curve all pass through its centre, and the two tangents' sections
    // sweep the same 120 x 120 block: the sum was 62 101 m3, more than there is.
    const SolvedAlignment alignment =
        through({{Point2(20, 20), 0.0}, {Point2(180, 20), 40.0}, {Point2(180, 180), 0.0}});
    ASSERT_NEAR(alignment.length(), 240.0 + 20.0 * katana::math::kPi, 1e-9);
    const TinSurface base = slab(0.0, 0.0, 200.0, 10.0);
    const TinSurface top = slab(0.0, 0.0, 200.0, 11.0);

    AlignmentVolumeOptions options;
    options.interval = 10.0;
    const auto refused = volumesAlongAlignment(base, top, alignment, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("cross each other"), std::string::npos)
        << refused.error().message;
    // The reach that clears every crossing is the radius, to the millimetre.
    EXPECT_NE(refused.error().message.find("at most 40.000 m"), std::string::npos)
        << refused.error().message;
    // The same pair and the same reach whatever the thread count.
    katana::core::TaskPool serial(0);
    katana::core::TaskPool many(4);
    const auto onOne = volumesAlongAlignment(base, top, alignment, options, &serial);
    const auto onMany = volumesAlongAlignment(base, top, alignment, options, &many);
    ASSERT_FALSE(onOne.ok());
    ASSERT_FALSE(onMany.ok());
    EXPECT_EQ(onOne.error().message, refused.error().message);
    EXPECT_EQ(onMany.error().message, refused.error().message);

    // A reach one metre past the radius is refused as the default is.
    options.left = 41.0;
    options.right = 41.0;
    EXPECT_FALSE(volumesAlongAlignment(base, top, alignment, options).ok());
    // A reach of 15 either side is a band 30 m wide along the centreline: 30 x
    // 302.832 = 7200 + 600 pi = 9084.956 m3, the area of a band round a curve
    // being its width times its centreline's length (Pappus; the eccentricities
    // of the two sides cancel), whatever the interval.
    options.left = 15.0;
    options.right = 15.0;
    const auto band = volumesAlongAlignment(base, top, alignment, options);
    ASSERT_TRUE(band.ok()) << band.error().describe();
    EXPECT_NEAR(band->endAreaFill, 7200.0 + 600.0 * katana::math::kPi, 1e-6);
    EXPECT_LT(band->endAreaFill, 40000.0);
    for (const StationSection& section : band->sections) {
        EXPECT_NEAR(section.fillArea, 30.0, 1e-9) << section.station;
    }
    // A reach of exactly the radius on the inside ends every section of the
    // curve on its centre: touching, not crossing, and allowed.
    options.left = 40.0;
    options.right = 10.0;
    EXPECT_TRUE(volumesAlongAlignment(base, top, alignment, options).ok());
}

TEST(VolumesAlongAlignment, TwoStraightLegsAtARightAngleAreRefusedWithTheDefaultReach)
{
    // No curve at all: legs along y = 0 and x = 100, a corner with no radius.
    // The first leg's sections, 283 m long, cross the second leg's wherever
    // they meet, a long way from the corner.
    const SolvedAlignment alignment =
        through({{Point2(0, 0), 0.0}, {Point2(100, 0), 0.0}, {Point2(100, 50), 0.0}});
    const TinSurface base = slab(-200.0, -200.0, 500.0, 0.0);
    const TinSurface top = slab(-200.0, -200.0, 500.0, 1.0);
    const auto refused = volumesAlongAlignment(base, top, alignment, AlignmentVolumeOptions{});
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST(VolumesAlongAlignment, ACornerAllowsAReachUpToTheIntervalOverTheSineOfItsDeflection)
{
    // The corner at (50, 0) turns 90 degrees left. The section at station 40
    // (the vertical x = 40) and the key section at the corner, station 50 (the
    // horizontal y = 0, cut square to the leg after it) cross at (40, 0): on
    // the second 10 m from the centreline, 10 = interval / sin 90 degrees. With
    // the interval 10: a reach of 15 is refused, naming 10, and 10 is allowed -
    // the crossing is then at the section's end.
    const SolvedAlignment alignment =
        through({{Point2(0, 0), 0.0}, {Point2(50, 0), 0.0}, {Point2(50, 50), 0.0}});
    const TinSurface base = slab(-100.0, -100.0, 300.0, 0.0);
    const TinSurface top = slab(-100.0, -100.0, 300.0, 1.0);
    AlignmentVolumeOptions options;
    options.interval = 10.0;
    options.left = 15.0;
    options.right = 15.0;
    const auto refused = volumesAlongAlignment(base, top, alignment, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("at most 10.000 m"), std::string::npos)
        << refused.error().message;
    options.left = 10.0;
    options.right = 10.0;
    EXPECT_TRUE(volumesAlongAlignment(base, top, alignment, options).ok());
}

TEST(VolumesAlongAlignment, AStraightAlignmentWithTheDefaultReachIsNeverRefusedForCrossing)
{
    // Parallel sections never cross, whatever the reach or the interval: a
    // diagonal line, a section every half metre, each 141 m long.
    const TinSurface base = slab(0.0, 0.0, 100.0, 10.0);
    const TinSurface top = slab(0.0, 0.0, 100.0, 11.0);
    AlignmentVolumeOptions options;
    options.interval = 0.5;
    const auto volumes =
        volumesAlongAlignment(base, top, straight(Point2(10, 10), Point2(90, 70)), options);
    ASSERT_TRUE(volumes.ok()) << volumes.error().describe();
    EXPECT_GT(volumes->sections.size(), 150u);
}

// ---- nothing to measure ------------------------------------------------------------------------

TEST(VolumesAlongAlignment, SurfacesWhoseBoxesMeetButWhoseTrianglesDoNotAreRefusedNamingBothExtents)
{
    // a is the triangle (0,0) (60,0) (0,60), b the triangle (100,100) (40,100)
    // (100,40): their boxes meet on [40, 60]^2, and a lies below x + y = 60
    // while b lies above x + y = 140, so they share no ground. Every other
    // VOLUME form refuses such a pair; this answered a cut and a fill of 0.
    const TinSurface a = buildFromPoints({Point3(0, 0, 0), Point3(60, 0, 0), Point3(0, 60, 0)});
    const TinSurface b =
        buildFromPoints({Point3(100, 100, 1), Point3(40, 100, 1), Point3(100, 40, 1)});
    const auto refused = volumesAlongAlignment(a, b, straight(Point2(0, 50), Point2(100, 50)),
                                               AlignmentVolumeOptions{});
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(refused.error().message.find("0.000,0.000,60.000,60.000"), std::string::npos)
        << refused.error().message;
    EXPECT_NE(refused.error().message.find("40.000,40.000,100.000,100.000"), std::string::npos)
        << refused.error().message;
    // Boxes that do not meet name the extents too.
    const TinSurface distant = planeSurface(Plane{0, 0, 0}, 10.0, 5, 14, 500.0, 500.0);
    const auto apart = volumesAlongAlignment(a, distant, straight(Point2(0, 50), Point2(100, 50)),
                                             AlignmentVolumeOptions{});
    ASSERT_FALSE(apart.ok());
    EXPECT_EQ(apart.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(apart.error().message.find("500.000,500.000,510.000,510.000"), std::string::npos)
        << apart.error().message;
}

TEST(VolumesAlongAlignment, AnAlignmentThatRunsClearOfTheCommonGroundIsRefusedButOneOverPartOfItIsNot)
{
    // Both surfaces are [0, 10]^2. Along y = 100 no section reaches them (the
    // default reach is the diagonal of the common ground, 14.1 m): refused.
    // Along y = 5 from x = -10 to 10 the sections at x = -10 and -5 are off
    // the ground and those at 0, 5 and 10 are on it: a part is not a refusal,
    // the empty ones are counted by the verb as sections.empty.
    const TinSurface base = slab(0.0, 0.0, 10.0, 0.0);
    const TinSurface top = slab(0.0, 0.0, 10.0, 1.0);
    AlignmentVolumeOptions options;
    options.interval = 5.0;
    const auto clear =
        volumesAlongAlignment(base, top, straight(Point2(0, 100), Point2(10, 100)), options);
    ASSERT_FALSE(clear.ok());
    EXPECT_EQ(clear.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(clear.error().message.find("no section"), std::string::npos)
        << clear.error().message;

    const auto over =
        volumesAlongAlignment(base, top, straight(Point2(0, 5), Point2(10, 5)), options);
    ASSERT_TRUE(over.ok()) << over.error().describe();
    EXPECT_NEAR(over->endAreaFill, 100.0, 1e-9);

    const auto partly =
        volumesAlongAlignment(base, top, straight(Point2(-10, 5), Point2(10, 5)), options);
    ASSERT_TRUE(partly.ok()) << partly.error().describe();
    ASSERT_EQ(partly->sections.size(), 5u);
    EXPECT_EQ(partly->sections[0].width, 0.0);
    EXPECT_EQ(partly->sections[1].width, 0.0);
    EXPECT_NEAR(partly->sections[2].width, 10.0, 1e-9);
    EXPECT_NEAR(partly->sections[4].width, 10.0, 1e-9);
}

// ---- the first moments of a section ----------------------------------------------------------

TEST(SignedTrapezoid, MomentsAreTheIntegralOfDepthTimesOffsetAndSplitWhereTheSidesDifferInSign)
{
    // A strip 2 wide whose first side is 5 from the line, the offset falling to 3
    // at its far side, so o(u) = 5 - u. Depth 1 to 3, d(u) = 1 + u:
    //   integral of (1 + u)(5 - u) = (5 + 4u - u^2) over [0, 2] = 10 + 8 - 8/3 = 46/3.
    EXPECT_NEAR(signedTrapezoidMoments(1, 3, 2, 5).positive, 46.0 / 3.0, 1e-12);
    EXPECT_EQ(signedTrapezoidMoments(1, 3, 2, 5).negative, 0.0);
    // The same depths below zero are the same moment in the negative part, the
    // depth taken as a magnitude.
    EXPECT_NEAR(signedTrapezoidMoments(-1, -3, 2, 5).negative, 46.0 / 3.0, 1e-12);
    EXPECT_EQ(signedTrapezoidMoments(-1, -3, 2, 5).positive, 0.0);
    // Depth 2 to -2 crossing in the middle: the positive triangle is over u in
    // [0, 1] with d = 2 - 2u:
    //   (2 - 2u)(5 - u) = 10 - 12u + 2u^2, integrated over [0, 1]: 10 - 6 + 2/3 = 14/3;
    // the negative one over [1, 2] with the magnitude 2u - 2:
    //   (2u - 2)(5 - u) = -2u^2 + 12u - 10, integrated: [-2u^3/3 + 6u^2 - 10u] from
    //   1 to 2 = (-16/3 + 24 - 20) - (-2/3 + 6 - 10) = -4/3 + 14/3 = 10/3.
    EXPECT_NEAR(signedTrapezoidMoments(2, -2, 2, 5).positive, 14.0 / 3.0, 1e-12);
    EXPECT_NEAR(signedTrapezoidMoments(2, -2, 2, 5).negative, 10.0 / 3.0, 1e-12);
    // Mirrored, the parts swap sides: the negative one is first.
    EXPECT_NEAR(signedTrapezoidMoments(-2, 2, 2, 5).negative, 14.0 / 3.0, 1e-12);
    EXPECT_NEAR(signedTrapezoidMoments(-2, 2, 2, 5).positive, 10.0 / 3.0, 1e-12);
    // Each moment over its area is a centroid between the part's sides: the
    // positive triangle's is 1/3 of its base from the tall side, 5 - 1/3.
    const SignedArea areas = signedTrapezoid(2, -2, 2);
    EXPECT_NEAR(signedTrapezoidMoments(2, -2, 2, 5).positive / areas.positive, 5.0 - 1.0 / 3.0,
                1e-12);
    EXPECT_NEAR(signedTrapezoidMoments(2, -2, 2, 5).negative / areas.negative, 5.0 - 5.0 / 3.0,
                1e-12);
}

TEST(SignedTrapezoid, AStripOnTheOtherSideOfTheLineHasANegativeMomentAndACentredOneNone)
{
    // Depth 2 all across a strip 2 wide that starts at offset -1 and falls to -3:
    // integral of 2 (-1 - u) over [0, 2] = 2 (-2 - 2) = -8.
    EXPECT_NEAR(signedTrapezoidMoments(2, 2, 2, -1).positive, -8.0, 1e-12);
    // A strip centred on the line (first side at +1, last at -1) has no moment
    // whatever it is deep, and neither has any nothing.
    EXPECT_NEAR(signedTrapezoidMoments(1, 1, 2, 1).positive, 0.0, 1e-12);
    EXPECT_EQ(signedTrapezoidMoments(0, 0, 2, 5).positive, 0.0);
    EXPECT_EQ(signedTrapezoidMoments(0, 0, 2, 5).negative, 0.0);
    // No width, no moment; a width that is not a number likewise.
    EXPECT_EQ(signedTrapezoidMoments(2, 2, 0, 5).positive, 0.0);
    EXPECT_EQ(signedTrapezoidMoments(2, 2, -1, 5).positive, 0.0);
    EXPECT_EQ(signedTrapezoidMoments(2, 2, std::nan(""), 5).positive, 0.0);
}

TEST(SectionAreas, AreTheirFirstMomentsAboutTheLineTheOffsetIsMeasuredFrom)
{
    // A section 20 long from (0, 10) to (0, -10), so the offset of a point is
    // its y when the first end is given 10 from the line. The base is flat at
    // 0 and the design z = 0.1 y + 1: 2 deep at y = 10, 0 at y = -10. A triangle
    // of base 20 and height 2: area 20, and its centroid 1/3 of the base from
    // the deep end, y = 10 - 20/3 = 10/3, so the moment is 20 x 10/3 = 200/3.
    // Also by the integral: (0.1 y + 1) y over [-10, 10] = 0.1 x 2000/3 = 200/3.
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 100.0, 60, 31, -50.0, -50.0);
    const TinSurface top = planeSurface(Plane{0, 0.1, 1}, 100.0, 60, 32, -50.0, -50.0);
    const SegmentProfiler under(base);
    const SegmentProfiler over(top);
    const Point2 from(0, 10);
    const Point2 to(0, -10);
    const SectionAreas areas = sectionAreas(under.along(from, to), over.along(from, to), 20.0, 10.0);
    EXPECT_NEAR(areas.fill, 20.0, 1e-10);
    EXPECT_NEAR(areas.fillMoment, 200.0 / 3.0, 1e-9);
    // The design is 0 deep at the far end, where rounding may leave -1e-16.
    EXPECT_NEAR(areas.cut, 0.0, 1e-12);
    EXPECT_NEAR(areas.cutMoment, 0.0, 1e-11);
    // With the offset measured from the first end itself (the default) the offset
    // is -x along the section, and the centroid is 20/3 from the deep end, which is
    // the first: the moment is 20 x (-20/3) = -400/3. By the integral:
    // (2 - 0.1 x)(-x) over [0, 20] = -(400 - 0.1 x 8000/3) = -(400 - 800/3) = -400/3.
    const SectionAreas fromTheEnd = sectionAreas(under.along(from, to), over.along(from, to), 20.0);
    EXPECT_NEAR(fromTheEnd.fillMoment, -400.0 / 3.0, 1e-9);
    // Swapped, the design is the lower: all cut, the same magnitudes in the cut.
    const SectionAreas swapped = sectionAreas(over.along(from, to), under.along(from, to), 20.0, 10.0);
    EXPECT_NEAR(swapped.cut, 20.0, 1e-10);
    EXPECT_NEAR(swapped.cutMoment, 200.0 / 3.0, 1e-9);
    EXPECT_NEAR(swapped.fill, 0.0, 1e-12);
    EXPECT_NEAR(swapped.fillMoment, 0.0, 1e-11);
}

TEST(SectionAreas, ASectionWhereTheSurfacesCrossHasACutMomentOnOneSideAndAFillMomentOnTheOther)
{
    // The same section against z = 0.2 y: fill for y > 0 (depth 0.2 y, area
    // 0.5 x 10 x 2 = 10, moment the integral of 0.2 y^2 over [0, 10] = 200/3)
    // and cut for y < 0 (area 10 again; the magnitude 0.2 |y| times the offset
    // y, negative: -200/3).
    const TinSurface base = planeSurface(Plane{0, 0, 0}, 100.0, 60, 33, -50.0, -50.0);
    const TinSurface top = planeSurface(Plane{0, 0.2, 0}, 100.0, 60, 34, -50.0, -50.0);
    const SegmentProfiler under(base);
    const SegmentProfiler over(top);
    const Point2 from(0, 10);
    const Point2 to(0, -10);
    const SectionAreas areas = sectionAreas(under.along(from, to), over.along(from, to), 20.0, 10.0);
    EXPECT_NEAR(areas.fill, 10.0, 1e-10);
    EXPECT_NEAR(areas.cut, 10.0, 1e-10);
    EXPECT_NEAR(areas.fillMoment, 200.0 / 3.0, 1e-9);
    EXPECT_NEAR(areas.cutMoment, -200.0 / 3.0, 1e-9);
}

// ---- the curvature correction ---------------------------------------------------------------

namespace {

// Fifty metres of tangent east from (0, 0), a quarter circle of radius 50
// turning left about the centre (50, 50), fifty metres of tangent north to
// (100, 100): the PI is (100, 0), a 90 degree deflection, and the tangents
// are 50 long on each side of the curve's own 25 pi = 78.540.
SolvedAlignment quarterCircleLeft()
{
    return through({{Point2(0, 0), 0.0}, {Point2(100, 0), 50.0}, {Point2(100, 100), 0.0}});
}

// The same way back, from (100, 100) south and then west: it turns right about
// the same centre, so its inside is on the right.
SolvedAlignment quarterCircleRight()
{
    return through({{Point2(100, 100), 0.0}, {Point2(100, 0), 50.0}, {Point2(0, 0), 0.0}});
}

AlignmentVolumeOptions stripReach(double left, double right, double interval, bool curvature = true)
{
    AlignmentVolumeOptions options;
    options.interval = interval;
    options.left = left;
    options.right = right;
    options.curvature = curvature;
    return options;
}

} // namespace

TEST(VolumesAlongAlignment, AStripOnTheInsideOfAQuarterCircleIsTheAnnulusBetweenItsRadiiNotTheCentrelineArc)
{
    // A 1 m slab over [-50, 150]^2, sections reaching 10 m to the left (the
    // inside: the centre is at (50, 50), on the left of the way) and none to the
    // right: a strip 10 wide and 1 deep, so 10 m2 on every section, its centroid
    // 5 m left of the centreline, its first moment 50 m3.
    // Exactly: the tangents hold 50 x 10 = 500 m3 each, and between the radii 50
    // and 40 the quarter circle holds (pi/4)(50^2 - 40^2) = 225 pi = 706.858:
    // 1000 + 225 pi = 1706.858 in all. The centreline's arc is 25 pi = 78.540
    // long, so the plain end area, 10 m2 over its length, is 500 + 250 pi + 500 =
    // 1785.398 - 4.6 % over, since the strip's centroid travels (R - 5) theta and
    // not R theta (Pappus). Corrected a panel on the arc is L (A - k M) = L (10 -
    // 50/50) = 9 L, and 9 x 25 pi = 225 pi. The area is constant, so the end-area
    // rule itself has nothing to discretise and the interval cannot matter: the
    // sum is exact to the rounding of the areas, 1e-12 each.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    const SolvedAlignment alignment = quarterCircleLeft();
    ASSERT_NEAR(alignment.length(), 100.0 + 25.0 * katana::math::kPi, 1e-9);
    for (const double interval : {10.0, 1.0}) {
        const auto on = volumesAlongAlignment(base, top, alignment, stripReach(10.0, 0.0, interval));
        ASSERT_TRUE(on.ok()) << on.error().describe();
        EXPECT_TRUE(on->curvature);
        EXPECT_NEAR(on->endAreaFill, 1000.0 + 225.0 * katana::math::kPi, 1e-7) << interval;
        EXPECT_NEAR(on->endAreaShift.fill, -25.0 * katana::math::kPi, 1e-7) << interval;
        EXPECT_EQ(on->endAreaCut, 0.0);
        EXPECT_EQ(on->endAreaShift.cut, 0.0);
        for (const StationSection& section : on->sections) {
            EXPECT_NEAR(section.fillArea, 10.0, 1e-9) << section.station;
            EXPECT_NEAR(section.fillMoment, 50.0, 1e-9) << section.station;
        }
        // The curvature is the arc's 1/50 on the panels whose middle is between
        // its ends (TS at station 50, ST at 50 + 25 pi), and 0 on the tangents.
        bool onTheArc = false;
        bool onATangent = false;
        for (const SectionPanel& panel : on->panels) {
            const double middle = 0.5 * (panel.fromStation + panel.toStation);
            const bool arc = middle > 50.0 && middle < 50.0 + 25.0 * katana::math::kPi;
            EXPECT_NEAR(panel.curvature, arc ? 0.02 : 0.0, 1e-12) << middle;
            onTheArc = onTheArc || arc;
            onATangent = onATangent || !arc;
        }
        EXPECT_TRUE(onTheArc);
        EXPECT_TRUE(onATangent);

        // Off: the plain figure along the centreline, nothing shifted.
        const auto off =
            volumesAlongAlignment(base, top, alignment, stripReach(10.0, 0.0, interval, false));
        ASSERT_TRUE(off.ok()) << off.error().describe();
        EXPECT_FALSE(off->curvature);
        EXPECT_NEAR(off->endAreaFill, 1000.0 + 250.0 * katana::math::kPi, 1e-7) << interval;
        EXPECT_EQ(off->endAreaShift.fill, 0.0);
        for (const SectionPanel& panel : off->panels) {
            EXPECT_EQ(panel.curvature, 0.0);
        }
    }
}

TEST(VolumesAlongAlignment, AStripOnTheOutsideOfAQuarterCircleIsLongerThanItsCentrelineArc)
{
    // The same slab and way, the strip on the right, the outside: radii 50 to 60.
    // Exactly 1000 + (pi/4)(60^2 - 50^2) = 1000 + 275 pi = 1863.938, the plain
    // 1785.398 4.2 % under. The strip's centroid is 5 m right, M = -50, so a
    // panel on the arc is L (10 + 50/50) = 11 L, 11 x 25 pi = 275 pi.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    const auto outside =
        volumesAlongAlignment(base, top, quarterCircleLeft(), stripReach(0.0, 10.0, 5.0));
    ASSERT_TRUE(outside.ok()) << outside.error().describe();
    EXPECT_NEAR(outside->endAreaFill, 1000.0 + 275.0 * katana::math::kPi, 1e-7);
    EXPECT_NEAR(outside->endAreaShift.fill, 25.0 * katana::math::kPi, 1e-7);
    for (const StationSection& section : outside->sections) {
        EXPECT_NEAR(section.fillMoment, -50.0, 1e-9) << section.station;
    }
    // Prismoidal, with a section cut halfway between each pair, is the same
    // constant area through Simpson's rule and takes the same correction.
    AlignmentVolumeOptions both = stripReach(0.0, 10.0, 5.0);
    both.prismoidal = true;
    const auto prismoidal = volumesAlongAlignment(base, top, quarterCircleLeft(), both);
    ASSERT_TRUE(prismoidal.ok()) << prismoidal.error().describe();
    EXPECT_NEAR(prismoidal->prismoidalFill, 1000.0 + 275.0 * katana::math::kPi, 1e-7);
    EXPECT_NEAR(prismoidal->prismoidalShift.fill, 25.0 * katana::math::kPi, 1e-7);
}

TEST(VolumesAlongAlignment, AStripSymmetricAboutTheCentrelineIsTheSameWithOrWithoutTheCorrection)
{
    // 10 m each side: 20 m2, centroid on the centreline, M = 0. The curve holds
    // (pi/4)(60^2 - 40^2) = 500 pi, the tangents 50 x 20 = 1000 each: 2000 + 500 pi
    // = 3570.796, which the centreline arc (25 pi x 20 = 500 pi) gives as well.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    const auto on = volumesAlongAlignment(base, top, quarterCircleLeft(), stripReach(10.0, 10.0, 5.0));
    const auto off =
        volumesAlongAlignment(base, top, quarterCircleLeft(), stripReach(10.0, 10.0, 5.0, false));
    ASSERT_TRUE(on.ok()) << on.error().describe();
    ASSERT_TRUE(off.ok()) << off.error().describe();
    EXPECT_NEAR(on->endAreaFill, 2000.0 + 500.0 * katana::math::kPi, 1e-7);
    EXPECT_NEAR(off->endAreaFill, 2000.0 + 500.0 * katana::math::kPi, 1e-7);
    EXPECT_NEAR(on->endAreaShift.fill, 0.0, 1e-9);
    for (const StationSection& section : on->sections) {
        EXPECT_NEAR(section.fillMoment, 0.0, 1e-9) << section.station;
    }
}

TEST(VolumesAlongAlignment, TheSameStripOnAWayThatTurnsRightHasItsInsideOnTheRight)
{
    // From (100, 100) south to the PI (100, 0) and west to (0, 0): the centre
    // (50, 50) is on the right. The right strip is the inside, 1706.858, with its
    // centroid at offset -5 (negative: right) and the curvature -1/50; the left
    // strip is the outside, 1863.938.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    const auto inside = volumesAlongAlignment(base, top, quarterCircleRight(), stripReach(0.0, 10.0, 5.0));
    ASSERT_TRUE(inside.ok()) << inside.error().describe();
    EXPECT_NEAR(inside->endAreaFill, 1000.0 + 225.0 * katana::math::kPi, 1e-7);
    for (const StationSection& section : inside->sections) {
        EXPECT_NEAR(section.fillMoment, -50.0, 1e-9) << section.station;
    }
    const auto turning = std::count_if(inside->panels.begin(), inside->panels.end(),
                                       [](const SectionPanel& panel) {
                                           return std::abs(panel.curvature + 0.02) < 1e-12;
                                       });
    EXPECT_GT(turning, 5);
    const auto outside =
        volumesAlongAlignment(base, top, quarterCircleRight(), stripReach(10.0, 0.0, 5.0));
    ASSERT_TRUE(outside.ok()) << outside.error().describe();
    EXPECT_NEAR(outside->endAreaFill, 1000.0 + 275.0 * katana::math::kPi, 1e-7);
}

TEST(VolumesAlongAlignment, OnAStraightTheCorrectionChangesNoBitOfAnAsymmetricSection)
{
    // Curvature 0 everywhere: the panel is the plain expression, bit for bit, on
    // an asymmetric reach and with the middle sections of the prismoidal formula.
    const Wedge w = wedge();
    const SolvedAlignment alignment = straight(Point2(0, 0), Point2(100, 0));
    AlignmentVolumeOptions options;
    options.interval = 10.0;
    options.left = 5.0;
    options.right = 20.0;
    options.prismoidal = true;
    const auto on = volumesAlongAlignment(w.ground, w.top, alignment, options);
    options.curvature = false;
    const auto off = volumesAlongAlignment(w.ground, w.top, alignment, options);
    ASSERT_TRUE(on.ok()) << on.error().describe();
    ASSERT_TRUE(off.ok()) << off.error().describe();
    EXPECT_TRUE(sameBits(on->endAreaFill, off->endAreaFill));
    EXPECT_TRUE(sameBits(on->endAreaCut, off->endAreaCut));
    EXPECT_TRUE(sameBits(on->prismoidalFill, off->prismoidalFill));
    EXPECT_EQ(on->endAreaShift.fill, 0.0);
    EXPECT_EQ(on->prismoidalShift.fill, 0.0);
    // The wedge's sections are s/5 wide about the centreline, but only 5 of it
    // are reached to the left: the moments are real, the correction not applied.
    EXPECT_NE(on->sections.back().fillMoment, 0.0);
}

TEST(VolumesAlongAlignment, AStripAlongSpiralsAndACircleLosesItsMomentTimesTheTurnWhateverTheSpiralsDo)
{
    // Tangent east, a spiral of 30 m into a circle of radius 100 and one out of it,
    // tangent north: the alignment turns left by the deflection at the PI, 90 degrees,
    // however it is shared between the spirals and the circle. A strip 10 wide on
    // the inside (left), 1 m deep: A = 10, M = 50. The volume between two normals is
    // the integral of A - k M, so over the alignment A L - M (the angle turned) =
    // 10 L - 50 pi/2, and the plain figure less the corrected is M x pi/2 = 78.540
    // - which does not need the length at all. A panel's curvature is taken at its
    // middle, and curvature is linear on a spiral and constant on a circle, so
    // k(middle) L is the angle the panel turns exactly: the sum is the turn.
    HorizontalAlignment definition;
    definition.pis = {AlignmentPI{Point2(0, 0)}, AlignmentPI{Point2(200, 0), 100.0, 30.0, 30.0},
                      AlignmentPI{Point2(200, 200)}};
    const auto alignment = katana::geometry::solveAlignment(definition);
    ASSERT_TRUE(alignment.ok()) << alignment.error().describe();
    const TinSurface base = slab(-50.0, -50.0, 300.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 300.0, 1.0);
    AlignmentVolumeOptions options = stripReach(10.0, 0.0, 10.0);
    options.prismoidal = true;
    const auto on = volumesAlongAlignment(base, top, *alignment, options);
    options.curvature = false;
    const auto off = volumesAlongAlignment(base, top, *alignment, options);
    ASSERT_TRUE(on.ok()) << on.error().describe();
    ASSERT_TRUE(off.ok()) << off.error().describe();
    const double turn = katana::math::kPi / 2.0;
    EXPECT_NEAR(off->endAreaFill - on->endAreaFill, 50.0 * turn, 1e-6);
    EXPECT_NEAR(on->endAreaShift.fill, -50.0 * turn, 1e-6);
    EXPECT_NEAR(on->prismoidalShift.fill, -50.0 * turn, 1e-6);
    EXPECT_NEAR(on->endAreaFill, 10.0 * alignment->length() - 50.0 * turn, 1e-6);
}

TEST(VolumesAlongAlignment, ASlopingSurfaceOnACurveIsCorrectedToItsExactVolumeAsTheIntervalShrinks)
{
    // The ground z = 0, the design z = 0.02 x, over the left strip 10 wide of
    // the quarter circle. Exactly (integrating 0.02 x over the ground):
    //   tangent east, x in [0, 50], y in [0, 10]: 0.02 x 10 x 1250 = 250;
    //   tangent north, x in [90, 100], y in [50, 100]: 0.02 x 50 x 950 = 950;
    //   the arc, x = 50 + r cos(phi), r in [40, 50], phi in [-pi/2, 0]:
    //     per metre of station the corrected integrand is the integral over the
    //     offset o in [0, 10] of 0.02 (50 + (50 - o) cos phi)(1 - o/50) do
    //       = 0.02 (450 + 406.667 cos phi) = 9 + 8.1333 cos phi,
    //     and ds = 50 dphi: 50 (9 pi/2 + 8.1333) = 1113.525.
    // 2313.525 in all. The panel sum is the trapezoid rule on that integrand
    // (the area is a function of station, so the end-area rule is an
    // approximation): its error per panel of length h is h^3 F''/12, with
    // |F''| = 8.1333/50^2 |cos phi| <= 0.003253 and the arc 78.540 long, so
    // at most 78.540 x 0.003253 / 12 h^2 = 0.02129 h^2 over the whole arc;
    // the tangents, where the area is linear or constant in station, are exact.
    // Uncorrected, the integrand is the plain area 10 + 9 cos phi: 50 (5 pi + 9)
    // = 1235.398 on the arc, 2435.398 in all, 5.3 % over, with the same bound
    // (9/2500 for F'') 0.02356 h^2 about its own - and it never gets closer
    // to the exact figure, which is what the correction is for.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = planeSurface(Plane{0.02, 0.0, 0.0}, 200.0, 40, 35, -50.0, -50.0);
    const SolvedAlignment alignment = quarterCircleLeft();
    const double exact = 250.0 + 950.0 + 50.0 * (4.5 * katana::math::kPi + 8.0 + 2.0 / 15.0);
    EXPECT_NEAR(exact, 2313.525014, 1e-5); // the arithmetic above, 406.667 x 0.02 = 8 + 2/15
    double previous = 1e9;
    for (const double interval : {2.0, 1.0, 0.25}) {
        const auto on = volumesAlongAlignment(base, top, alignment, stripReach(10.0, 0.0, interval));
        ASSERT_TRUE(on.ok()) << on.error().describe();
        const double error = std::abs(on->endAreaFill - exact);
        EXPECT_LE(error, 0.0215 * interval * interval + 1e-8) << interval;
        EXPECT_LT(error, previous) << interval; // shrinking, as the rule says
        previous = error;
        const auto off =
            volumesAlongAlignment(base, top, alignment, stripReach(10.0, 0.0, interval, false));
        ASSERT_TRUE(off.ok()) << off.error().describe();
        EXPECT_LE(std::abs(off->endAreaFill - (1200.0 + 50.0 * (5.0 * katana::math::kPi + 9.0))),
                  0.0236 * interval * interval + 1e-8)
            << interval;
        EXPECT_GT(off->endAreaFill - exact, 100.0) << interval;
    }
    // Simpson's rule is exact to the fourth derivative: 5 m panels are within
    // 5^4 x 78.540 x (8.1333 / 50^4) / 2880 = 2.2e-5.
    AlignmentVolumeOptions options = stripReach(10.0, 0.0, 5.0);
    options.prismoidal = true;
    const auto prismoidal = volumesAlongAlignment(base, top, alignment, options);
    ASSERT_TRUE(prismoidal.ok()) << prismoidal.error().describe();
    EXPECT_NEAR(prismoidal->prismoidalFill, exact, 1e-4);
}

// ---- stretches that run side by side ---------------------------------------------------------

namespace {

// East 100 m, north 15 m, west 100 m: a U-turn of two corners, whose legs are
// 15 m apart. With a reach under 7.5 each way the strips of the two legs clear
// one another.
SolvedAlignment uTurn()
{
    return through({{Point2(0, 0), 0.0}, {Point2(100, 0), 0.0}, {Point2(100, 15), 0.0},
                    {Point2(0, 15), 0.0}});
}

} // namespace

TEST(VolumesAlongAlignment, TwoLegsOfAUTurnCloserThanTwiceTheReachAreRefusedNamingTheStretches)
{
    // The legs along y = 0 and y = 15, 15 apart; sections every 20 m with a
    // reach of 10 each side, so the strips y in [-10, 10] and [5, 25] share
    // y in [5, 10] over the 100 m they run alongside, 500 m2 swept twice. No
    // section crosses another (the legs' are all parallel, and the corners'
    // reach is under the interval), so this is the case the crossing test cannot
    // see; the volume was the sum, with the shared 5 m counted for each leg.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    AlignmentVolumeOptions options;
    options.interval = 20.0;
    options.left = 10.0;
    options.right = 10.0;
    const auto refused = volumesAlongAlignment(base, top, uTurn(), options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    const std::string& message = refused.error().message;
    EXPECT_NE(message.find("side by side"), std::string::npos) << message;
    // The first leg's stretch begins at the alignment's start and the second leg's
    // runs to its end, 100 + 15 + 100 = 215.
    EXPECT_NE(message.find("from station 0.000"), std::string::npos) << message;
    EXPECT_NE(message.find("to 215.000"), std::string::npos) << message;
    EXPECT_NE(message.find("left= and right="), std::string::npos) << message;
    EXPECT_EQ(message.find("cross each other"), std::string::npos) << message;
    EXPECT_EQ(refused.error().context, "left=,right=");
    // The same pair named whatever the thread count.
    katana::core::TaskPool serial(0);
    katana::core::TaskPool many(4);
    const auto onOne = volumesAlongAlignment(base, top, uTurn(), options, &serial);
    const auto onMany = volumesAlongAlignment(base, top, uTurn(), options, &many);
    ASSERT_FALSE(onOne.ok());
    ASSERT_FALSE(onMany.ok());
    EXPECT_EQ(onOne.error().message, message);
    EXPECT_EQ(onMany.error().message, message);
}

TEST(VolumesAlongAlignment, StripsThatOnlyTouchOrClearOneAnotherAreMeasured)
{
    // Half the distance between the legs is 7.5. At 7 each way the strips are 1 m
    // apart; at exactly 7.5 they touch along y = 7.5 and share no area; a hair
    // over - 1e-6 each way, 2e-6 of overlap, above the 1e-7 of kGeometric - they
    // overlap, and 1e-8 each way (2e-8) is under it and is touching.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    const auto with = [&](double reachEachWay) {
        AlignmentVolumeOptions options;
        options.interval = 20.0;
        options.left = reachEachWay;
        options.right = reachEachWay;
        return volumesAlongAlignment(base, top, uTurn(), options);
    };
    EXPECT_TRUE(with(7.0).ok());
    EXPECT_TRUE(with(7.5).ok());
    EXPECT_TRUE(with(7.5 + 1e-8).ok());
    const auto overlapping = with(7.5 + 1e-6);
    ASSERT_FALSE(overlapping.ok());
    EXPECT_NE(overlapping.error().message.find("side by side"), std::string::npos);
    EXPECT_FALSE(with(8.0).ok());
}

TEST(VolumesAlongAlignment, LegsWhoseStripsAreAsymmetricAreRefusedOnlyWhenTheStripsMeet)
{
    // The first leg runs east along y = 0 and the last west along y = 15. The left
    // of the first is north, toward the other leg, and the left of the last is
    // south, toward the first: left=12 reaches y = 12 on the first and y = 3 on the
    // last, which overlap in y = [3, 12]. The right of each is away from the other
    // (south of the first, north of the last): right=12 gives strips y = [-12, 0]
    // and [15, 27], apart.
    const TinSurface base = slab(-50.0, -50.0, 200.0, 0.0);
    const TinSurface top = slab(-50.0, -50.0, 200.0, 1.0);
    AlignmentVolumeOptions towards;
    towards.interval = 20.0;
    towards.left = 12.0;
    towards.right = 0.0;
    EXPECT_FALSE(volumesAlongAlignment(base, top, uTurn(), towards).ok());
    AlignmentVolumeOptions away;
    away.interval = 20.0;
    away.left = 0.0;
    away.right = 12.0;
    EXPECT_TRUE(volumesAlongAlignment(base, top, uTurn(), away).ok());
}
