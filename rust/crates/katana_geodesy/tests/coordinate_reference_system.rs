//! The twins of tests/geodesy/test_coordinate_reference_system.cpp.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

pub mod support;

use katana_core::ErrorCode;
use katana_core::library_data::{bundled_proj_data_directory, proj_data_directory};
use katana_geodesy::coordinate_reference_system::{self, proj_runtime_info};
use katana_geodesy::{CoordinateReferenceSystem, CrsKind, Ellipsoid, LengthUnit, WktVersion};
use katana_math::numerics::{DEG_TO_RAD, nearly_equal, tolerance};

use support::{WGS84_A, WGS84_INVERSE_F, assert_err, assert_ok};

// Every PROJ-backed test needs proj.db. In gtest a global environment stopped
// the suite here with an explanation instead of a wall of "crs not found"
// failures; Rust has no such hook, so the same check is a case of its own.
mod proj_installation {
    use super::*;

    #[test]
    fn proj_opens_its_resource_database() {
        let info = match proj_runtime_info() {
            Ok(info) => info,
            Err(error) => panic!(
                "\n*** PROJ cannot find its resource database (proj.db). ***\n\
                 Put the PROJ runtime where Katana looks for it, or set PROJ_DATA to the\n\
                 directory containing proj.db, then re-run.\n{}",
                error.describe()
            ),
        };
        println!(
            "[ PROJ     ] version {}, database {}",
            info.version, info.database_path
        );
    }
}

mod geodesy_crs {
    use super::*;

    // ---- factories and metadata --------------------------------------------------

    #[test]
    fn wgs84_geographic_from_epsg() {
        let crs = assert_ok(CoordinateReferenceSystem::from_epsg(4326));
        assert_eq!(crs.name(), "WGS 84");
        assert_eq!(crs.kind(), CrsKind::Geographic2D);
        assert!(crs.is_geographic());
        assert!(!crs.is_projected());
        assert_eq!(crs.authority(), "EPSG");
        assert_eq!(crs.code(), "4326");
        assert_eq!(crs.epsg_code(), Some(4326));
        assert_eq!(crs.horizontal_unit_name(), "degree");
        assert_eq!(crs.length_unit(), None); // angular, not a length
        assert_eq!(crs.definition(), "EPSG:4326");

        // Defining constants of WGS 84 (NIMA TR8350.2), independent of PROJ.
        let ellipsoid = crs.ellipsoid().expect("WGS 84 has an ellipsoid");
        assert_eq!(ellipsoid.semi_major_axis, WGS84_A);
        assert_eq!(ellipsoid.inverse_flattening, WGS84_INVERSE_F);
        assert_eq!(ellipsoid, Ellipsoid::wgs84());
    }

    #[test]
    fn axes_are_reported_in_authority_order() {
        // EPSG:4326 is officially latitude first. axes() tells the truth about
        // the definition; the transformation API nevertheless uses longitude
        // first (tested in coordinate_transformer.rs).
        let crs = assert_ok(CoordinateReferenceSystem::from_epsg(4326));
        assert_eq!(crs.axes().len(), 2);
        assert_eq!(crs.axes()[0].direction, "north");
        assert_eq!(crs.axes()[0].abbreviation, "Lat");
        assert_eq!(crs.axes()[1].direction, "east");
        assert_eq!(crs.axes()[1].abbreviation, "Lon");
        assert!(nearly_equal(
            crs.axes()[0].unit_to_si,
            DEG_TO_RAD,
            tolerance::RELATIVE
        ));
    }

    #[test]
    fn utm30_n_projected_from_epsg() {
        let crs = assert_ok(CoordinateReferenceSystem::from_epsg(32630));
        assert_eq!(crs.name(), "WGS 84 / UTM zone 30N");
        assert_eq!(crs.kind(), CrsKind::Projected);
        assert!(crs.is_projected());
        assert!(!crs.is_geographic());
        assert_eq!(crs.epsg_code(), Some(32630));
        assert_eq!(crs.horizontal_unit_name(), "metre");
        assert_eq!(crs.horizontal_unit_to_si(), 1.0);
        assert_eq!(crs.length_unit(), Some(LengthUnit::Metre));
        assert_eq!(crs.axes().len(), 2);
        assert_eq!(crs.axes()[0].direction, "east");
        assert_eq!(crs.axes()[1].direction, "north");

        // UTM zone 30 is 6 W .. 0 by definition of the UTM system.
        let area = crs.area_of_use().expect("UTM zone 30N has an area of use");
        assert_eq!(area.west, -6.0);
        assert_eq!(area.east, 0.0);
        assert!(area.contains(40.0, -3.0));
        assert!(!area.contains(40.0, 120.0)); // PROJ itself would not object
    }

    #[test]
    fn state_plane_in_us_survey_feet_reports_its_unit() {
        // NAD27 / Texas South Central is defined in US survey feet (EPSG:32040).
        let crs = assert_ok(CoordinateReferenceSystem::from_epsg(32040));
        assert_eq!(crs.kind(), CrsKind::Projected);
        assert_eq!(crs.horizontal_unit_name(), "US survey foot");
        assert_eq!(crs.length_unit(), Some(LengthUnit::UsSurveyFoot));
        assert_ne!(crs.length_unit(), Some(LengthUnit::InternationalFoot));
        assert!(nearly_equal(
            crs.horizontal_unit_to_si(),
            1200.0 / 3937.0,
            tolerance::RELATIVE
        ));
    }

    #[test]
    fn kinds_are_classified() {
        let expect_kind = |epsg: i32, expected: CrsKind| {
            let crs = match CoordinateReferenceSystem::from_epsg(epsg) {
                Ok(crs) => crs,
                Err(error) => panic!("EPSG:{epsg} {}", error.describe()),
            };
            assert_eq!(
                crs.kind(),
                expected,
                "EPSG:{epsg} is {}",
                coordinate_reference_system::to_string(crs.kind())
            );
        };
        expect_kind(4326, CrsKind::Geographic2D);
        expect_kind(4979, CrsKind::Geographic3D);
        expect_kind(4978, CrsKind::Geocentric);
        expect_kind(27700, CrsKind::Projected);
        expect_kind(5701, CrsKind::Vertical); // ODN height
        expect_kind(7405, CrsKind::Compound); // OSGB36 / British National Grid + ODN height
    }

    #[test]
    fn compound_crs_describes_its_horizontal_component() {
        let compound = assert_ok(CoordinateReferenceSystem::from_epsg(7405));
        assert!(compound.is_projected());
        assert!(!compound.is_geographic());
        assert_eq!(compound.axes().len(), 3);
        assert_eq!(compound.axes()[2].direction, "up");

        let geographic_compound =
            assert_ok(CoordinateReferenceSystem::from_user_input("EPSG:4326+5701"));
        assert_eq!(geographic_compound.kind(), CrsKind::Compound);
        assert!(geographic_compound.is_geographic());
    }

    #[test]
    fn from_user_input_accepts_authority_codes() {
        let by_input = assert_ok(CoordinateReferenceSystem::from_user_input("  EPSG:32630 "));
        let by_code = assert_ok(CoordinateReferenceSystem::from_epsg(32630));
        assert_eq!(by_input, by_code);
        assert_eq!(by_input.epsg_code(), Some(32630));
    }

    #[test]
    fn from_proj_string_is_read_as_crs() {
        // Without "+type=crs" PROJ would read this as a bare projection operation.
        let crs = assert_ok(CoordinateReferenceSystem::from_proj_string(
            "+proj=utm +zone=30 +datum=WGS84",
        ));
        assert_eq!(crs.kind(), CrsKind::Projected);
        assert!(crs.authority().is_empty()); // a PROJ string declares no identifier
        assert_eq!(crs.epsg_code(), None);
        assert!(crs.definition().contains("+type=crs"));

        let epsg = assert_ok(CoordinateReferenceSystem::from_epsg(32630));
        assert!(crs != epsg); // textual identity: different definitions
        let equivalent = assert_ok(crs.is_equivalent_to(&epsg, false));
        assert!(equivalent); // geodetic identity (PROJ's judgement)
    }

    #[test]
    fn wkt_round_trip_preserves_the_crs() {
        for epsg in [4326, 32630, 27700, 32040, 4978, 7405] {
            let original = match CoordinateReferenceSystem::from_epsg(epsg) {
                Ok(crs) => crs,
                Err(error) => panic!("EPSG:{epsg} {}", error.describe()),
            };
            let wkt = match original.to_wkt(WktVersion::Wkt2_2019) {
                Ok(wkt) => wkt,
                Err(error) => panic!("EPSG:{epsg} {}", error.describe()),
            };
            assert!(!wkt.contains('\n'), "canonical WKT is single-line");

            let restored = match CoordinateReferenceSystem::from_wkt(&wkt) {
                Ok(crs) => crs,
                Err(error) => panic!("EPSG:{epsg} {}", error.describe()),
            };
            assert_eq!(restored, original, "EPSG:{epsg}");
            assert_eq!(restored.name(), original.name());
            assert_eq!(restored.kind(), original.kind());
            assert_eq!(restored.epsg_code(), original.epsg_code());
            assert_eq!(restored.axes(), original.axes());
            let equivalent = assert_ok(restored.is_equivalent_to(&original, false));
            assert!(equivalent, "EPSG:{epsg}");
        }
    }

    #[test]
    fn legacy_wkt_flavours_are_exported_on_demand() {
        let crs = assert_ok(CoordinateReferenceSystem::from_epsg(27700));
        let esri = assert_ok(crs.to_wkt(WktVersion::Wkt1Esri));
        assert!(esri.starts_with("PROJCS["), "{esri}");
        let gdal = assert_ok(crs.to_wkt(WktVersion::Wkt1Gdal));
        let reread = assert_ok(CoordinateReferenceSystem::from_wkt(&gdal));
        assert_eq!(reread.epsg_code(), Some(27700));
    }

    #[test]
    fn equivalence_distinguishes_datums_and_axis_order() {
        let wgs84 = assert_ok(CoordinateReferenceSystem::from_epsg(4326));
        let osgb36 = assert_ok(CoordinateReferenceSystem::from_epsg(4277));
        // lon, lat
        let crs84 = assert_ok(CoordinateReferenceSystem::from_user_input("OGC:CRS84"));

        assert!(!wgs84.is_equivalent_to(&osgb36, false).unwrap());
        assert!(wgs84.is_equivalent_to(&wgs84, false).unwrap());
        // Same datum, opposite axis order: distinct CRSs unless told otherwise.
        assert!(!wgs84.is_equivalent_to(&crs84, false).unwrap());
        assert!(wgs84.is_equivalent_to(&crs84, true).unwrap());
    }

    // ---- invalid input -------------------------------------------------------------------

    #[test]
    fn unknown_epsg_code_is_invalid_crs_with_proj_message() {
        let error = assert_err(CoordinateReferenceSystem::from_epsg(999999));
        assert_eq!(error.code, ErrorCode::InvalidCrs);
        assert!(
            error.context.contains("EPSG:999999"),
            "{}",
            error.describe()
        );
        // PROJ's own words are passed on, not replaced by a generic text.
        assert!(error.context.contains("PROJ"), "{}", error.describe());
        assert!(error.context.contains("not found"), "{}", error.describe());
    }

    #[test]
    fn non_positive_epsg_code_is_invalid_crs() {
        for code in [0, -1, -32630] {
            let error = assert_err(CoordinateReferenceSystem::from_epsg(code));
            assert_eq!(error.code, ErrorCode::InvalidCrs, "{code}");
        }
    }

    #[test]
    fn epsg_code_of_something_that_is_not_a_crs_is_rejected() {
        // EPSG numbers each class of object separately, so a code that names a
        // unit or a datum may well name a CRS as well: EPSG:9001 is both the
        // unit "metre" and (since the IGS realisations were registered) the
        // geocentric CRS IGS97. The three below have no entry in the CRS tables
        // at all - EPSG:1314 is the datum transformation OSGB36 to WGS 84 (6),
        // and codes 1024-1999 are coordinate operations and methods only;
        // EPSG:6326 is the WGS 84 datum ensemble and EPSG:7030 the WGS 84
        // ellipsoid.
        for code in [1314, 6326, 7030] {
            let result = CoordinateReferenceSystem::from_epsg(code);
            assert!(result.is_err(), "EPSG:{code} must not become a CRS");
            assert_eq!(assert_err(result).code, ErrorCode::InvalidCrs);
        }
    }

    #[test]
    fn garbage_wkt_is_invalid_crs_with_proj_message() {
        let garbage = assert_err(CoordinateReferenceSystem::from_wkt(
            "PROJCS[\"broken\",GEOGCS[oops",
        ));
        assert_eq!(garbage.code, ErrorCode::InvalidCrs);
        assert!(garbage.context.contains("PROJ"), "{}", garbage.describe());

        // Text that PROJ would accept elsewhere is still not WKT.
        for text in ["", "   ", "not wkt", "EPSG:4326", "+proj=longlat"] {
            let error = assert_err(CoordinateReferenceSystem::from_wkt(text));
            assert_eq!(error.code, ErrorCode::InvalidCrs, "'{text}'");
        }
    }

    #[test]
    fn garbage_proj_string_and_user_input_are_invalid_crs() {
        for text in ["", "EPSG:4326", "+proj=no_such_projection", "+proj=utm"] {
            let error = assert_err(CoordinateReferenceSystem::from_proj_string(text));
            assert_eq!(error.code, ErrorCode::InvalidCrs, "'{text}'");
        }
        for text in ["", "  ", "EPSG:", "EPSG:abc", "complete nonsense"] {
            let error = assert_err(CoordinateReferenceSystem::from_user_input(text));
            assert_eq!(error.code, ErrorCode::InvalidCrs, "'{text}'");
        }
    }

    // ---- local engineering ------------------------------------------------------------------

    #[test]
    fn local_engineering_crs() {
        let site = assert_ok(CoordinateReferenceSystem::local_engineering(
            "Plant grid",
            LengthUnit::Metre,
        ));
        assert_eq!(site.name(), "Plant grid");
        assert_eq!(site.kind(), CrsKind::LocalEngineering);
        assert!(!site.is_geographic());
        assert!(!site.is_projected());
        assert!(site.authority().is_empty());
        assert_eq!(site.ellipsoid(), None); // no geodetic datum
        assert_eq!(site.area_of_use(), None);
        assert_eq!(site.length_unit(), Some(LengthUnit::Metre));
        assert_eq!(site.axes().len(), 2);
        assert_eq!(site.axes()[0].direction, "east");
        assert_eq!(site.axes()[1].direction, "north");

        assert!(CoordinateReferenceSystem::local_engineering("", LengthUnit::Metre).is_err());
        assert!(CoordinateReferenceSystem::local_engineering("   ", LengthUnit::Metre).is_err());
    }

    #[test]
    fn local_engineering_wkt_is_understood_by_proj() {
        // The WKT is written by Katana without PROJ. PROJ must read it back as
        // the same engineering CRS, including a name that needs quote escaping
        // and a unit other than the metre.
        let site = assert_ok(CoordinateReferenceSystem::local_engineering(
            "Dock \"B\" grid",
            LengthUnit::UsSurveyFoot,
        ));
        let wkt = assert_ok(site.to_wkt(WktVersion::Wkt2_2019));

        let reread = assert_ok(CoordinateReferenceSystem::from_wkt(&wkt));
        assert_eq!(reread.kind(), CrsKind::LocalEngineering);
        assert_eq!(reread.name(), "Dock \"B\" grid");
        assert_eq!(reread.length_unit(), Some(LengthUnit::UsSurveyFoot));
        assert_eq!(reread.axes().len(), 2);
        assert_eq!(reread.axes()[0].direction, "east");
        assert_eq!(reread.axes()[1].direction, "north");
        let equivalent = assert_ok(site.is_equivalent_to(&reread, false));
        assert!(equivalent);

        let metric = assert_ok(CoordinateReferenceSystem::local_engineering(
            "Dock \"B\" grid",
            LengthUnit::Metre,
        ));
        assert!(site != metric);
    }

    #[test]
    fn value_semantics() {
        let original = assert_ok(CoordinateReferenceSystem::from_epsg(27700));
        let copy = original.clone(); // plain copy, no PROJ handle involved
        assert_eq!(copy, original);
        assert_eq!(copy.name(), "OSGB36 / British National Grid");
        let other = assert_ok(CoordinateReferenceSystem::from_epsg(32630));
        assert!(copy != other);
    }
}

mod proj_data {
    use super::*;

    // In a relocated Linux prefix PROJ's compiled-in search path is wrong, and
    // Katana points its contexts at the data beside libproj instead
    // (katana_core::library_data). Wherever that is decided, it must be a
    // directory PROJ can open proj.db from - this process has libproj loaded.
    #[test]
    fn the_directory_katana_points_proj_at_holds_its_database() {
        let Some(data) = proj_data_directory() else {
            eprintln!(
                "PROJ's own search is used here (PROJ_DATA set, or not a relocated Linux prefix)"
            );
            return;
        };
        assert!(data.join("proj.db").is_file(), "{}", data.display());
        // And PROJ opens it: a CRS the database defines is found by its code.
        assert_ok(CoordinateReferenceSystem::from_epsg(28356));
    }

    // The PROJ data Katana carries is named whatever the environment says - it
    // is what a context falls back on when PROJ_DATA or PROJ_LIB names data
    // this PROJ cannot open (ProjContext::create; the cli.proj_* tests set the
    // variables in a process of their own, since PROJ reads them at its first
    // search). Wherever it is given it must be a directory with the database
    // in it, and one PROJ really opens: the context made from it names a CRS
    // by its code.
    #[test]
    fn the_data_katana_carries_holds_its_database_whatever_the_environment_says() {
        let Some(bundled) = bundled_proj_data_directory() else {
            eprintln!("this PROJ is a distribution's, with no share/proj beside its library");
            return;
        };
        assert!(bundled.join("proj.db").is_file(), "{}", bundled.display());
        // The environment's choice, when it works, is not the bundled data;
        // when it is not set, PROJ finds this directory itself (Windows) or
        // Katana points it there (Linux, macOS).
        let info = assert_ok(proj_runtime_info());
        if std::env::var_os("PROJ_DATA").is_none() && std::env::var_os("PROJ_LIB").is_none() {
            // std::filesystem::equivalent: the same file, however named.
            let database = std::fs::canonicalize(&info.database_path)
                .unwrap_or_else(|error| panic!("{}: {error}", info.database_path));
            let expected = std::fs::canonicalize(bundled.join("proj.db"))
                .unwrap_or_else(|error| panic!("{}: {error}", bundled.display()));
            assert_eq!(
                database,
                expected,
                "{} is not in {}",
                info.database_path,
                bundled.display()
            );
        }
    }
}
