// The project's coordinate system (project_crs.rs): read from what a person or
// an agent types, stored in one form, set as one undo step, found in the common
// list, suggested for a place, and driven from the command line with CRS
// (tests/cad/test_project_crs.cpp).

use std::collections::BTreeSet;

use katana_cad::project_crs::{
    common_coordinate_systems, describe_coordinate_system, find_coordinate_systems,
    normalise_coordinate_system, suggest_coordinate_systems,
};
use katana_cad::{CommandInterpreter, Document};
use katana_core::ErrorCode;

use crate::{Session, done, err, ok};

fn steps(document: &Document) -> usize {
    document.history().undo_count()
}

mod project_crs {
    use super::*;

    #[test]
    fn every_spelling_of_a_code_is_stored_as_epsg_colon_code() {
        for text in ["EPSG:7856", "epsg:7856", "7856", "  EPSG:7856  "] {
            let stored = ok(normalise_coordinate_system(text));
            assert_eq!(stored, "EPSG:7856", "{text}");
        }
        let empty = ok(normalise_coordinate_system("   "));
        assert_eq!(empty, "", "blank text is local coordinates");
        let nonsense = err(normalise_coordinate_system("not a coordinate system"));
        assert_eq!(nonsense.code, ErrorCode::InvalidCrs);
    }

    #[test]
    fn a_system_is_described_by_its_registered_name() {
        let mga = ok(describe_coordinate_system("EPSG:7856"));
        assert_eq!(mga.id, "EPSG:7856");
        assert_eq!(mga.name, "GDA2020 / MGA zone 56");
        assert!(mga.projected);
        assert_eq!(mga.units, "metre");
        match &mga.area_of_use {
            Some(area) => assert!(area.contains(-33.87, 151.21), "Sydney is in MGA zone 56"),
            None => panic!("MGA zone 56 has an area of use"),
        }

        let wgs84 = ok(describe_coordinate_system("4326"));
        assert!(!wgs84.projected);
        assert_eq!(wgs84.name, "WGS 84");
    }

    #[test]
    fn the_common_list_names_every_system_as_the_register_does() {
        // Checked against PROJ's own database, so a typo in the list - a name or
        // a code - cannot reach a menu.
        let mut ids = BTreeSet::new();
        for entry in common_coordinate_systems() {
            assert!(ids.insert(entry.id.clone()), "{} is listed twice", entry.id);
            let description = match describe_coordinate_system(&entry.id) {
                Ok(description) => description,
                Err(error) => panic!("{}: {}", entry.id, error.describe()),
            };
            assert_eq!(description.name, entry.name, "{}", entry.id);
            assert!(!entry.group.is_empty(), "{}", entry.id);
        }
        assert!(ids.len() >= 140);
    }

    #[test]
    fn find_matches_every_word_anywhere_in_the_entry() {
        let zone56 = find_coordinate_systems("mga 56");
        assert_eq!(zone56.len(), 2);
        assert_eq!(zone56[0].id, "EPSG:7856");
        assert_eq!(zone56[1].id, "EPSG:28356");
        assert_eq!(find_coordinate_systems("27700").len(), 1);
        assert_eq!(
            find_coordinate_systems("").len(),
            common_coordinate_systems().len()
        );
        assert!(find_coordinate_systems("mga 99").is_empty());
    }

    #[test]
    fn a_place_is_suggested_its_zone_best_first() {
        // Sydney: GDA2020 MGA 56, then GDA94 MGA 56, then WGS 84 UTM 56S, then WGS 84.
        let sydney = ok(suggest_coordinate_systems(151.21, -33.87));
        assert_eq!(sydney.len(), 4);
        assert_eq!(sydney[0].id, "EPSG:7856");
        assert_eq!(sydney[1].id, "EPSG:28356");
        assert_eq!(sydney[2].id, "EPSG:32756");
        assert_eq!(sydney[3].id, "EPSG:4326");
        // Perth is in zone 50.
        let perth = ok(suggest_coordinate_systems(115.86, -31.95));
        assert_eq!(perth[0].id, "EPSG:7850");
        // London: no MGA, UTM 30N.
        let london = ok(suggest_coordinate_systems(-0.12, 51.5));
        assert_eq!(london.len(), 2);
        assert_eq!(london[0].id, "EPSG:32630");
        // Every suggestion is a real system.
        for list in [&sydney, &perth, &london] {
            for entry in list {
                let description = match describe_coordinate_system(&entry.id) {
                    Ok(description) => description,
                    Err(error) => panic!("{}: {}", entry.id, error.describe()),
                };
                assert_eq!(description.name, entry.name, "{}", entry.id);
            }
        }
        // Beyond UTM's reach, WGS 84 alone.
        let pole = ok(suggest_coordinate_systems(10.0, 86.0));
        assert_eq!(pole.len(), 1);
        assert_eq!(pole[0].id, "EPSG:4326");
        assert_eq!(
            err(suggest_coordinate_systems(190.0, 0.0)).code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn setting_it_is_one_undo_step_and_a_refusal_changes_nothing() {
        let mut document = Document::default();
        assert!(document.metadata().coordinate_system.is_empty());
        let before = steps(&document);

        done(document.set_coordinate_system("epsg:7856", Document::SET_CRS));
        assert_eq!(document.metadata().coordinate_system, "EPSG:7856");
        assert_eq!(steps(&document), before + 1);
        assert_eq!(document.history().undo_name(), "SET_CRS");

        // The same system again, spelt another way: no step.
        done(document.set_coordinate_system("7856", Document::SET_CRS));
        assert_eq!(steps(&document), before + 1);

        // Nonsense is refused and changes nothing.
        let refused = err(document.set_coordinate_system("GDA3000 zone 99", Document::SET_CRS));
        assert_eq!(refused.code, ErrorCode::InvalidCrs);
        assert_eq!(document.metadata().coordinate_system, "EPSG:7856");
        assert_eq!(steps(&document), before + 1);

        done(document.set_coordinate_system("EPSG:28356", Document::SET_CRS));
        done(document.undo());
        assert_eq!(document.metadata().coordinate_system, "EPSG:7856");
        done(document.undo());
        assert_eq!(document.metadata().coordinate_system, "");
        done(document.redo());
        assert_eq!(document.metadata().coordinate_system, "EPSG:7856");

        done(document.set_coordinate_system("", Document::SET_CRS));
        assert_eq!(
            document.metadata().coordinate_system,
            "",
            "cleared: local coordinates"
        );
    }

    #[test]
    fn the_crs_verb_shows_sets_finds_and_suggests() {
        let mut session = Session::default();
        assert_eq!(session.ok("CRS"), "crs id=none (local coordinates)");

        let before = steps(&session.document);
        assert_eq!(
            session.ok("CRS SET 7856"),
            "crs id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" kind=\"projected\" units=metre"
        );
        assert_eq!(steps(&session.document), before + 1);
        assert_eq!(session.document.metadata().coordinate_system, "EPSG:7856");

        assert_eq!(
            session.ok("CRS FIND mga 56"),
            "found count=2\n\
             choice id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" group=\"Australia - GDA2020 MGA\"\n\
             choice id=EPSG:28356 name=\"GDA94 / MGA zone 56\" group=\"Australia - GDA94 MGA\""
        );

        let suggested = session.ok("CRS SUGGEST 151.21,-33.87");
        assert_eq!(
            suggested.split('\n').next().unwrap_or(""),
            "suggested count=4"
        );
        assert!(suggested.contains("choice id=EPSG:7856"));

        let refused = session.refused("CRS SET nowhere");
        assert_eq!(refused.code, ErrorCode::InvalidCrs);
        assert_eq!(session.document.metadata().coordinate_system, "EPSG:7856");

        assert_eq!(session.ok("CRS CLEAR"), "crs id=none (local coordinates)");
        assert!(CommandInterpreter::help_text().contains("CRS SET"));
    }

    // CRS SET's help says it takes WKT, and a WKT names everything in double
    // quotes. The verb once rebuilt its argument from the line's tokens, which
    // drop every quote, so a pasted WKT - the text of a .prj file - reached PROJ
    // without its quotes and every one was refused "InvalidCRS: not a coordinate
    // system". The dialog, which called Document::set_coordinate_system itself,
    // took the same text, so only the command line, katana_cli and MCP failed.
    //
    // Both WKTs are written out by hand for GDA2020 / MGA zone 56, EPSG:7856 -
    // Transverse Mercator on GRS 1980, central meridian 153 E, scale 0.9996,
    // false origin 500 000 m E, 10 000 000 m N (the ICSM GDA2020 Technical
    // Manual, MGA2020 zone parameters): an OGC WKT1 that carries its EPSG
    // authority, stored by its code, and an ESRI-style .prj that carries none,
    // stored as typed - its quotes and all.
    #[test]
    fn the_crs_verb_takes_a_wkt_with_its_quotes_as_typed() {
        let ogc = concat!(
            "PROJCS[\"GDA2020 / MGA zone 56\",GEOGCS[\"GDA2020\",DATUM[\"Geocentric_Datum_of_",
            "Australia_2020\",SPHEROID[\"GRS 1980\",6378137,298.257222101]],PRIMEM[\"Greenwich\",0],",
            "UNIT[\"degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],",
            "PARAMETER[\"latitude_of_origin\",0],PARAMETER[\"central_meridian\",153],",
            "PARAMETER[\"scale_factor\",0.9996],PARAMETER[\"false_easting\",500000],",
            "PARAMETER[\"false_northing\",10000000],UNIT[\"metre\",1],AXIS[\"Easting\",EAST],",
            "AXIS[\"Northing\",NORTH],AUTHORITY[\"EPSG\",\"7856\"]]"
        );
        let prj = concat!(
            "PROJCS[\"GDA2020_MGA_Zone_56\",GEOGCS[\"GCS_GDA2020\",DATUM[\"GDA2020\",",
            "SPHEROID[\"GRS_1980\",6378137.0,298.257222101]],PRIMEM[\"Greenwich\",0.0],",
            "UNIT[\"Degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],",
            "PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",10000000.0],",
            "PARAMETER[\"Central_Meridian\",153.0],PARAMETER[\"Scale_Factor\",0.9996],",
            "PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]"
        );

        let mut session = Session::default();
        let by_authority = session.ok(&format!("CRS SET {ogc}"));
        assert_eq!(
            by_authority,
            "crs id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" kind=\"projected\" units=metre"
        );
        assert_eq!(session.document.metadata().coordinate_system, "EPSG:7856");

        // Blanks around it, a tab after SET and a lower-case verb change nothing.
        let as_typed = session.ok(&format!("  crs set\t{prj}  "));
        assert_eq!(
            session.document.metadata().coordinate_system,
            prj,
            "stored as typed, quotes and all"
        );
        assert!(
            as_typed.contains("kind=\"projected\" units=metre"),
            "{as_typed}"
        );

        // A name in quotes is still the grammar's: the quotes group it and go.
        session.ok("CRS CLEAR");
        session.ok("CRS SET \"GDA2020 / MGA zone 56\"");
        assert_eq!(session.document.metadata().coordinate_system, "EPSG:7856");

        // A refusal says what PROJ made of the text, not only that it failed.
        let broken = session.refused("CRS SET PROJCS[\"half a WKT\",GEOGCS[");
        assert_eq!(broken.code, ErrorCode::InvalidCrs);
        assert!(
            broken.context.contains("PROJCS[\"half a WKT\",GEOGCS["),
            "the text as typed: {}",
            broken.context
        );
        assert!(
            broken.context.contains("PROJ"),
            "PROJ's reason: {}",
            broken.context
        );
        assert_eq!(session.document.metadata().coordinate_system, "EPSG:7856");
    }
}
