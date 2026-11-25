//! Reading a DWG Katana did not write, and every way a file is refused (docs/dwg.md,
//! "Reading" and "When the file is not one"). The drawings are built with acadrust
//! (support::foreign), each value stated there; what Katana must make of them is worked out
//! from those values and the DXF reader's documented rules (docs/dxf.md, "Reading").

pub mod support;

use katana_commands::CommandStack;
use katana_core::error::ErrorCode;
use katana_dwg::{is_dwg_path, read_dwg, read_dwg_file};
use katana_dxf::{DxfImport, ImportOptions, import_command, indexed_colour};
use katana_entity::{EntityType, Geometry, Model};
use katana_geometry::{Circle2, Segment2};
use support::{Scratch, foreign, layer_named, p, read, rgb, warned};

use acadrust::DxfVersion;

/// The error `bytes` is refused with; a panic naming the drawing when it is read.
#[track_caller]
fn refused(bytes: &[u8]) -> katana_core::Error {
    match read_dwg(bytes, &ImportOptions::default()) {
        Ok(imported) => panic!(
            "read, where a refusal was expected: {} entities, warnings {:?}",
            imported.entities.len(),
            imported.warnings
        ),
        Err(error) => error,
    }
}

fn tallied(imported: &DxfImport, kind: &str) -> Option<(usize, usize)> {
    imported
        .tally
        .iter()
        .find(|tally| tally.kind == kind)
        .map(|tally| (tally.read, tally.imported))
}

mod dwg_reading {
    use super::*;

    #[test]
    fn a_drawing_another_program_wrote_comes_in_with_its_blocks_expanded_and_its_layers() {
        let imported = read(&foreign::bytes_of(&foreign::site(DxfVersion::AC1032)));
        assert_eq!(imported.version, "AC1032");
        assert_eq!(imported.release, "R2018");

        // The line, the circle and the text on Kerb, and the block's circle through the insert:
        // four entities in the model. The paper-space border is counted and not imported.
        assert_eq!(imported.entities.len(), 4, "{:?}", imported.tally);
        let line = imported
            .entities
            .iter()
            .find(|entity| entity.r#type() == EntityType::Line)
            .expect("the line");
        assert_eq!(
            line.geometry,
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(30.0, 40.0)))
        );
        assert_eq!(line.layer, "Kerb");
        let text = imported
            .entities
            .iter()
            .find_map(|entity| match &entity.geometry {
                Geometry::Text(text) => Some(text),
                _ => None,
            })
            .expect("the text");
        assert_eq!(text.text, "Straße 7");
        assert_eq!(text.position, p(10.0, 20.0));
        assert_eq!(text.height, 2.5);

        // The block's circle, radius 1.5 at its origin, placed by the insert: at (200, 300),
        // scaled 2 - radius 3. On layer 0 in the block, it takes the insert's layer, Trees
        // (docs/dxf.md, "Blocks").
        let circles: Vec<_> = imported
            .entities
            .iter()
            .filter(|entity| entity.r#type() == EntityType::Circle)
            .collect();
        assert_eq!(circles.len(), 2);
        let tree = circles
            .iter()
            .find(|entity| entity.layer == "Trees")
            .expect("the tree");
        assert_eq!(
            tree.geometry,
            Geometry::Circle(Circle2::new(p(200.0, 300.0), 3.0))
        );
        let kerb_circle = circles
            .iter()
            .find(|entity| entity.layer == "Kerb")
            .expect("the kerb's circle");
        assert_eq!(
            kerb_circle.geometry,
            Geometry::Circle(Circle2::new(p(100.0, 100.0), 5.0))
        );

        // Kerb's true colour is read exactly; Trees' index 3 is the index table's green.
        let kerb = layer_named(&imported, "Kerb").expect("Kerb");
        assert_eq!(kerb.color, rgb(250, 5, 5));
        let trees = layer_named(&imported, "Trees").expect("Trees");
        assert_eq!(Some(trees.color), indexed_colour(3));

        // The tally names the kinds the drawing held, by their DXF names.
        assert_eq!(tallied(&imported, "INSERT").map(|(read, _)| read), Some(1));
        let (lines_read, lines_imported) = tallied(&imported, "LINE").expect("LINE");
        assert_eq!(lines_read, 2, "the model's line and the paper's border");
        assert_eq!(lines_imported, 1);
    }

    #[test]
    fn an_r2000_drawing_another_program_wrote_comes_in_as_the_r2018_one_does() {
        // The same drawing in the oldest release written, whose text is in a code page and
        // whose file is laid out section after section rather than in pages.
        let imported = read(&foreign::bytes_of(&foreign::site(DxfVersion::AC1015)));
        assert_eq!(imported.release, "R2000");
        assert_eq!(imported.entities.len(), 4, "{:?}", imported.tally);
        let text = imported
            .entities
            .iter()
            .find_map(|entity| match &entity.geometry {
                Geometry::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .expect("the text");
        // R2000's text is in the code page, Windows-1252, which holds the sharp s.
        assert_eq!(text, "Straße 7");
    }

    #[test]
    fn an_r12_drawing_is_read() {
        let imported = read(&foreign::bytes_of(&foreign::release_12()));
        assert_eq!(imported.version, "AC1009");
        assert_eq!(imported.release, "R12");
        assert_eq!(imported.entities.len(), 2);
        assert!(imported.entities.iter().any(|entity| {
            entity.geometry == Geometry::Line(Segment2::new(p(1.0, 2.0), p(4.0, 6.0)))
        }));
        assert!(imported.entities.iter().any(|entity| {
            entity.geometry == Geometry::Circle(Circle2::new(p(5.0, 3.0), 2.0))
        }));
    }

    #[test]
    fn an_entity_whose_coordinate_is_not_a_number_is_left_out_and_said_and_the_rest_come_in() {
        // acadrust's DXF would write the number as 0.0 and the line would come in from the
        // origin; it is left out instead, as the DXF reader leaves out what it cannot use.
        let imported = read(&foreign::bytes_of(&foreign::one_line_damaged()));
        assert_eq!(imported.entities.len(), 1);
        assert_eq!(
            imported.entities[0].geometry,
            Geometry::Line(Segment2::new(p(5.0, 5.0), p(15.0, 5.0)))
        );
        assert!(
            warned(&imported, "1 entities with a coordinate, size or angle that is not a number"),
            "{:?}",
            imported.warnings
        );
    }

    #[test]
    fn the_import_is_one_undo_step() {
        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        let mut imported = read(&foreign::bytes_of(&foreign::site(DxfVersion::AC1032)));
        let count = imported.entities.len();
        let command = import_command(&mut imported, &model).expect("a command");
        if let Err(error) = stack.execute(&mut model, command) {
            panic!("{}", error.describe());
        }
        assert_eq!(model.entities.size(), count);
        assert!(model.layers.find("Kerb").is_some());
        assert_eq!(stack.undo_count(), 1);
        assert!(stack.undo(&mut model).is_ok());
        assert_eq!(model.entities.size(), 0);
        assert!(model.layers.find("Kerb").is_none());
    }

    #[test]
    fn a_file_read_by_its_path_names_it_in_a_failure() {
        let scratch = Scratch::new("a_file_read_by_its_path_names_it_in_a_failure");
        let path = scratch.file("site.dwg");
        assert!(std::fs::write(&path, b"").is_ok());
        let error = read_dwg_file(&path, &ImportOptions::default()).expect_err("an empty file");
        assert!(error.context.contains("site.dwg"), "{}", error.describe());
        let missing = read_dwg_file(&scratch.file("absent.dwg"), &ImportOptions::default())
            .expect_err("a file that is not there");
        assert_eq!(missing.code, ErrorCode::FileImportFailure);
        assert!(missing.context.contains("absent.dwg"));
    }

    #[test]
    fn a_dwg_path_is_known_by_its_extension_in_any_case() {
        assert!(is_dwg_path(std::path::Path::new("C:/plans/site.dwg")));
        assert!(is_dwg_path(std::path::Path::new("site.DWG")));
        assert!(!is_dwg_path(std::path::Path::new("site.dxf")));
        assert!(!is_dwg_path(std::path::Path::new("site.dwg.bak")));
        assert!(!is_dwg_path(std::path::Path::new("dwg")));
    }
}

mod dwg_refusals {
    use super::*;

    #[test]
    fn an_empty_file_is_not_a_dwg() {
        let error = refused(b"");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(error.message, "not a DWG file: it is empty (0 bytes)");
    }

    #[test]
    fn a_file_shorter_than_a_version_string_is_not_a_dwg() {
        let error = refused(b"AC1");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("3 bytes long"), "{}", error.message);
    }

    #[test]
    fn a_dxf_given_the_dwg_extension_is_said_to_be_a_dxf() {
        // How every ASCII DXF begins, CRLF and LF alike, with its comment or without.
        for text in [
            "  0\r\nSECTION\r\n  2\r\nHEADER\r\n",
            "0\nSECTION\n2\nENTITIES\n",
            "999\nwritten by hand\n  0\nSECTION\n",
            "\u{FEFF}  0\nSECTION\n",
        ] {
            let error = refused(text.as_bytes());
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text:?}");
            assert!(error.message.contains("it is a DXF"), "{text:?}: {}", error.message);
        }
    }

    #[test]
    fn a_binary_dxf_given_the_dwg_extension_is_said_to_be_one() {
        let error = refused(b"AutoCAD Binary DXF\r\n\x1a\0");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("binary DXF"), "{}", error.message);
    }

    #[test]
    fn a_release_older_than_r12_is_refused_by_name() {
        // AC1006 is R10's version string (katana_dxf's table of releases).
        let mut bytes = b"AC1006".to_vec();
        bytes.resize(1024, 0);
        let error = refused(&bytes);
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error.message.starts_with("R10 DWG (AC1006) is not read"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_release_this_reader_does_not_know_is_refused_naming_its_version_string() {
        let mut bytes = b"AC1040".to_vec();
        bytes.resize(1024, 0);
        let error = refused(&bytes);
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(
            error
                .message
                .starts_with("DWG version AC1040, a release this reader does not know,"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_file_that_is_not_a_drawing_at_all_is_not_a_dwg() {
        // A zip archive's first bytes.
        let error = refused(b"PK\x03\x04\x14\0\0\0\x08\0");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(
            error.message.contains("does not begin with a DWG version string"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_dwg_cut_short_after_its_file_header_is_damaged() {
        // An R2018 file header is its first 0x100 bytes; the section page map it points at
        // lies past them, so with them alone nothing of the drawing can be found.
        let whole = foreign::bytes_of(&foreign::site(DxfVersion::AC1032));
        assert!(whole.len() > 0x100);
        let error = refused(&whole[..0x100]);
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(
            error.message.starts_with("the R2018 DWG file is damaged"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_curve_tolerance_that_is_not_above_zero_is_refused_before_the_file_is_read() {
        let bytes = foreign::bytes_of(&foreign::release_12());
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let options = ImportOptions {
                curve_tolerance: tolerance,
                ..ImportOptions::default()
            };
            let error = read_dwg(&bytes, &options).expect_err("a tolerance not above 0");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{tolerance}");
        }
    }
}
