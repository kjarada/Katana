#include <gtest/gtest.h>

#include <iostream>
#include <string>

#include "geodesy_test_support.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"

using namespace katana::geodesy;
using katana::core::ErrorCode;

namespace {

// Every PROJ-backed test needs proj.db. MSYS2's PROJ finds it relative to its DLL
// when C:\msys64\ucrt64\bin is on PATH. When it is not found the whole suite
// stops here with an explanation instead of a wall of "crs not found" failures.
class ProjInstallation : public ::testing::Environment {
  public:
    void SetUp() override
    {
        const auto info = projRuntimeInfo();
        ASSERT_TRUE(info.ok())
            << "\n*** PROJ cannot find its resource database (proj.db). ***\n"
            << "Put the PROJ runtime on PATH (C:\\msys64\\ucrt64\\bin) or set PROJ_DATA to the\n"
            << "directory containing proj.db, then re-run.\n"
            << info.error().describe();
        std::cout << "[ PROJ     ] version " << info->version << ", database "
                  << info->databasePath << "\n";
    }
};

[[maybe_unused]] ::testing::Environment* const kProjInstallation =
    ::testing::AddGlobalTestEnvironment(new ProjInstallation); // gtest takes ownership

} // namespace

// ---- factories and metadata ----------------------------------------------------

TEST(GeodesyCrs, Wgs84GeographicFromEpsg)
{
    const auto crs = CoordinateReferenceSystem::fromEpsg(4326);
    ASSERT_OK(crs);
    EXPECT_EQ(crs->name(), "WGS 84");
    EXPECT_EQ(crs->kind(), CrsKind::Geographic2D);
    EXPECT_TRUE(crs->isGeographic());
    EXPECT_FALSE(crs->isProjected());
    EXPECT_EQ(crs->authority(), "EPSG");
    EXPECT_EQ(crs->code(), "4326");
    EXPECT_EQ(crs->epsgCode(), 4326);
    EXPECT_EQ(crs->horizontalUnitName(), "degree");
    EXPECT_FALSE(crs->lengthUnit().has_value()); // angular, not a length
    EXPECT_EQ(crs->definition(), "EPSG:4326");

    // Defining constants of WGS 84 (NIMA TR8350.2), independent of PROJ.
    ASSERT_TRUE(crs->ellipsoid().has_value());
    EXPECT_EQ(crs->ellipsoid()->semiMajorAxis, test::kWgs84A);
    EXPECT_EQ(crs->ellipsoid()->inverseFlattening, test::kWgs84InverseF);
    EXPECT_EQ(*crs->ellipsoid(), Ellipsoid::wgs84());
}

TEST(GeodesyCrs, AxesAreReportedInAuthorityOrder)
{
    // EPSG:4326 is officially latitude first. axes() tells the truth about the
    // definition; the transformation API nevertheless uses longitude first
    // (tested in test_coordinate_transformer.cpp).
    const auto crs = CoordinateReferenceSystem::fromEpsg(4326);
    ASSERT_OK(crs);
    ASSERT_EQ(crs->axes().size(), 2u);
    EXPECT_EQ(crs->axes()[0].direction, "north");
    EXPECT_EQ(crs->axes()[0].abbreviation, "Lat");
    EXPECT_EQ(crs->axes()[1].direction, "east");
    EXPECT_EQ(crs->axes()[1].abbreviation, "Lon");
    EXPECT_TRUE(katana::math::nearlyEqual(crs->axes()[0].unitToSi, katana::math::kDegToRad));
}

TEST(GeodesyCrs, Utm30NProjectedFromEpsg)
{
    const auto crs = CoordinateReferenceSystem::fromEpsg(32630);
    ASSERT_OK(crs);
    EXPECT_EQ(crs->name(), "WGS 84 / UTM zone 30N");
    EXPECT_EQ(crs->kind(), CrsKind::Projected);
    EXPECT_TRUE(crs->isProjected());
    EXPECT_FALSE(crs->isGeographic());
    EXPECT_EQ(crs->epsgCode(), 32630);
    EXPECT_EQ(crs->horizontalUnitName(), "metre");
    EXPECT_EQ(crs->horizontalUnitToSi(), 1.0);
    EXPECT_EQ(crs->lengthUnit(), LengthUnit::Metre);
    ASSERT_EQ(crs->axes().size(), 2u);
    EXPECT_EQ(crs->axes()[0].direction, "east");
    EXPECT_EQ(crs->axes()[1].direction, "north");

    // UTM zone 30 is 6 W .. 0 by definition of the UTM system.
    ASSERT_TRUE(crs->areaOfUse().has_value());
    EXPECT_EQ(crs->areaOfUse()->west, -6.0);
    EXPECT_EQ(crs->areaOfUse()->east, 0.0);
    EXPECT_TRUE(crs->areaOfUse()->contains(40.0, -3.0));
    EXPECT_FALSE(crs->areaOfUse()->contains(40.0, 120.0)); // PROJ itself would not object
}

TEST(GeodesyCrs, StatePlaneInUsSurveyFeetReportsItsUnit)
{
    // NAD27 / Texas South Central is defined in US survey feet (EPSG:32040).
    const auto crs = CoordinateReferenceSystem::fromEpsg(32040);
    ASSERT_OK(crs);
    EXPECT_EQ(crs->kind(), CrsKind::Projected);
    EXPECT_EQ(crs->horizontalUnitName(), "US survey foot");
    EXPECT_EQ(crs->lengthUnit(), LengthUnit::UsSurveyFoot);
    EXPECT_NE(crs->lengthUnit(), LengthUnit::InternationalFoot);
    EXPECT_TRUE(katana::math::nearlyEqual(crs->horizontalUnitToSi(), 1200.0 / 3937.0));
}

TEST(GeodesyCrs, KindsAreClassified)
{
    const auto expectKind = [](int epsg, CrsKind expected) {
        const auto crs = CoordinateReferenceSystem::fromEpsg(epsg);
        ASSERT_TRUE(crs.ok()) << "EPSG:" << epsg << " " << crs.error().describe();
        EXPECT_EQ(crs->kind(), expected) << "EPSG:" << epsg << " is " << toString(crs->kind());
    };
    expectKind(4326, CrsKind::Geographic2D);
    expectKind(4979, CrsKind::Geographic3D);
    expectKind(4978, CrsKind::Geocentric);
    expectKind(27700, CrsKind::Projected);
    expectKind(5701, CrsKind::Vertical); // ODN height
    expectKind(7405, CrsKind::Compound); // OSGB36 / British National Grid + ODN height
}

TEST(GeodesyCrs, CompoundCrsDescribesItsHorizontalComponent)
{
    const auto compound = CoordinateReferenceSystem::fromEpsg(7405);
    ASSERT_OK(compound);
    EXPECT_TRUE(compound->isProjected());
    EXPECT_FALSE(compound->isGeographic());
    ASSERT_EQ(compound->axes().size(), 3u);
    EXPECT_EQ(compound->axes()[2].direction, "up");

    const auto geographicCompound = CoordinateReferenceSystem::fromUserInput("EPSG:4326+5701");
    ASSERT_OK(geographicCompound);
    EXPECT_EQ(geographicCompound->kind(), CrsKind::Compound);
    EXPECT_TRUE(geographicCompound->isGeographic());
}

TEST(GeodesyCrs, FromUserInputAcceptsAuthorityCodes)
{
    const auto byInput = CoordinateReferenceSystem::fromUserInput("  EPSG:32630 ");
    const auto byCode = CoordinateReferenceSystem::fromEpsg(32630);
    ASSERT_OK(byInput);
    ASSERT_OK(byCode);
    EXPECT_EQ(*byInput, *byCode);
    EXPECT_EQ(byInput->epsgCode(), 32630);
}

TEST(GeodesyCrs, FromProjStringIsReadAsCrs)
{
    // Without "+type=crs" PROJ would read this as a bare projection operation.
    const auto crs = CoordinateReferenceSystem::fromProjString("+proj=utm +zone=30 +datum=WGS84");
    ASSERT_OK(crs);
    EXPECT_EQ(crs->kind(), CrsKind::Projected);
    EXPECT_TRUE(crs->authority().empty()); // a PROJ string declares no identifier
    EXPECT_FALSE(crs->epsgCode().has_value());
    EXPECT_NE(crs->definition().find("+type=crs"), std::string::npos);

    const auto epsg = CoordinateReferenceSystem::fromEpsg(32630);
    ASSERT_OK(epsg);
    EXPECT_FALSE(*crs == *epsg); // textual identity: different definitions
    const auto equivalent = crs->isEquivalentTo(*epsg);
    ASSERT_OK(equivalent);
    EXPECT_TRUE(*equivalent); // geodetic identity (PROJ's judgement)
}

TEST(GeodesyCrs, WktRoundTripPreservesTheCrs)
{
    for (const int epsg : {4326, 32630, 27700, 32040, 4978, 7405}) {
        const auto original = CoordinateReferenceSystem::fromEpsg(epsg);
        ASSERT_TRUE(original.ok()) << "EPSG:" << epsg;
        const auto wkt = original->toWkt();
        ASSERT_TRUE(wkt.ok()) << "EPSG:" << epsg << " " << wkt.error().describe();
        EXPECT_EQ(wkt->find('\n'), std::string::npos) << "canonical WKT is single-line";

        const auto restored = CoordinateReferenceSystem::fromWkt(*wkt);
        ASSERT_TRUE(restored.ok()) << "EPSG:" << epsg << " " << restored.error().describe();
        EXPECT_EQ(*restored, *original) << "EPSG:" << epsg;
        EXPECT_EQ(restored->name(), original->name());
        EXPECT_EQ(restored->kind(), original->kind());
        EXPECT_EQ(restored->epsgCode(), original->epsgCode());
        EXPECT_EQ(restored->axes(), original->axes());
        const auto equivalent = restored->isEquivalentTo(*original);
        ASSERT_TRUE(equivalent.ok());
        EXPECT_TRUE(*equivalent) << "EPSG:" << epsg;
    }
}

TEST(GeodesyCrs, LegacyWktFlavoursAreExportedOnDemand)
{
    const auto crs = CoordinateReferenceSystem::fromEpsg(27700);
    ASSERT_OK(crs);
    const auto esri = crs->toWkt(WktVersion::Wkt1Esri);
    ASSERT_OK(esri);
    EXPECT_EQ(esri->rfind("PROJCS[", 0), 0u) << *esri;
    const auto gdal = crs->toWkt(WktVersion::Wkt1Gdal);
    ASSERT_OK(gdal);
    const auto reread = CoordinateReferenceSystem::fromWkt(*gdal);
    ASSERT_OK(reread);
    EXPECT_EQ(reread->epsgCode(), 27700);
}

TEST(GeodesyCrs, EquivalenceDistinguishesDatumsAndAxisOrder)
{
    const auto wgs84 = CoordinateReferenceSystem::fromEpsg(4326);
    const auto osgb36 = CoordinateReferenceSystem::fromEpsg(4277);
    const auto crs84 = CoordinateReferenceSystem::fromUserInput("OGC:CRS84"); // lon, lat
    ASSERT_OK(wgs84);
    ASSERT_OK(osgb36);
    ASSERT_OK(crs84);

    EXPECT_FALSE(*wgs84->isEquivalentTo(*osgb36));
    EXPECT_TRUE(*wgs84->isEquivalentTo(*wgs84));
    // Same datum, opposite axis order: distinct CRSs unless told otherwise.
    EXPECT_FALSE(*wgs84->isEquivalentTo(*crs84));
    EXPECT_TRUE(*wgs84->isEquivalentTo(*crs84, /*ignoreAxisOrder=*/true));
}

// ---- invalid input -----------------------------------------------------------------

TEST(GeodesyCrs, UnknownEpsgCodeIsInvalidCrsWithProjMessage)
{
    const auto crs = CoordinateReferenceSystem::fromEpsg(999999);
    ASSERT_FALSE(crs.ok());
    EXPECT_EQ(crs.error().code, ErrorCode::InvalidCRS);
    EXPECT_NE(crs.error().context.find("EPSG:999999"), std::string::npos) << crs.error().describe();
    // PROJ's own words are passed on, not replaced by a generic text.
    EXPECT_NE(crs.error().context.find("PROJ"), std::string::npos) << crs.error().describe();
    EXPECT_NE(crs.error().context.find("not found"), std::string::npos) << crs.error().describe();
}

TEST(GeodesyCrs, NonPositiveEpsgCodeIsInvalidCrs)
{
    for (const int code : {0, -1, -32630}) {
        const auto crs = CoordinateReferenceSystem::fromEpsg(code);
        ASSERT_FALSE(crs.ok()) << code;
        EXPECT_EQ(crs.error().code, ErrorCode::InvalidCRS);
    }
}

TEST(GeodesyCrs, EpsgCodeOfSomethingThatIsNotACrsIsRejected)
{
    // EPSG numbers each class of object separately, so a code that names a unit
    // or a datum may well name a CRS as well: EPSG:9001 is both the unit "metre"
    // and (since the IGS realisations were registered) the geocentric CRS IGS97.
    // The three below have no entry in the CRS tables at all - EPSG:1314 is the
    // datum transformation OSGB36 to WGS 84 (6), and codes 1024-1999 are
    // coordinate operations and methods only; EPSG:6326 is the WGS 84 datum
    // ensemble and EPSG:7030 the WGS 84 ellipsoid.
    for (const int code : {1314, 6326, 7030}) {
        const auto crs = CoordinateReferenceSystem::fromEpsg(code);
        ASSERT_FALSE(crs.ok()) << "EPSG:" << code << " must not become a CRS";
        EXPECT_EQ(crs.error().code, ErrorCode::InvalidCRS);
    }
}

TEST(GeodesyCrs, GarbageWktIsInvalidCrsWithProjMessage)
{
    const auto garbage = CoordinateReferenceSystem::fromWkt("PROJCS[\"broken\",GEOGCS[oops");
    ASSERT_FALSE(garbage.ok());
    EXPECT_EQ(garbage.error().code, ErrorCode::InvalidCRS);
    EXPECT_NE(garbage.error().context.find("PROJ"), std::string::npos)
        << garbage.error().describe();

    // Text that PROJ would accept elsewhere is still not WKT.
    for (const std::string_view text : {"", "   ", "not wkt", "EPSG:4326", "+proj=longlat"}) {
        const auto crs = CoordinateReferenceSystem::fromWkt(text);
        ASSERT_FALSE(crs.ok()) << "'" << text << "'";
        EXPECT_EQ(crs.error().code, ErrorCode::InvalidCRS);
    }
}

TEST(GeodesyCrs, GarbageProjStringAndUserInputAreInvalidCrs)
{
    for (const std::string_view text : {"", "EPSG:4326", "+proj=no_such_projection", "+proj=utm"}) {
        const auto crs = CoordinateReferenceSystem::fromProjString(text);
        ASSERT_FALSE(crs.ok()) << "'" << text << "'";
        EXPECT_EQ(crs.error().code, ErrorCode::InvalidCRS);
    }
    for (const std::string_view text : {"", "  ", "EPSG:", "EPSG:abc", "complete nonsense"}) {
        const auto crs = CoordinateReferenceSystem::fromUserInput(text);
        ASSERT_FALSE(crs.ok()) << "'" << text << "'";
        EXPECT_EQ(crs.error().code, ErrorCode::InvalidCRS);
    }
}

// ---- local engineering ---------------------------------------------------------------

TEST(GeodesyCrs, LocalEngineeringCrs)
{
    const auto site = CoordinateReferenceSystem::localEngineering("Plant grid", LengthUnit::Metre);
    ASSERT_OK(site);
    EXPECT_EQ(site->name(), "Plant grid");
    EXPECT_EQ(site->kind(), CrsKind::LocalEngineering);
    EXPECT_FALSE(site->isGeographic());
    EXPECT_FALSE(site->isProjected());
    EXPECT_TRUE(site->authority().empty());
    EXPECT_FALSE(site->ellipsoid().has_value()); // no geodetic datum
    EXPECT_FALSE(site->areaOfUse().has_value());
    EXPECT_EQ(site->lengthUnit(), LengthUnit::Metre);
    ASSERT_EQ(site->axes().size(), 2u);
    EXPECT_EQ(site->axes()[0].direction, "east");
    EXPECT_EQ(site->axes()[1].direction, "north");

    EXPECT_FALSE(CoordinateReferenceSystem::localEngineering("", LengthUnit::Metre).ok());
    EXPECT_FALSE(CoordinateReferenceSystem::localEngineering("   ", LengthUnit::Metre).ok());
}

TEST(GeodesyCrs, LocalEngineeringWktIsUnderstoodByProj)
{
    // The WKT is written by Katana without PROJ. PROJ must read it back as the
    // same engineering CRS, including a name that needs quote escaping and a
    // unit other than the metre.
    const auto site =
        CoordinateReferenceSystem::localEngineering("Dock \"B\" grid", LengthUnit::UsSurveyFoot);
    ASSERT_OK(site);
    const auto wkt = site->toWkt();
    ASSERT_OK(wkt);

    const auto reread = CoordinateReferenceSystem::fromWkt(*wkt);
    ASSERT_OK(reread);
    EXPECT_EQ(reread->kind(), CrsKind::LocalEngineering);
    EXPECT_EQ(reread->name(), "Dock \"B\" grid");
    EXPECT_EQ(reread->lengthUnit(), LengthUnit::UsSurveyFoot);
    ASSERT_EQ(reread->axes().size(), 2u);
    EXPECT_EQ(reread->axes()[0].direction, "east");
    EXPECT_EQ(reread->axes()[1].direction, "north");
    const auto equivalent = site->isEquivalentTo(*reread);
    ASSERT_OK(equivalent);
    EXPECT_TRUE(*equivalent);

    const auto metric = CoordinateReferenceSystem::localEngineering("Dock \"B\" grid",
                                                                   LengthUnit::Metre);
    ASSERT_OK(metric);
    EXPECT_FALSE(*site == *metric);
}

TEST(GeodesyCrs, ValueSemantics)
{
    const auto original = CoordinateReferenceSystem::fromEpsg(27700);
    ASSERT_OK(original);
    const CoordinateReferenceSystem copy = *original; // plain copy, no PROJ handle involved
    EXPECT_EQ(copy, *original);
    EXPECT_EQ(copy.name(), "OSGB36 / British National Grid");
    const auto other = CoordinateReferenceSystem::fromEpsg(32630);
    ASSERT_OK(other);
    EXPECT_FALSE(copy == *other);
}
