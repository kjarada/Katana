//! IMPORT and EXPORT of a .dwg through the one executor, as katana_cli, katana_mcp and the
//! window's dialogs run them (docs/dwg.md, "On every front end"): the records, release=, the
//! shared scope, placement, one undo step, and the refusals by name.
//!
//! The drawing each test starts from, by hand (draw_site), as export_options draws it:
//!
//! ```text
//! lots   two closed rectangles, (0, 0) - (50, 40) and (50, 0) - (100, 40)
//! roads  one line, (0, 50) - (100, 50)
//! pegs   one point, (10, 60)
//! ```
//!
//! so 4 entities on 4 layers with 0, inside the box (0, 0) - (100, 60); LAYERS lots takes 2.

use std::path::Path;

use katana_app::dwg_verbs::read_dwg_import;
use katana_core::ErrorCode;
use katana_entity::{Geometry, PointGeometry, PropertyMap};
use katana_geometry::Point2;

use crate::geo_fixture::{GeoFixture, holds, must, refused};
use crate::quoted;

fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-geo-dwg-", name)
}

fn draw_site(fixture: &mut GeoFixture) {
    fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
    fixture.rect((50.0, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
    fixture.line((0.0, 50.0), (100.0, 50.0), "roads", PropertyMap::new());
    fixture.add(
        Geometry::Point(PointGeometry {
            position: Point2::new(10.0, 60.0),
        }),
        "pegs",
        PropertyMap::new(),
    );
}

/// The field `key` of the first record of `kind`.
#[track_caller]
fn field(reply: &str, kind: &str, key: &str) -> String {
    let record = GeoFixture::record(reply, kind).unwrap_or_else(|| panic!("no {kind}:\n{reply}"));
    String::from(
        record
            .get(key)
            .unwrap_or_else(|| panic!("no {key} in {kind}:\n{reply}")),
    )
}

mod dwg_verbs {
    use super::*;

    #[test]
    fn a_drawing_goes_out_as_a_dwg_and_comes_back_in_records() {
        let mut fixture = fixture("ADrawingGoesOutAsADwgAndComesBackInRecords");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("site plan.dwg");
        let exported = fixture.ok(&format!("EXPORT {}", quoted(&file)));
        assert_eq!(field(&exported, "exported", "kind"), "dwg");
        assert_eq!(field(&exported, "exported", "driver"), "DWG");
        assert_eq!(field(&exported, "exported", "format"), "DWG R2018");
        assert_eq!(field(&exported, "exported", "entities"), "4");
        assert_eq!(field(&exported, "exported", "skipped"), "0");
        // 0, lots, roads, pegs: every layer the drawing has.
        assert_eq!(field(&exported, "exported", "layers"), "4");

        let mut again = super::fixture("ADrawingGoesOutAsADwgAndComesBackInRecordsAgain");
        let imported = again.ok(&format!("IMPORT {}", quoted(&file)));
        assert_eq!(field(&imported, "imported", "kind"), "dwg");
        assert_eq!(field(&imported, "imported", "format"), "DWG R2018");
        assert_eq!(field(&imported, "imported", "entities"), "4");
        assert_eq!(field(&imported, "imported", "bounds"), "0,0,100,60");
        // Each kind the file held, by its DXF name: the two lots, the road, the peg.
        let tallies = katana_app::geo::replies::parse_records(&imported);
        for (element, count) in [("LWPOLYLINE", "2"), ("LINE", "1"), ("POINT", "1")] {
            let tally = tallies
                .iter()
                .find(|record| record.kind == "tally" && record.get("element") == Some(element))
                .unwrap_or_else(|| panic!("no tally of {element}:\n{imported}"));
            assert_eq!(tally.get("read"), Some(count), "{element}");
            assert_eq!(tally.get("imported"), Some(count), "{element}");
        }
        assert_eq!(again.entity_count(), 4);
        assert_eq!(again.on("lots").len(), 2);
    }

    #[test]
    fn release_chooses_the_dwg_release_and_the_reply_says_which() {
        let mut fixture = fixture("ReleaseChoosesTheDwgReleaseAndTheReplySaysWhich");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("site.dwg");
        let exported = fixture.ok(&format!("EXPORT {} release=r2010", quoted(&file)));
        assert_eq!(field(&exported, "exported", "format"), "DWG R2010");
        // A DWG begins with its version string; R2010's is AC1024.
        let bytes = std::fs::read(&file).expect("the file");
        assert_eq!(&bytes[..6], b"AC1024");
        let imported = fixture.ok(&format!("IMPORT {}", quoted(&file)));
        assert_eq!(field(&imported, "imported", "format"), "DWG R2010");
    }

    #[test]
    fn a_scoped_dwg_export_writes_only_the_scope() {
        let mut fixture = fixture("AScopedDwgExportWritesOnlyTheScope");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("lots.dwg");
        let reply = fixture.ok(&format!("EXPORT {} LAYERS lots", quoted(&file)));
        holds(&reply, "matched=2");
        let read = must(read_dwg_import(Path::new(&file), None));
        assert_eq!(read.entities.len(), 2);
        assert!(read.entities.iter().all(|entity| entity.layer == "lots"));
    }

    #[test]
    fn preview_of_a_dwg_export_says_what_and_writes_nothing() {
        let mut fixture = fixture("PreviewOfADwgExportSaysWhatAndWritesNothing");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("preview.dwg");
        let reply = fixture.ok(&format!("EXPORT {} PREVIEW", quoted(&file)));
        assert_eq!(field(&reply, "export", "kind"), "dwg");
        assert_eq!(field(&reply, "export", "preview"), "yes");
        assert_eq!(field(&reply, "export", "entities"), "4");
        assert!(!Path::new(&file).exists());
    }

    #[test]
    fn a_dwg_import_takes_a_placement_and_is_one_undo_step() {
        let mut fixture = fixture("ADwgImportTakesAPlacementAndIsOneUndoStep");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("site.dwg");
        fixture.ok(&format!("EXPORT {}", quoted(&file)));

        let mut again = super::fixture("ADwgImportTakesAPlacementAndIsOneUndoStepAgain");
        let imported = again.ok(&format!("IMPORT {} OFFSET=10,-20.5", quoted(&file)));
        // (0, 0) - (100, 60) moved 10 east and 20.5 south.
        assert_eq!(field(&imported, "imported", "bounds"), "10,-20.5,110,39.5");
        assert_eq!(field(&imported, "placed", "placement"), "offset");
        assert_eq!(again.entity_count(), 4);
        must(again.document.undo());
        assert_eq!(again.entity_count(), 0);
    }

    #[test]
    fn release_is_refused_by_name_for_a_file_that_is_not_a_dwg() {
        let mut fixture = fixture("ReleaseIsRefusedByNameForAFileThatIsNotADwg");
        draw_site(&mut fixture);
        for name in ["site.dxf", "site.12da", "site.gpkg"] {
            let file = fixture.scratch.file(name);
            let error = refused(
                fixture.run(&format!("EXPORT {} release=R2018", quoted(&file))),
                "release= for a file that is not a DWG",
            );
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{name}");
            holds(&error.message, "release= is an option of a DWG export");
            assert!(!Path::new(&file).exists(), "{name}");
        }
    }

    #[test]
    fn a_release_katana_does_not_write_is_refused_by_name() {
        let mut fixture = fixture("AReleaseKatanaDoesNotWriteIsRefusedByName");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("old.dwg");
        let error = refused(
            fixture.run(&format!("EXPORT {} release=R14", quoted(&file))),
            "R14",
        );
        assert_eq!(error.code, ErrorCode::Unsupported);
        holds(&error.message, "R14 DWG (AC1014) is not written");
        let error = refused(
            fixture.run(&format!("EXPORT {} release=R2019", quoted(&file))),
            "R2019",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "R2018");
        assert!(!Path::new(&file).exists());
    }

    #[test]
    fn gdal_options_for_a_dwg_are_refused_by_name() {
        let mut fixture = fixture("GdalOptionsForADwgAreRefusedByName");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("x.dwg");
        let error = refused(
            fixture.run(&format!("EXPORT {} split=layer", quoted(&file))),
            "a GDAL option for a DWG",
        );
        holds(&error.message, "split=");
        holds(&error.message, "release=");
    }

    #[test]
    fn an_import_option_for_another_kind_of_file_is_refused_for_a_dwg() {
        let mut fixture = fixture("AnImportOptionForAnotherKindOfFileIsRefusedForADwg");
        draw_site(&mut fixture);
        let file = fixture.scratch.file("site.dwg");
        fixture.ok(&format!("EXPORT {}", quoted(&file)));
        let error = refused(
            fixture.run(&format!("IMPORT {} band=1", quoted(&file))),
            "band= for a DWG",
        );
        holds(&error.message, "band= is not an option of a dwg import");
    }

    #[test]
    fn a_dxf_saved_under_the_dwg_extension_is_refused_and_imports_nothing() {
        let mut fixture = fixture("ADxfSavedUnderTheDwgExtensionIsRefusedAndImportsNothing");
        draw_site(&mut fixture);
        let dxf = fixture.scratch.file("site.dxf");
        fixture.ok(&format!("EXPORT {}", quoted(&dxf)));
        let renamed = fixture.scratch.file("site.dwg");
        assert!(std::fs::copy(&dxf, &renamed).is_ok());
        let before = fixture.entity_count();
        let error = refused(
            fixture.run(&format!("IMPORT {}", quoted(&renamed))),
            "a DXF under the DWG extension",
        );
        assert_eq!(error.code, ErrorCode::ParseFailure);
        holds(&error.message, "it is a DXF");
        assert_eq!(fixture.entity_count(), before);
    }
}
