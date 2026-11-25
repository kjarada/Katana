//! What the DWG writer puts in the file, as another program reads it, and what it refuses
//! (docs/dwg.md, "Writing"). "Another program" is acadrust read directly, the library Katana
//! writes with: what it finds in the file is what every reader of the format finds there.

pub mod support;

use std::io::Cursor;

use acadrust::{CadDocument, Color as AcadColor, DwgReader, EntityType as AcadEntity};
use katana_core::error::ErrorCode;
use katana_dwg::{DwgRelease, write_dwg, write_dwg_file};
use katana_dxf::{ExportOptions, ImportOptions};
use katana_entity::{Entity, Geometry, Layer, Model};
use katana_geometry::Segment2;
use support::{Scratch, add, add_layer, dwg_of, layer_named, p, read, rgb};

/// The DWG `bytes` as acadrust reads it.
#[track_caller]
fn as_another_program_reads(bytes: Vec<u8>) -> CadDocument {
    match DwgReader::from_stream(Cursor::new(bytes)).read() {
        Ok(document) => document,
        Err(error) => panic!("{error}"),
    }
}

/// One line coloured (250, 5, 5), which no index is; its nearest index is 1, red.
fn off_index_line() -> Model {
    let mut model = Model::default();
    let entity = Entity {
        geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0))),
        color: Some(rgb(250, 5, 5)),
        ..Entity::default()
    };
    assert!(model.entities.add(entity).is_ok());
    model
}

fn the_line(document: &CadDocument) -> AcadColor {
    document
        .entities()
        .find_map(|entity| match entity {
            AcadEntity::Line(line) => Some(line.common.color),
            _ => None,
        })
        .expect("the line")
}

mod dwg_writing {
    use super::*;

    #[test]
    fn from_r2004_a_colour_no_index_is_is_the_drawings_own_true_colour() {
        let model = off_index_line();
        for release in [
            DwgRelease::R2004,
            DwgRelease::R2007,
            DwgRelease::R2010,
            DwgRelease::R2013,
            DwgRelease::R2018,
        ] {
            let document = as_another_program_reads(dwg_of(&model, release));
            assert_eq!(
                the_line(&document),
                AcadColor::from_rgb(250, 5, 5),
                "{}",
                release.name()
            );
        }
    }

    #[test]
    fn in_r2000_a_colour_no_index_is_is_its_nearest_index() {
        // R2000 holds indices alone: another program draws index 1; Katana reads the exact
        // colour back from beside it (round_trip.rs).
        let document = as_another_program_reads(dwg_of(&off_index_line(), DwgRelease::R2000));
        assert_eq!(the_line(&document), AcadColor::Index(1));
    }

    #[test]
    fn a_nested_layer_is_a_legal_name_in_the_drawing_and_its_path_beside_it() {
        // "/" is refused in a symbol name: the path's separators are "$" in the drawing, the
        // separator bound references use (katana_dxf's layer_name_for).
        let mut model = Model::default();
        add_layer(
            &mut model,
            Layer {
                name: String::from("survey/kerb"),
                ..Layer::default()
            },
        );
        add(&mut model, Segment2::new(p(0.0, 0.0), p(1.0, 0.0)), "survey/kerb", &[]);
        let bytes = dwg_of(&model, DwgRelease::DEFAULT);
        let document = as_another_program_reads(bytes.clone());
        assert!(document.layers.get("survey$kerb").is_some());
        assert!(document.layers.get("survey/kerb").is_none());
        let back = read(&bytes);
        assert_eq!(back.entities[0].layer, "survey/kerb");
        assert!(layer_named(&back, "survey/kerb").is_some());
    }

    #[test]
    fn a_layer_path_longer_than_r2004_extended_data_holds_is_refused_there_and_written_from_r2007()
    {
        // 150 + 1 + 150 = 301 characters, all ASCII, so 301 bytes in any code page: more than the
        // 255 an extended data string holds before R2007. Written as the path beside the name
        // "aaa...$bbb...", since the name cannot hold the "/".
        let path = format!("{}/{}", "a".repeat(150), "b".repeat(150));
        let mut model = Model::default();
        add_layer(
            &mut model,
            Layer {
                name: path.clone(),
                ..Layer::default()
            },
        );
        add(&mut model, Segment2::new(p(0.0, 0.0), p(1.0, 0.0)), &path, &[]);
        for release in [DwgRelease::R2000, DwgRelease::R2004] {
            let error = write_dwg(&model, &ExportOptions::default(), release)
                .expect_err("a path too long for the release");
            assert_eq!(error.code, ErrorCode::Unsupported, "{}", release.name());
            assert!(error.message.contains("301 bytes"), "{}", error.message);
            assert!(error.message.contains("R2007"), "{}", error.message);
        }
        let back = read(&dwg_of(&model, DwgRelease::R2007));
        assert_eq!(back.entities[0].layer, path);
    }

    #[test]
    fn writing_to_a_file_replaces_it_whole_and_leaves_nothing_beside_it() {
        let scratch = Scratch::new("writing_to_a_file_replaces_it_whole");
        let path = scratch.file("site.dwg");
        assert!(std::fs::write(&path, b"an older file, longer than nothing").is_ok());
        let model = off_index_line();
        let written = match write_dwg_file(&model, &path, &ExportOptions::default(), DwgRelease::R2013)
        {
            Ok(written) => written,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(written.bytes.is_empty(), "the bytes are the file's, not the reply's");
        assert_eq!(written.release, DwgRelease::R2013);
        assert_eq!(written.entities_written, 1);
        let on_disk = std::fs::read(&path).expect("the file");
        assert_eq!(on_disk.len(), written.bytes_written);
        assert_eq!(&on_disk[..6], b"AC1027");
        let mut partial = path.as_os_str().to_owned();
        partial.push(".partial");
        assert!(!std::path::Path::new(&partial).exists());
        let back = katana_dwg::read_dwg_file(&path, &ImportOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(back.entities.len(), 1);
    }

    #[test]
    fn only_the_entities_the_options_name_are_written() {
        // The DXF writer's scope (ExportOptions::entities), which EXPORT's shared scope and
        // filter fill: two lines, one named.
        let mut model = Model::default();
        let kept = add(&mut model, Segment2::new(p(0.0, 0.0), p(1.0, 0.0)), "0", &[]);
        add(&mut model, Segment2::new(p(5.0, 5.0), p(6.0, 5.0)), "0", &[]);
        let options = ExportOptions {
            entities: vec![kept],
            ..ExportOptions::default()
        };
        let written = write_dwg(&model, &options, DwgRelease::DEFAULT)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(written.entities_written, 1);
        let back = read(&written.bytes);
        assert_eq!(back.entities.len(), 1);
        assert_eq!(
            back.entities[0].geometry,
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0)))
        );
    }
}
