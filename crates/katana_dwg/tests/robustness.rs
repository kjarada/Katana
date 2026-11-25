//! Damaged DWGs read to the end (docs/dwg.md, "When the file is damaged"), as katana_dxf's
//! robustness tests read damaged DXFs: copies of a DWG Katana wrote, bytes changed and the file
//! cut short, each refused through the Result or read into geometry the model accepts - never
//! a crash, never a value the model would refuse.

pub mod support;

use katana_core::error::Result;
use katana_core::text_encoding::is_valid_utf8;
use katana_dwg::{DwgRelease, read_dwg};
use katana_dxf::{DxfImport, ImportOptions};
use katana_entity::{Layer, Model, PointGeometry, validate};
use katana_geometry::{Circle2, Polyline2, Segment2};
use support::{Mt19937, add, add_layer, dwg_of, p};

/// Whatever the reader said, it said it through the Result: a success holds only geometry the
/// model accepts.
fn expect_sound(result: &Result<DxfImport>) {
    match result {
        Err(error) => assert!(!error.message.is_empty()),
        Ok(imported) => {
            for entity in &imported.entities {
                assert!(validate(&entity.geometry).is_ok(), "{:?}", entity.geometry);
                assert!(is_valid_utf8(&entity.layer));
            }
        }
    }
}

/// A small drawing of the kinds a survey holds, each with its own numbers.
fn survey() -> Model {
    let mut model = Model::default();
    for name in ["pegs", "kerb", "trees", "lots"] {
        add_layer(
            &mut model,
            Layer {
                name: String::from(name),
                ..Layer::default()
            },
        );
    }
    add(
        &mut model,
        PointGeometry {
            position: p(502_000.125, 6_250_000.5),
        },
        "pegs",
        &[Some(31.25)],
    );
    add(&mut model, Segment2::new(p(0.0, 0.0), p(30.0, 40.0)), "kerb", &[]);
    add(&mut model, Circle2::new(p(15.0, 10.0), 5.0), "trees", &[]);
    add(
        &mut model,
        Polyline2::new(vec![p(0.0, 0.0), p(30.0, 0.0), p(30.0, 20.0)], true),
        "lots",
        &[Some(10.0), Some(11.0), Some(12.5)],
    );
    model
}

mod dwg_robustness {
    use super::*;

    #[test]
    fn the_undamaged_copy_reads_whole() {
        // The control: what the damaged copies are made from reads as written.
        for release in [DwgRelease::R2000, DwgRelease::R2018] {
            let bytes = dwg_of(&survey(), release);
            let result = read_dwg(&bytes, &ImportOptions::default());
            expect_sound(&result);
            let imported = result.unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(imported.entities.len(), 4, "{}", release.name());
        }
    }

    #[test]
    fn damaged_copies_are_refused_or_read_into_sound_geometry() {
        // 150 copies of each of two releases - one laid out section after section, one in
        // compressed pages - each with up to eight bytes changed past the version string; and
        // 50 each cut short at a point drawn at random. The version string is left alone so
        // that the copies reach acadrust rather than the refusal of what is not a DWG.
        let mut random = Mt19937::new(20261125);
        for release in [DwgRelease::R2000, DwgRelease::R2018] {
            let original = dwg_of(&survey(), release);
            for _ in 0..150 {
                let mut copy = original.clone();
                let changes = 1 + random.below(8);
                for _ in 0..changes {
                    let at = 6 + random.below(copy.len() - 6);
                    copy[at] = (random.next_u32() & 0xFF) as u8;
                }
                expect_sound(&read_dwg(&copy, &ImportOptions::default()));
            }
            for _ in 0..50 {
                let length = 6 + random.below(original.len() - 6);
                expect_sound(&read_dwg(&original[..length], &ImportOptions::default()));
            }
        }
    }
}
