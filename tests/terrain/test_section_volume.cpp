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
    EXPECT_NEAR(covered(Point2(-10, -5e-8), Point2(110, -5e-8)), 100.0 / 120.0, 1e-12);
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
