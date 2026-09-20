#include <gtest/gtest.h>

#include <cmath>
#include <cstring>
#include <limits>
#include <string>
#include <thread>
#include <vector>

#include "geodesy_test_support.hpp"
#include "katana/geodesy/coordinate_transformer.hpp"
#include "katana/math/numerics.hpp"
#include "support/property.hpp"

// Provenance of expected values in this file
//   [identity]  exact mathematical property of the CRS definition
//   [oracle]    formula coded inside the tests (geodesy_test_support.hpp)
//   [published] worked example of an authority, quoted with its printed precision
//   [self]      PROJ self-consistency only - says nothing about absolute truth
// Every assertion on a coordinate value carries one of these tags.

using namespace katana::geodesy;
using namespace katana::geodesy::test;
using katana::core::ErrorCode;
using katana::test::kPropertyIterations;
using katana::test::Random;
namespace tol = katana::math::tolerance;

namespace {

constexpr int kWgs84 = 4326;
constexpr int kUtm30N = 32630;

TransverseMercator utmZone30()
{
    TransverseMercator tm;
    tm.centralMeridianDeg = -3.0; // zone 30 spans 6 W .. 0
    return tm;
}

// Bit pattern equality: distinguishes -0.0 from 0.0 and compares NaN payloads,
// which operator== cannot.
bool sameBits(double a, double b)
{
    return std::memcmp(&a, &b, sizeof(double)) == 0;
}

} // namespace

// ---- regression: known control values ----------------------------------------------

TEST(GeodesyTransformer, Utm30NCentralMeridianMapsToFalseEasting)
{
    // [identity] On the central meridian eta = 0, hence E = false easting exactly;
    // on the equator xi = 0, hence N = 0 exactly.
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    const auto origin = transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{0.0, -3.0});
    ASSERT_OK(origin);
    EXPECT_NEAR(origin->easting, 500000.0, tol::kGeometric);
    EXPECT_NEAR(origin->northing, 0.0, tol::kGeometric);

    for (const double latitude : {-80.0, -33.5, 12.25, 47.0, 84.0}) {
        const auto onMeridian =
            transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{latitude, -3.0});
        ASSERT_OK(onMeridian);
        EXPECT_NEAR(onMeridian->easting, 500000.0, tol::kGeometric) << "latitude " << latitude;
    }
}

TEST(GeodesyTransformer, Utm30NEquatorAndMeridianSymmetry)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double latitude = random.real(0.0, 84.0);
        const double offset = random.real(0.0, 3.0);

        // [identity] Points of the equator have northing 0.
        const auto equator =
            transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{0.0, -3.0 + offset});
        ASSERT_OK(equator);
        EXPECT_NEAR(equator->northing, 0.0, tol::kGeometric);

        // [identity] Mirror images about the central meridian: eastings
        // symmetric about 500 000, equal northings.
        const auto east =
            transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{latitude, -3.0 + offset});
        const auto west =
            transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{latitude, -3.0 - offset});
        ASSERT_OK(east);
        ASSERT_OK(west);
        EXPECT_NEAR(east->easting - 500000.0, 500000.0 - west->easting, tol::kGeometric);
        EXPECT_NEAR(east->northing, west->northing, tol::kGeometric);

        // [identity] Mirror images about the equator (false northing is 0 here).
        const auto south =
            transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{-latitude, -3.0 + offset});
        ASSERT_OK(south);
        EXPECT_NEAR(south->northing, -east->northing, tol::kGeometric);
        EXPECT_NEAR(south->easting, east->easting, tol::kGeometric);
    }
}

TEST(GeodesyTransformer, Wgs84OriginInUtm30NMatchesKruegerSeries)
{
    // The point the removed placeholder got wrong: it "transformed" (0 N, 0 E) to
    // easting 0. The truth is 3 degrees east of the zone's central meridian.
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    const auto grid = transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{0.0, 0.0});
    ASSERT_OK(grid);

    // [published] 833 978.557 m, quoted to the millimetre.
    EXPECT_NEAR(grid->easting, 833978.557, halfUnit(0.001));
    EXPECT_NEAR(grid->northing, 0.0, tol::kGeometric); // [identity]

    // [oracle] Krueger series evaluated here, independent of PROJ. First the
    // oracle itself is pinned to the published millimetre, then the library is
    // compared with the oracle far below the millimetre.
    const GridPosition expected = krugerForward(utmZone30(), 0.0, 0.0);
    EXPECT_NEAR(expected.easting, 833978.557, halfUnit(0.001));
    EXPECT_NEAR(grid->easting, expected.easting, tol::kGeometric);
    EXPECT_NEAR(grid->northing, expected.northing, tol::kGeometric);
}

TEST(GeodesyTransformerProperty, Utm30NAgreesWithKruegerSeriesAcrossTheZone)
{
    // [oracle] The whole zone, and well beyond its 3 degree half-width, to 0.1
    // micrometre. The two implementations share no code: PROJ uses the
    // Engsager-Poder formulation, the oracle Karney's.
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    Random random;
    double worst = 0.0;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double latitude = random.real(-80.0, 84.0);
        const double longitude = random.real(-9.0, 3.0);
        const auto grid =
            transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{latitude, longitude});
        ASSERT_OK(grid);
        // The EPSG definition of zone 30N has false northing 0 on both hemispheres.
        const GridPosition expected = krugerForward(utmZone30(), latitude, longitude);
        EXPECT_NEAR(grid->easting, expected.easting, tol::kGeometric)
            << "lat " << latitude << " lon " << longitude;
        EXPECT_NEAR(grid->northing, expected.northing, tol::kGeometric)
            << "lat " << latitude << " lon " << longitude;
        worst = std::max({worst, std::abs(grid->easting - expected.easting),
                          std::abs(grid->northing - expected.northing)});
    }
    RecordProperty("worst_difference_metres", std::to_string(worst));
}

TEST(GeodesyTransformer, OrdnanceSurveyWorkedExample)
{
    // [published] Ordnance Survey, "A guide to coordinate systems in Great
    // Britain", worked example of the National Grid projection (Airy 1830):
    //   52 39' 27.2531" N, 1 43' 4.5177" E  ->  E 651 409.903 m, N 313 177.270 m.
    // OSGB36 geographic -> British National Grid is a pure map projection, so no
    // datum transformation (and no grid file) is involved.
    auto transformer = makeTransformer(4277, 27700);
    ASSERT_OK(transformer);
    EXPECT_FALSE(transformer->operation().isBallpark);
    const GeographicCoordinate position{dms(52, 39, 27.2531), dms(1, 43, 4.5177)};
    const auto grid = transformer->forwardAs<ProjectedCoordinate>(position);
    ASSERT_OK(grid);
    EXPECT_NEAR(grid->easting, 651409.903, halfUnit(0.001));
    EXPECT_NEAR(grid->northing, 313177.270, halfUnit(0.001));

    // And back: the published latitude/longitude carry 0.0001" = 2.8e-8 degrees.
    const auto back = transformer->inverseAs<GeographicCoordinate>(
        ProjectedCoordinate{651409.903, 313177.270});
    ASSERT_OK(back);
    EXPECT_NEAR(back->latitude, position.latitude, halfUnit(0.0001 / 3600.0));
    EXPECT_NEAR(back->longitude, position.longitude, halfUnit(0.0001 / 3600.0));
}

TEST(GeodesyTransformer, EpsgGuidanceNoteLambertConformalTwoParallelsInUsSurveyFeet)
{
    // [published] IOGP Guidance Note 7-2, Lambert Conic Conformal (2SP) example,
    // NAD27 / Texas South Central (EPSG:32040), results in US SURVEY FEET:
    //   28 30' N, 96 00' W  ->  E 2 963 503.91 ftUS, N 254 759.80 ftUS.
    // Had the library converted with the international foot the easting would be
    // off by 5.9 ft - the unit handling is what this example pins down.
    auto transformer = makeTransformer(4267, 32040);
    ASSERT_OK(transformer);
    EXPECT_EQ(transformer->target().lengthUnit(), LengthUnit::UsSurveyFoot);
    const auto grid =
        transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{28.5, -96.0});
    ASSERT_OK(grid);
    EXPECT_NEAR(grid->easting, 2963503.91, halfUnit(0.01));
    EXPECT_NEAR(grid->northing, 254759.80, halfUnit(0.01));
}

TEST(GeodesyTransformer, EpsgGuidanceNoteLambertConformalOneParallel)
{
    // [published] IOGP Guidance Note 7-2, Lambert Conic Conformal (1SP) example,
    // JAD69 / Jamaica National Grid (EPSG:24200):
    //   17 55' 55.80" N, 76 56' 37.26" W  ->  E 255 966.58 m, N 142 493.51 m.
    auto transformer = makeTransformer(4242, 24200);
    ASSERT_OK(transformer);
    const GeographicCoordinate position{dms(17, 55, 55.80), -dms(76, 56, 37.26)};
    const auto grid = transformer->forwardAs<ProjectedCoordinate>(position);
    ASSERT_OK(grid);
    EXPECT_NEAR(grid->easting, 255966.58, halfUnit(0.01));
    EXPECT_NEAR(grid->northing, 142493.51, halfUnit(0.01));
}

TEST(GeodesyTransformer, EpsgGuidanceNoteObliqueStereographic)
{
    // [published] IOGP Guidance Note 7-2, Oblique Stereographic example,
    // Amersfoort / RD New (EPSG:28992):  53 N, 6 E -> E 196 105.283 m, N 557 057.739 m.
    auto transformer = makeTransformer(4289, 28992);
    ASSERT_OK(transformer);
    const auto grid = transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{53.0, 6.0});
    ASSERT_OK(grid);
    EXPECT_NEAR(grid->easting, 196105.283, halfUnit(0.001));
    EXPECT_NEAR(grid->northing, 557057.739, halfUnit(0.001));
}

TEST(GeodesyTransformer, EpsgGuidanceNoteGeographicToGeocentric)
{
    // [published] IOGP Guidance Note 7-2, geographic/geocentric conversion, WGS 84:
    //   53 48' 33.820" N, 2 07' 46.380" E, h = 73.0 m
    //   ->  X 3 771 793.968 m, Y 140 253.342 m, Z 5 124 304.349 m.
    auto transformer = makeTransformer(4979, 4978);
    ASSERT_OK(transformer);
    const double latitude = dms(53, 48, 33.820);
    const double longitude = dms(2, 7, 46.380);
    // Geocentric has no named type: generic Coordinate, x = longitude first.
    const auto xyz = transformer->forward(Coordinate{longitude, latitude, 73.0});
    ASSERT_OK(xyz);
    EXPECT_NEAR(xyz->x, 3771793.968, halfUnit(0.001));
    EXPECT_NEAR(xyz->y, 140253.342, halfUnit(0.001));
    EXPECT_NEAR(xyz->z, 5124304.349, halfUnit(0.001));

    // [oracle] The closed form agrees with the published digits too.
    const Geocentric expected =
        geodeticToGeocentric(kWgs84A, kWgs84InverseF, latitude, longitude, 73.0);
    EXPECT_NEAR(expected.x, 3771793.968, halfUnit(0.001));
    EXPECT_NEAR(xyz->x, expected.x, tol::kGeometric);
    EXPECT_NEAR(xyz->y, expected.y, tol::kGeometric);
    EXPECT_NEAR(xyz->z, expected.z, tol::kGeometric);
}

TEST(GeodesyTransformerProperty, GeocentricMatchesClosedFormAndRoundTrips)
{
    auto transformer = makeTransformer(4979, 4978);
    ASSERT_OK(transformer);
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double latitude = random.real(-89.9, 89.9);
        const double longitude = random.real(-180.0, 180.0);
        const double height = random.real(-500.0, 9000.0);

        // [oracle] X = (N + h) cos(phi) cos(lambda) ... coded in the test support.
        const Geocentric expected =
            geodeticToGeocentric(kWgs84A, kWgs84InverseF, latitude, longitude, height);
        const auto xyz = transformer->forward(Coordinate{longitude, latitude, height});
        ASSERT_OK(xyz);
        EXPECT_NEAR(xyz->x, expected.x, tol::kGeometric);
        EXPECT_NEAR(xyz->y, expected.y, tol::kGeometric);
        EXPECT_NEAR(xyz->z, expected.z, tol::kGeometric);

        // [identity] inverse(forward(p)) == p. The inverse is iterative (no
        // closed form exists), so this also checks its convergence.
        const auto back = transformer->inverse(*xyz);
        ASSERT_OK(back);
        EXPECT_NEAR(back->x, longitude, kCoordinateAsDegrees);
        EXPECT_NEAR(back->y, latitude, kCoordinateAsDegrees);
        EXPECT_NEAR(back->z, height, tol::kCoordinate);
    }
}

// ---- the axis-order convention -------------------------------------------------------

TEST(GeodesyTransformer, GenericCoordinateIsLongitudeFirstWhateverTheAuthoritySays)
{
    // EPSG:4326 declares latitude first (see GeodesyCrs.AxesAreReportedInAuthorityOrder).
    // Katana's Coordinate is nevertheless ALWAYS x = longitude, y = latitude.
    // [identity] (lat 40, lon -3) lies on the central meridian: E = 500 000. With
    // the axes swapped the input would mean (lat -3, lon 40), 43 degrees off the
    // zone, and the easting would be millions of metres away.
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    const auto generic = transformer->forward(Coordinate{/*x = lon*/ -3.0, /*y = lat*/ 40.0});
    ASSERT_OK(generic);
    EXPECT_NEAR(generic->x, 500000.0, tol::kGeometric);
    EXPECT_GT(generic->y, 4.4e6); // 40 degrees of meridian arc: ~4 430 km
    EXPECT_LT(generic->y, 4.5e6);

    // The named type says the same thing without any order to remember, and the
    // two paths agree bit for bit.
    const auto named = transformer->forwardAs<ProjectedCoordinate>(
        GeographicCoordinate{.latitude = 40.0, .longitude = -3.0});
    ASSERT_OK(named);
    EXPECT_TRUE(sameBits(named->easting, generic->x));
    EXPECT_TRUE(sameBits(named->northing, generic->y));

    // Inverse: x = longitude comes back first.
    const auto back = transformer->inverse(*generic);
    ASSERT_OK(back);
    EXPECT_NEAR(back->x, -3.0, kCoordinateAsDegrees);
    EXPECT_NEAR(back->y, 40.0, kCoordinateAsDegrees);
}

TEST(GeodesyTransformer, ProjectedCoordinateIsEastingFirstForNorthingFirstAuthorities)
{
    // EPSG:3035 (ETRS89-extended / LAEA Europe) declares its axes northing, easting.
    auto crs = CoordinateReferenceSystem::fromEpsg(3035);
    ASSERT_OK(crs);
    ASSERT_EQ(crs->axes().size(), 2u);
    EXPECT_EQ(crs->axes()[0].direction, "north");

    // [identity] The projection centre (52 N, 10 E) maps to the false origin,
    // E 4 321 000, N 3 210 000 - two values that cannot be confused.
    auto transformer = makeTransformer(4258, 3035);
    ASSERT_OK(transformer);
    const auto named =
        transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{52.0, 10.0});
    ASSERT_OK(named);
    EXPECT_NEAR(named->easting, 4321000.0, tol::kGeometric);
    EXPECT_NEAR(named->northing, 3210000.0, tol::kGeometric);

    const auto generic = transformer->forward(Coordinate{10.0, 52.0});
    ASSERT_OK(generic);
    EXPECT_NEAR(generic->x, 4321000.0, tol::kGeometric); // x is the EASTING
    EXPECT_NEAR(generic->y, 3210000.0, tol::kGeometric);
}

TEST(GeodesyTransformer, NamedGeographicCoordinatesAreDegreesEvenForGradBasedCrs)
{
    // NTF (Paris), EPSG:4807, measures angles in grads from the Paris meridian.
    // [identity] Lambert zone II (EPSG:27572) has its origin at 52 grad = 46.8
    // degrees on the Paris meridian, false origin E 600 000, N 2 200 000.
    auto transformer = makeTransformer(4807, 27572);
    ASSERT_OK(transformer);
    EXPECT_EQ(transformer->source().horizontalUnitName(), "grad");

    const auto named = transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{46.8, 0.0});
    ASSERT_OK(named);
    EXPECT_NEAR(named->easting, 600000.0, tol::kGeometric);
    EXPECT_NEAR(named->northing, 2200000.0, tol::kGeometric);

    // The generic escape hatch carries the CRS's own unit: 52 grads.
    const auto generic = transformer->forward(Coordinate{0.0, 52.0});
    ASSERT_OK(generic);
    EXPECT_NEAR(generic->x, 600000.0, tol::kGeometric);
    EXPECT_NEAR(generic->y, 2200000.0, tol::kGeometric);

    const auto back = transformer->inverseAs<GeographicCoordinate>(*named);
    ASSERT_OK(back);
    EXPECT_NEAR(back->latitude, 46.8, kCoordinateAsDegrees); // degrees, not grads
    EXPECT_NEAR(back->longitude, 0.0, kCoordinateAsDegrees);
}

TEST(GeodesyTransformer, NamedTypesMustMatchTheCrsKind)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);

    // The source CRS is geographic: a ProjectedCoordinate cannot be forwarded.
    const auto wrongSource =
        transformer->forwardAs<ProjectedCoordinate>(ProjectedCoordinate{500000.0, 0.0});
    ASSERT_FALSE(wrongSource.ok());
    EXPECT_EQ(wrongSource.error().code, ErrorCode::InvalidArgument);

    // The target CRS is projected: the result cannot be a GeographicCoordinate.
    const auto wrongTarget =
        transformer->forwardAs<GeographicCoordinate>(GeographicCoordinate{40.0, -3.0});
    ASSERT_FALSE(wrongTarget.ok());
    EXPECT_EQ(wrongTarget.error().code, ErrorCode::InvalidArgument);

    // For the inverse the roles swap.
    EXPECT_FALSE(transformer->inverseAs<ProjectedCoordinate>(GeographicCoordinate{40.0, -3.0}).ok());
    EXPECT_TRUE(
        transformer->inverseAs<GeographicCoordinate>(ProjectedCoordinate{500000.0, 4.4e6}).ok());
}

// ---- failures are errors, never numbers ---------------------------------------------

TEST(GeodesyTransformer, LatitudeBeyondThePoleIsRejected)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);

    const auto named = transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{91.0, -3.0});
    ASSERT_FALSE(named.ok());
    EXPECT_EQ(named.error().code, ErrorCode::InvalidArgument);

    // The generic path relies on PROJ, whose message is passed on.
    const auto generic = transformer->forward(Coordinate{-3.0, 91.0});
    ASSERT_FALSE(generic.ok());
    EXPECT_EQ(generic.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(generic.error().context.find("PROJ"), std::string::npos)
        << generic.error().describe();

    // The transformer is still usable afterwards.
    EXPECT_TRUE(transformer->forward(Coordinate{-3.0, 40.0}).ok());
}

TEST(GeodesyTransformer, NonFiniteInputIsRejected)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();
    constexpr double kInf = std::numeric_limits<double>::infinity();
    for (const Coordinate& bad :
         {Coordinate{kNaN, 40.0}, Coordinate{-3.0, kInf}, Coordinate{-3.0, 40.0, kNaN},
          Coordinate{-3.0, 40.0, 0.0, kNaN}, Coordinate{-3.0, 40.0, 0.0, -kInf}}) {
        const auto result = transformer->forward(bad);
        ASSERT_FALSE(result.ok());
        EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
    }
    EXPECT_FALSE(
        transformer->forwardAs<ProjectedCoordinate>(GeographicCoordinate{kNaN, -3.0}).ok());
    EXPECT_FALSE(
        transformer->inverseAs<GeographicCoordinate>(ProjectedCoordinate{kInf, 0.0}).ok());
}

TEST(GeodesyTransformer, CoordinatesOutsideTheProjectionDomainAreRejected)
{
    // Inverse Transverse Mercator far outside any possible grid value.
    auto utm = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(utm);
    for (const ProjectedCoordinate& absurd :
         {ProjectedCoordinate{1e9, 1e9}, ProjectedCoordinate{1e12, 0.0},
          ProjectedCoordinate{5e7, 1e6}}) {
        const auto result = utm->inverseAs<GeographicCoordinate>(absurd);
        ASSERT_FALSE(result.ok()) << absurd.easting;
        EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
    }

    // A northern Lambert cone has no image of the south pole.
    auto lambert = makeTransformer(4267, 32040);
    ASSERT_OK(lambert);
    const auto pole = lambert->forwardAs<ProjectedCoordinate>(GeographicCoordinate{-90.0, -96.0});
    ASSERT_FALSE(pole.ok());
    EXPECT_EQ(pole.error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(pole.error().context.empty());
}

TEST(GeodesyTransformer, LocalEngineeringCrsCannotBeTransformedByProj)
{
    const auto site = CoordinateReferenceSystem::localEngineering("Site", LengthUnit::Metre);
    const auto utm = CoordinateReferenceSystem::fromEpsg(kUtm30N);
    ASSERT_OK(site);
    ASSERT_OK(utm);
    for (const auto& result : {CoordinateTransformer::create(*site, *utm).ok(),
                               CoordinateTransformer::create(*utm, *site).ok()}) {
        EXPECT_FALSE(result);
    }
    const auto failed = CoordinateTransformer::create(*site, *utm);
    ASSERT_FALSE(failed.ok());
    EXPECT_EQ(failed.error().code, ErrorCode::Unsupported);
    EXPECT_NE(failed.error().message.find("site calibration"), std::string::npos);
}

TEST(GeodesyTransformer, InvalidOptionsAreRejected)
{
    const auto source = CoordinateReferenceSystem::fromEpsg(kWgs84);
    const auto target = CoordinateReferenceSystem::fromEpsg(kUtm30N);
    ASSERT_OK(source);
    ASSERT_OK(target);

    TransformerOptions upsideDown;
    upsideDown.areaOfInterest = GeographicExtent{-6.0, 60.0, 0.0, 50.0}; // south > north
    EXPECT_EQ(CoordinateTransformer::create(*source, *target, upsideDown).error().code,
              ErrorCode::InvalidArgument);

    TransformerOptions negativeAccuracy;
    negativeAccuracy.requiredAccuracyMetres = -1.0;
    EXPECT_EQ(CoordinateTransformer::create(*source, *target, negativeAccuracy).error().code,
              ErrorCode::InvalidArgument);

    TransformerOptions badEpoch;
    badEpoch.coordinateEpoch = std::numeric_limits<double>::quiet_NaN();
    EXPECT_EQ(CoordinateTransformer::create(*source, *target, badEpoch).error().code,
              ErrorCode::InvalidArgument);
}

// ---- batch ------------------------------------------------------------------------------

TEST(GeodesyTransformer, BatchEqualsPerPointBitwise)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    Random random;
    std::vector<Coordinate> points;
    for (int i = 0; i < 2000; ++i) {
        points.push_back(Coordinate{random.real(-9.0, 3.0), random.real(-80.0, 84.0),
                                    random.real(-100.0, 4000.0)});
    }

    std::vector<Coordinate> batch = points;
    const auto forwardReport = transformer->forward(batch);
    ASSERT_OK(forwardReport);
    EXPECT_EQ(forwardReport->succeeded, points.size());
    EXPECT_EQ(forwardReport->failed, 0u);
    EXPECT_TRUE(forwardReport->allSucceeded());
    for (std::size_t i = 0; i < points.size(); ++i) {
        const auto single = transformer->forward(points[i]);
        ASSERT_OK(single);
        ASSERT_TRUE(sameBits(single->x, batch[i].x) && sameBits(single->y, batch[i].y) &&
                    sameBits(single->z, batch[i].z) && sameBits(single->t, batch[i].t))
            << "forward, point " << i;
    }

    std::vector<Coordinate> inverseBatch = batch;
    const auto inverseReport = transformer->inverse(inverseBatch);
    ASSERT_OK(inverseReport);
    EXPECT_TRUE(inverseReport->allSucceeded());
    for (std::size_t i = 0; i < batch.size(); ++i) {
        const auto single = transformer->inverse(batch[i]);
        ASSERT_OK(single);
        ASSERT_TRUE(sameBits(single->x, inverseBatch[i].x) && sameBits(single->y, inverseBatch[i].y) &&
                    sameBits(single->z, inverseBatch[i].z))
            << "inverse, point " << i;
    }
}

TEST(GeodesyTransformer, NamedBatchEqualsNamedPerPointBitwise)
{
    auto transformer = makeTransformer(4267, 32040); // result in US survey feet
    ASSERT_OK(transformer);
    Random random;
    std::vector<GeographicCoordinate> positions;
    for (int i = 0; i < 1000; ++i) {
        positions.push_back(GeographicCoordinate{random.real(27.0, 31.0), random.real(-101.0, -94.0),
                                                 random.real(0.0, 500.0)});
    }
    std::vector<ProjectedCoordinate> grid(positions.size());
    const auto report =
        transformer->forwardAs<ProjectedCoordinate, GeographicCoordinate>(positions, grid);
    ASSERT_OK(report);
    EXPECT_TRUE(report->allSucceeded());
    for (std::size_t i = 0; i < positions.size(); ++i) {
        const auto single = transformer->forwardAs<ProjectedCoordinate>(positions[i]);
        ASSERT_OK(single);
        ASSERT_TRUE(sameBits(single->easting, grid[i].easting) &&
                    sameBits(single->northing, grid[i].northing) &&
                    sameBits(single->height, grid[i].height))
            << "point " << i;
    }

    std::vector<GeographicCoordinate> back(grid.size());
    const auto inverseReport =
        transformer->inverseAs<GeographicCoordinate, ProjectedCoordinate>(grid, back);
    ASSERT_OK(inverseReport);
    EXPECT_TRUE(inverseReport->allSucceeded());
    for (std::size_t i = 0; i < positions.size(); ++i) {
        EXPECT_NEAR(back[i].latitude, positions[i].latitude, kCoordinateAsDegrees);
        EXPECT_NEAR(back[i].longitude, positions[i].longitude, kCoordinateAsDegrees);
        EXPECT_EQ(back[i].height, positions[i].height); // 2D operation: passed through
    }

    std::vector<ProjectedCoordinate> tooSmall(3);
    const auto mismatch =
        transformer->forwardAs<ProjectedCoordinate, GeographicCoordinate>(positions, tooSmall);
    ASSERT_FALSE(mismatch.ok());
    EXPECT_EQ(mismatch.error().code, ErrorCode::InvalidArgument);
}

TEST(GeodesyTransformer, BatchMarksFailedPointsAsNaNAndCountsThem)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();
    std::vector<Coordinate> points = {
        Coordinate{-3.0, 40.0, 10.0},
        Coordinate{-3.0, 95.0, 10.0}, // latitude beyond the pole
        Coordinate{-2.0, 41.0, 20.0},
        Coordinate{kNaN, 41.0, 20.0}, // not a number to begin with
        Coordinate{-1.0, 42.0, 30.0},
    };
    const auto report = transformer->forward(points);
    ASSERT_OK(report);
    EXPECT_EQ(report->succeeded, 3u);
    EXPECT_EQ(report->failed, 2u);
    EXPECT_EQ(report->firstFailedIndex, 1u);
    EXPECT_FALSE(report->allSucceeded());

    for (const std::size_t index : {std::size_t{1}, std::size_t{3}}) {
        EXPECT_TRUE(std::isnan(points[index].x));
        EXPECT_TRUE(std::isnan(points[index].y));
        EXPECT_TRUE(std::isnan(points[index].z));
    }
    // A failure does not disturb its neighbours.
    EXPECT_NEAR(points[0].x, 500000.0, tol::kGeometric); // [identity]
    const auto third = transformer->forward(Coordinate{-1.0, 42.0, 30.0});
    ASSERT_OK(third);
    EXPECT_TRUE(sameBits(points[4].x, third->x) && sameBits(points[4].y, third->y));

    // Named batch: same policy.
    const std::vector<GeographicCoordinate> positions = {
        GeographicCoordinate{40.0, -3.0}, GeographicCoordinate{95.0, -3.0},
        GeographicCoordinate{41.0, -2.0}};
    std::vector<ProjectedCoordinate> grid(positions.size());
    const auto namedReport =
        transformer->forwardAs<ProjectedCoordinate, GeographicCoordinate>(positions, grid);
    ASSERT_OK(namedReport);
    EXPECT_EQ(namedReport->succeeded, 2u);
    EXPECT_EQ(namedReport->failed, 1u);
    EXPECT_EQ(namedReport->firstFailedIndex, 1u);
    EXPECT_TRUE(std::isnan(grid[1].easting) && std::isnan(grid[1].northing) &&
                std::isnan(grid[1].height));
    EXPECT_NEAR(grid[0].easting, 500000.0, tol::kGeometric);
}

TEST(GeodesyTransformer, EmptyBatchIsANoOp)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    std::vector<Coordinate> none;
    const auto report = transformer->forward(none);
    ASSERT_OK(report);
    EXPECT_EQ(report->succeeded, 0u);
    EXPECT_EQ(report->failed, 0u);
}

// ---- round trips ----------------------------------------------------------------------------

TEST(GeodesyTransformerProperty, ProjectedRoundTripIsFarBelowTheCoordinateTolerance)
{
    // transform(inverse_transform(x)) ~ x (PLAN.MD section 34). The requirement
    // is tolerance::kCoordinate (0.1 mm). What the arithmetic can deliver is much
    // better: the series are accurate to nanometres and a UTM northing resolves
    // to 1.9e-9 m (1 ulp at 1e7), so grid -> geographic -> grid is asserted at
    // tolerance::kGeometric (0.1 micrometre, ~50 ulp) - three orders tighter.
    ASSERT_LT(tol::kGeometric, tol::kCoordinate);
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    Random random;
    double worst = 0.0;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const ProjectedCoordinate grid{random.real(166000.0, 834000.0),
                                       random.real(0.0, 9300000.0), random.real(0.0, 3000.0)};
        const auto geographic = transformer->inverseAs<GeographicCoordinate>(grid);
        ASSERT_OK(geographic);
        const auto back = transformer->forwardAs<ProjectedCoordinate>(*geographic);
        ASSERT_OK(back);
        EXPECT_NEAR(back->easting, grid.easting, tol::kGeometric);
        EXPECT_NEAR(back->northing, grid.northing, tol::kGeometric);
        EXPECT_EQ(back->height, grid.height);
        worst = std::max({worst, std::abs(back->easting - grid.easting),
                          std::abs(back->northing - grid.northing)});
    }
    RecordProperty("worst_round_trip_metres", std::to_string(worst));
}

TEST(GeodesyTransformerProperty, GeographicRoundTripIsBelowOneNanodegree)
{
    // geographic -> projected -> geographic, for several projection methods.
    struct Case {
        int geographic;
        int projected;
        double south, north, west, east;
    };
    const Case cases[] = {
        {4326, 32630, -80.0, 84.0, -9.0, 3.0},  // Transverse Mercator
        {4267, 32040, 25.0, 33.0, -104.0, -92.0}, // Lambert Conformal Conic 2SP, ftUS
        {4289, 28992, 50.5, 53.7, 3.2, 7.3},    // Oblique Stereographic
        {4277, 27700, 49.8, 60.9, -8.0, 1.8},   // Transverse Mercator, Airy
    };
    for (const Case& c : cases) {
        auto transformer = makeTransformer(c.geographic, c.projected);
        ASSERT_OK(transformer);
        Random random;
        for (int i = 0; i < kPropertyIterations; ++i) {
            const GeographicCoordinate position{random.real(c.south, c.north),
                                                random.real(c.west, c.east)};
            const auto grid = transformer->forwardAs<ProjectedCoordinate>(position);
            ASSERT_OK(grid);
            const auto back = transformer->inverseAs<GeographicCoordinate>(*grid);
            ASSERT_OK(back);
            EXPECT_NEAR(back->latitude, position.latitude, kCoordinateAsDegrees)
                << "EPSG:" << c.projected;
            EXPECT_NEAR(back->longitude, position.longitude, kCoordinateAsDegrees)
                << "EPSG:" << c.projected;
        }
    }
}

// ---- datum transformations --------------------------------------------------------------------
// What is asserted here holds for ANY installation: with or without the optional
// datum-shift grids (none are installed on the reference machine, whose
// share/proj holds only proj.db and the legacy init files).

TEST(GeodesyTransformerDatum, SameDatumConversionIsNotATransformation)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    const OperationInfo& operation = transformer->operation();
    EXPECT_FALSE(operation.name.empty());
    EXPECT_TRUE(operation.isUsable);
    EXPECT_FALSE(operation.isBallpark);
    EXPECT_FALSE(operation.requiresEpoch);
    EXPECT_TRUE(operation.grids.empty());
    EXPECT_TRUE(transformer->missingGrids().empty());
    EXPECT_NE(operation.projPipeline.find("utm"), std::string::npos) << operation.projPipeline;
    // A map projection is exact by definition: declared accuracy 0.
    ASSERT_TRUE(operation.accuracyMetres.has_value());
    EXPECT_EQ(*operation.accuracyMetres, 0.0);
}

TEST(GeodesyTransformerDatum, Wgs84ToBritishNationalGridReportsItsOperation)
{
    TransformerOptions options;
    options.areaOfInterest = GeographicExtent{-6.0, 50.0, 2.0, 56.0}; // England and Wales
    auto transformer = makeTransformer(kWgs84, 27700, options);
    ASSERT_OK(transformer);

    const OperationInfo& operation = transformer->operation();
    SCOPED_TRACE("selected: " + operation.name);
    EXPECT_NE(operation.name.find("OSGB36"), std::string::npos);
    EXPECT_TRUE(operation.isUsable);
    EXPECT_FALSE(operation.isBallpark); // ballpark is refused by default
    EXPECT_FALSE(operation.projPipeline.empty());
    // EPSG publishes 1 m for the OSTN15 grid and 2 m for the national Helmert.
    ASSERT_TRUE(operation.accuracyMetres.has_value());
    EXPECT_GT(*operation.accuracyMetres, 0.0);
    EXPECT_LE(*operation.accuracyMetres, 2.0);
    EXPECT_GE(transformer->candidates().size(), 2u);

    // Whatever was selected, every grid it uses is present ...
    for (const GridFile& grid : operation.grids) {
        EXPECT_TRUE(grid.available) << grid.name;
    }
    // ... and the caller is told when a better, grid-based candidate was skipped.
    const bool gridBased = !operation.grids.empty();
    if (gridBased) {
        EXPECT_TRUE(transformer->missingGrids().empty());
    } else {
        ASSERT_FALSE(transformer->missingGrids().empty())
            << "the 1 m OSTN15 candidate needs a grid that is not installed";
        for (const GridFile& grid : transformer->missingGrids()) {
            EXPECT_FALSE(grid.available);
            EXPECT_FALSE(grid.name.empty());
        }
    }

    // A real datum shift was applied. [independent bound] OSGB36 and WGS 84
    // differ by roughly 100-140 m on the ground across Great Britain; a null
    // (ballpark) transformation would give 0 m, a wrong-sign one ~250 m.
    const GeographicCoordinate position{dms(52, 39, 27.2531), dms(1, 43, 4.5177)};
    const auto shifted = transformer->forwardAs<ProjectedCoordinate>(position);
    ASSERT_OK(shifted);
    auto projectionOnly = makeTransformer(4277, 27700);
    ASSERT_OK(projectionOnly);
    const auto unshifted = projectionOnly->forwardAs<ProjectedCoordinate>(position);
    ASSERT_OK(unshifted);
    const double shift = std::hypot(shifted->easting - unshifted->easting,
                                    shifted->northing - unshifted->northing);
    EXPECT_GT(shift, 50.0);
    EXPECT_LT(shift, 200.0);

    // Round trip through the datum transformation. This is NOT an identity, and
    // no implementation can make it one: both CRSs are two-dimensional, so the
    // operation saves the input ellipsoidal height, runs the three-dimensional
    // Helmert and restores that same height afterwards (+proj=push/pop +v_3).
    // The forward pass hands the Helmert h = 0 and discards the OSGB36 height it
    // produces - about -45 m, since the Airy 1830 ellipsoid as placed by OSGB36
    // lies that far above the WGS 84 ellipsoid over Great Britain - and the
    // reverse pass hands it h = 0 again.
    //
    // [independent bound] displacing a point by h_o along the ellipsoid normal
    // before the reverse Helmert moves it horizontally by |h_o| * (|dn| + |th|),
    // where dn is the angle between the two datums' normals at the point (the
    // 134 m datum shift over R = 6.37e6 m, i.e. 2.1e-5 rad) and th the 0.88
    // arc-second total rotation of EPSG:1314 (4.3e-6 rad). With |h_o| <= 56 m
    // anywhere in Great Britain that is at most 1.4 mm, so 2 mm is a bound only
    // a discarded height can sit under: the smallest error of the datum shift
    // itself would be tens of metres.
    constexpr double kHeightlessDatumRoundTripMetres = 2e-3;
    const auto back = transformer->inverseAs<GeographicCoordinate>(*shifted);
    ASSERT_OK(back);
    const double residual = groundSeparationMetres(*back, position);
    EXPECT_LT(residual, kHeightlessDatumRoundTripMetres);
    RecordProperty("datum_round_trip_residual_metres", std::to_string(residual));
}

TEST(GeodesyTransformerDatum, RequiredAccuracyFiltersCandidates)
{
    TransformerOptions options;
    options.areaOfInterest = GeographicExtent{-6.0, 50.0, 2.0, 56.0};
    options.requiredAccuracyMetres = 1.5; // only the OSTN15 grid (1 m) qualifies
    auto transformer = makeTransformer(kWgs84, 27700, options);
    if (transformer.ok()) {
        // The grid is installed here.
        ASSERT_TRUE(transformer->operation().accuracyMetres.has_value());
        EXPECT_LE(*transformer->operation().accuracyMetres, 1.5);
        EXPECT_FALSE(transformer->operation().grids.empty());
    } else {
        // It is not: refusing beats silently delivering 2 m.
        EXPECT_EQ(transformer.error().code, ErrorCode::Unsupported);
        EXPECT_NE(transformer.error().context.find("missing grid"), std::string::npos)
            << transformer.error().describe();
    }
}

TEST(GeodesyTransformerDatum, Nad27ToNad83NeverFallsBackToBallparkSilently)
{
    TransformerOptions conus;
    conus.areaOfInterest = GeographicExtent{-100.0, 28.0, -94.0, 32.0}; // Texas
    auto strict = makeTransformer(4267, 4269, conus);

    TransformerOptions lenient = conus;
    lenient.allowBallpark = true;
    auto permissive = makeTransformer(4267, 4269, lenient);
    ASSERT_OK(permissive); // a ballpark operation always exists

    if (strict.ok()) {
        // NADCON grids are installed: a genuine, accuracy-rated transformation.
        EXPECT_FALSE(strict->operation().isBallpark);
        EXPECT_FALSE(strict->operation().grids.empty());
        ASSERT_TRUE(strict->operation().accuracyMetres.has_value());
        EXPECT_LE(*strict->operation().accuracyMetres, 2.0);
        const auto moved = strict->forwardAs<GeographicCoordinate>(GeographicCoordinate{30.0, -97.0});
        ASSERT_OK(moved);
        // [independent bound] the NAD27 -> NAD83 shift in Texas is tens of metres.
        EXPECT_GT(std::abs(moved->longitude + 97.0) + std::abs(moved->latitude - 30.0), 1e-5);
    } else {
        // No grids (the reference machine): the default is to REFUSE, naming them.
        EXPECT_EQ(strict.error().code, ErrorCode::Unsupported);
        EXPECT_NE(strict.error().context.find("missing grid"), std::string::npos)
            << strict.error().describe();
        EXPECT_NE(strict.error().context.find("allowBallpark"), std::string::npos);

        // Only an explicit opt-in yields the ballpark operation, and it is labelled.
        const OperationInfo& operation = permissive->operation();
        EXPECT_TRUE(operation.isBallpark);
        EXPECT_FALSE(operation.accuracyMetres.has_value()); // unknown, not "0"
        ASSERT_FALSE(permissive->missingGrids().empty());
        for (const GridFile& grid : permissive->missingGrids()) {
            EXPECT_FALSE(grid.available);
        }
        // [self] What "ballpark" means: the numbers pass through unshifted,
        // although the true shift is tens of metres. Hence the refusal above.
        const auto unmoved =
            permissive->forwardAs<GeographicCoordinate>(GeographicCoordinate{30.0, -97.0});
        ASSERT_OK(unmoved);
        EXPECT_NEAR(unmoved->latitude, 30.0, kCoordinateAsDegrees);
        EXPECT_NEAR(unmoved->longitude, -97.0, kCoordinateAsDegrees);
    }
}

TEST(GeodesyTransformerDatum, TimeDependentOperationDemandsAnEpoch)
{
    // ITRF2014 -> ETRF2000 (geocentric, EPSG:7789 -> EPSG:7930) is a 14-parameter
    // time-dependent Helmert transformation: Europe moves ~2.5 cm per year in ITRF.
    auto transformer = makeTransformer(7789, 7930);
    ASSERT_OK(transformer);
    EXPECT_TRUE(transformer->operation().requiresEpoch);

    const Coordinate withoutEpoch{3771793.968, 140253.342, 5124304.349};
    ASSERT_EQ(withoutEpoch.t, kUnspecifiedEpoch);
    const auto refused = transformer->forward(withoutEpoch);
    ASSERT_FALSE(refused.ok()) << "PROJ would silently assume the reference epoch";
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("epoch"), std::string::npos);

    Coordinate at2010 = withoutEpoch;
    at2010.t = 2010.0;
    Coordinate at2020 = withoutEpoch;
    at2020.t = 2020.0;
    const auto result2010 = transformer->forward(at2010);
    const auto result2020 = transformer->forward(at2020);
    ASSERT_OK(result2010);
    ASSERT_OK(result2020);
    // [independent bound] ten years of Eurasian plate motion at about 2.5 cm/yr.
    const double drift = std::hypot(result2020->x - result2010->x, result2020->y - result2010->y,
                                    result2020->z - result2010->z);
    EXPECT_GT(drift, 0.15);
    EXPECT_LT(drift, 0.35);

    // The batch path refuses the same point and transforms the others.
    std::vector<Coordinate> batch = {at2010, withoutEpoch, at2020};
    const auto report = transformer->forward(batch);
    ASSERT_OK(report);
    EXPECT_EQ(report->succeeded, 2u);
    EXPECT_EQ(report->failed, 1u);
    EXPECT_EQ(report->firstFailedIndex, 1u);
    EXPECT_TRUE(std::isnan(batch[1].x));
    EXPECT_TRUE(sameBits(batch[0].x, result2010->x) && sameBits(batch[2].x, result2020->x));

    // A default epoch in the options serves coordinates that carry none.
    TransformerOptions options;
    options.coordinateEpoch = 2020.0;
    auto withDefault = makeTransformer(7789, 7930, options);
    ASSERT_OK(withDefault);
    const auto defaulted = withDefault->forward(withoutEpoch);
    ASSERT_OK(defaulted);
    EXPECT_TRUE(sameBits(defaulted->x, result2020->x) && sameBits(defaulted->y, result2020->y) &&
                sameBits(defaulted->z, result2020->z));
}

// ---- ownership and threading ------------------------------------------------------------------

TEST(GeodesyTransformer, CloneIsIndependentAndIdentical)
{
    auto original = makeTransformer(kWgs84, 27700);
    ASSERT_OK(original);
    auto clone = original->clone();
    ASSERT_OK(clone);
    EXPECT_EQ(clone->operation().name, original->operation().name);
    EXPECT_EQ(clone->operation().projPipeline, original->operation().projPipeline);
    EXPECT_EQ(clone->source(), original->source());
    EXPECT_EQ(clone->target(), original->target());

    const Coordinate point{-1.5, 52.5, 100.0};
    const auto a = original->forward(point);
    const auto b = clone->forward(point);
    ASSERT_OK(a);
    ASSERT_OK(b);
    EXPECT_TRUE(sameBits(a->x, b->x) && sameBits(a->y, b->y) && sameBits(a->z, b->z));
}

TEST(GeodesyTransformer, OneTransformerPerThread)
{
    // The documented model: clone() per thread, nothing shared. Four threads
    // transform the same points concurrently and must reproduce the reference
    // result bit for bit (PLAN.MD Rule 7, deterministic computation).
    auto reference = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(reference);
    Random random;
    std::vector<Coordinate> points;
    for (int i = 0; i < 5000; ++i) {
        points.push_back(Coordinate{random.real(-6.0, 0.0), random.real(0.0, 84.0), 0.0});
    }
    std::vector<Coordinate> expected = points;
    ASSERT_OK(reference->forward(expected));

    constexpr int kThreads = 4;
    std::vector<CoordinateTransformer> clones;
    for (int i = 0; i < kThreads; ++i) {
        auto clone = reference->clone();
        ASSERT_OK(clone);
        clones.push_back(std::move(*clone));
    }
    std::vector<std::vector<Coordinate>> results(kThreads, points);
    std::vector<int> failures(kThreads, -1);
    std::vector<std::thread> threads;
    for (int i = 0; i < kThreads; ++i) {
        threads.emplace_back([&, i] {
            const auto report = clones[static_cast<std::size_t>(i)].forward(
                results[static_cast<std::size_t>(i)]);
            failures[static_cast<std::size_t>(i)] =
                report.ok() ? static_cast<int>(report->failed) : -1;
        });
    }
    for (std::thread& thread : threads) {
        thread.join();
    }
    for (std::size_t i = 0; i < results.size(); ++i) {
        EXPECT_EQ(failures[i], 0);
        bool identical = true;
        for (std::size_t p = 0; p < points.size(); ++p) {
            identical = identical && sameBits(results[i][p].x, expected[p].x) &&
                        sameBits(results[i][p].y, expected[p].y);
        }
        EXPECT_TRUE(identical) << "thread " << i;
    }
}

TEST(GeodesyTransformer, MovedFromTransformerReportsInvalidState)
{
    auto transformer = makeTransformer(kWgs84, kUtm30N);
    ASSERT_OK(transformer);
    CoordinateTransformer moved = std::move(*transformer);
    EXPECT_TRUE(moved.forward(Coordinate{-3.0, 40.0}).ok());

    CoordinateTransformer& husk = *transformer; // NOLINT(bugprone-use-after-move): the point
    const auto result = husk.forward(Coordinate{-3.0, 40.0});
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidState);
    EXPECT_FALSE(husk.clone().ok());
    EXPECT_THROW((void)husk.operation(), std::logic_error);
}
