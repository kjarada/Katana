//! Damaged and hostile designs (docs/dgn.md, "When the file is damaged"):
//! thousands of damaged copies of a design holding every kind the reader
//! reads, each read to the end, none crashing, and every success holding only
//! geometry the drawing accepts.

pub mod support;

use katana_core::error::Result;
use katana_core::text_encoding::is_valid_utf8;
use katana_dgn::{DgnImport, ImportOptions, read_dgn};
use katana_entity::layer_path::validate_layer_path;
use katana_entity::validate;
use support::{Design, Look, Random, member};

/// Whatever the reader said, it said it through the Result: a success holds
/// only geometry the model accepts, on layers that are layer paths.
fn expect_sound(result: &Result<DgnImport>) {
    match result {
        Err(error) => assert!(!error.message.is_empty()),
        Ok(imported) => {
            for entity in &imported.entities {
                assert!(validate(&entity.geometry).is_ok(), "{:?}", entity.geometry);
                assert!(validate_layer_path(&entity.layer).is_ok());
                assert!(is_valid_utf8(&entity.layer));
            }
        }
    }
}

/// A design of every kind the reader reads: lines, a line string, a shape,
/// an arc, an ellipse, a text, a complex chain and a cell holding a hole, in
/// 2D or 3D.
fn every_kind(three_d: bool) -> Vec<u8> {
    let mut design = Design::new(three_d, 1000, 10, [-1e9, -2e9, 0.0]);
    let look = Look {
        colour: 4,
        style: 2,
        weight: 3,
        ..Look::default()
    };
    let line = design.line(1, look, (0, 0, 5), (10_000, 0, 7));
    let string = design.points(4, 2, look, &[(0, 0, 0), (10, 10, 1), (20, 0, 2), (30, 10, 3)]);
    let shape = design.points(6, 3, look, &[(0, 0, 0), (10, 0, 0), (10, 10, 0), (0, 0, 0)]);
    let orientation: &[i32] = if three_d { &[i32::MAX, 0, 0, 0] } else { &[0] };
    let arc = design.arc(4, look, 0, 32_400_000, (5000.0, 5000.0), orientation, (100.0, 100.0, 9.0));
    let ellipse = design.ellipse(5, look, (8000.0, 3000.0), orientation, (0.0, 0.0, 0.0));
    let text = design.text(6, look, 1, 2, 4000, orientation, (5, 5, 0), b"LOT 42");
    let straight = design.points(4, 7, member(look), &[(0, 0, 0), (1000, 0, 0)]);
    let bend = design.arc(7, member(look), -32_400_000, 64_800_000, (500.0, 500.0), orientation, (1000.0, 500.0, 0.0));
    let chain = design.complex_header(12, 7, look, &[straight.clone(), bend.clone()]);
    let area = design.points(6, 8, member(look), &[(0, 0, 0), (100, 0, 0), (100, 100, 0), (0, 0, 0)]);
    let hole = design.points(
        6,
        8,
        member(Look {
            properties: 0x8000,
            ..look
        }),
        &[(10, 10, 0), (20, 10, 0), (20, 20, 0), (10, 10, 0)],
    );
    let cell = design.cell_header(8, (32_725, 9_240), &[area.clone(), hole.clone()]);
    for element in [line, string, shape, arc, ellipse, text, chain, straight, bend, cell, area, hole] {
        design.add(element);
    }
    design.finish()
}

#[test]
fn every_kind_reads_whole_before_it_is_damaged() {
    // The fixture itself: what the damaged copies start from is read whole,
    // so a copy that reads less read less because of its damage.
    for three_d in [false, true] {
        let imported = read_dgn(&every_kind(three_d), &ImportOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        // Line, line string, shape, arc, ellipse, text, the chain as one,
        // and the cell's two shapes.
        assert_eq!(imported.entities.len(), 9, "{:?}", imported.warnings);
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    }
}

#[test]
fn damaged_copies_never_crash_and_successes_hold_only_valid_geometry() {
    let mut random = Random::new(20_261_125);
    let mut copies = 0;
    for three_d in [false, true] {
        let whole = every_kind(three_d);
        for _ in 0..1500 {
            let mut bytes = whole.clone();
            match random.below(4) {
                // Bytes changed anywhere after the design header's signature.
                0 => {
                    for _ in 0..1 + random.below(16) {
                        let at = 4 + random.below(bytes.len() - 4);
                        bytes[at] = random.next_value() as u8;
                    }
                }
                // Cut short anywhere.
                1 => bytes.truncate(random.below(bytes.len())),
                // A run of bytes cut out of the middle.
                2 => {
                    let from = 4 + random.below(bytes.len() - 4);
                    let to = (from + 1 + random.below(64)).min(bytes.len());
                    bytes.drain(from..to);
                }
                // A run repeated.
                _ => {
                    let from = 1536 + random.below(bytes.len() - 1536);
                    let to = (from + 1 + random.below(128)).min(bytes.len());
                    let run: Vec<u8> = bytes[from..to].to_vec();
                    let at = from + random.below(bytes.len() - from);
                    bytes.splice(at..at, run);
                }
            }
            expect_sound(&read_dgn(&bytes, &ImportOptions::default()));
            copies += 1;
        }
    }
    assert_eq!(copies, 3000);
}

#[test]
fn a_hostile_design_is_stopped_at_the_limits_and_says_so() {
    // Cells nested past the depth the reader takes: each the first component
    // of the one before, its length covering the rest.
    let mut design = Design::new(false, 1, 1, [0.0; 3]);
    let line = design.line(1, support::member(Look::default()), (0, 0, 0), (1, 0, 0));
    let mut inner = vec![line];
    for _ in 0..20 {
        let mut header = design.cell_header(1, (0, 0), &inner);
        header[0] |= 0x80; // a component of the next cell out
        let mut nested = vec![header];
        nested.extend(inner);
        inner = nested;
    }
    for element in inner {
        design.add(element);
    }
    let imported = read_dgn(&design.finish(), &ImportOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    assert_eq!(imported.entities.len(), 1);
    assert!(
        imported.warnings.iter().any(|warning| warning.contains("nested more than 16 deep")),
        "{:?}",
        imported.warnings
    );

    // An import stops at its most entities.
    let mut many = Design::new(false, 1, 1, [0.0; 3]);
    for index in 0..10 {
        let line = many.line(1, Look::default(), (index, 0, 0), (index, 1, 0));
        many.add(line);
    }
    let options = ImportOptions {
        maximum_entities: 4,
        ..ImportOptions::default()
    };
    let imported = read_dgn(&many.finish(), &options).expect("read");
    assert_eq!(imported.entities.len(), 4);
    assert!(imported.warnings.iter().any(|warning| warning.contains("stopped at 4")));
}
