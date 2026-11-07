//! The twin of tests/dxf/test_robustness.cpp: malformed and hostile input,
//! each refusal and each limit with what it says, and thousands of damaged
//! copies of the fixtures read to the end.

pub mod support;

use katana_core::error::{ErrorCode, Result};
use katana_core::text_encoding::is_valid_utf8;
use katana_dxf::{DxfImport, ImportOptions, read_dxf};
use katana_entity::validate;
use support::{Mt19937, fixture};

/// Whatever the reader said, it said it through the Result: a success holds
/// only geometry the model accepts.
fn expect_sound(result: &Result<DxfImport>) {
    match result {
        Err(error) => assert!(!error.message.is_empty()),
        Ok(imported) => {
            for entity in &imported.entities {
                assert!(validate(&entity.geometry).is_ok());
                assert!(is_valid_utf8(&entity.layer));
            }
        }
    }
}

/// The C++'s `bound == 0 ? 0 : random() % bound`.
fn below(random: &mut Mt19937, bound: usize) -> usize {
    if bound == 0 {
        return 0;
    }
    (u64::from(random.next_u32()) % bound as u64) as usize
}

/// Where the line holding `at` ends, its '\n' included: the end of the text
/// when no '\n' follows.
fn line_end(text: &[u8], at: usize) -> usize {
    text[at..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(text.len(), |offset| at + offset + 1)
}

mod dxf_robustness {
    use super::*;

    #[test]
    fn something_that_is_not_a_dxf_file_is_an_error() {
        let error = read_dxf(
            "hello, world\nthis is not a drawing\n",
            &ImportOptions::default(),
        )
        .expect_err("not a DXF file");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("not a DXF file"));
    }

    #[test]
    fn an_empty_file_is_an_error() {
        assert!(read_dxf("", &ImportOptions::default()).is_err());
    }

    #[test]
    fn a_binary_dxf_is_refused_by_name() {
        let error = read_dxf(b"AutoCAD Binary DXF\r\n\x1a\0", &ImportOptions::default())
            .expect_err("a binary DXF");
        assert_eq!(error.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_group_code_that_is_not_a_number_is_an_error_that_says_where() {
        let error = read_dxf(
            "  0\nSECTION\n  2\nENTITIES\nten\nLINE\n",
            &ImportOptions::default(),
        )
        .expect_err("a group code that is not a number");
        assert!(error.message.contains("line 5"));
    }

    #[test]
    fn a_truncated_file_keeps_what_was_read_and_says_it_is_truncated() {
        let whole = fixture("r2000_site.dxf");
        // Cut inside the ENTITIES section, after the LWPOLYLINE and the MTEXT.
        let needle = b"  0\nLINE\n  5\n52";
        let cut = whole
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("the line after the MTEXT");
        let result = read_dxf(&whole[..cut + 7], &ImportOptions::default())
            .expect("what was read before the cut");
        // The polyline and the three lines of the MTEXT.
        assert_eq!(result.entities.len(), 4);
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("truncated"));
    }

    #[test]
    fn a_number_that_is_not_a_number_costs_only_its_entity() {
        let text = concat!(
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nLINE\n  8\n0\n 10\nabc\n 20\n0\n 11\n1\n 21\n1\n",
            "  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n",
            "  0\nENDSEC\n  0\nEOF\n"
        );
        let result = read_dxf(text, &ImportOptions::default()).expect("the second line");
        assert_eq!(result.entities.len(), 1);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn a_block_that_inserts_itself_is_not_expanded_forever() {
        let text = concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nLOOP\n 10\n0\n 20\n0\n",
            "  0\nLINE\n 10\n0\n 20\n0\n 11\n1\n 21\n0\n",
            "  0\nINSERT\n  2\nLOOP\n 10\n5\n 20\n0\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  2\nLOOP\n 10\n0\n 20\n0\n",
            "  0\nENDSEC\n  0\nEOF\n"
        );
        let result = read_dxf(text, &ImportOptions::default()).expect("the block's own line");
        // The block's own line, once; the insert of itself inside it is refused.
        assert_eq!(result.entities.len(), 1);
    }

    #[test]
    fn a_huge_array_insert_is_capped_not_allocated() {
        let text = concat!(
            "  0\nSECTION\n  2\nBLOCKS\n",
            "  0\nBLOCK\n  2\nDOT\n 10\n0\n 20\n0\n",
            "  0\nPOINT\n 10\n0\n 20\n0\n",
            "  0\nENDBLK\n  0\nENDSEC\n",
            "  0\nSECTION\n  2\nENTITIES\n",
            "  0\nINSERT\n  2\nDOT\n 10\n0\n 20\n0\n 70\n2000000000\n 71\n",
            "2000000000\n 44\n1\n 45\n1\n",
            "  0\nENDSEC\n  0\nEOF\n"
        );
        let options = ImportOptions {
            maximum_entities: 1000,
            ..ImportOptions::default()
        };
        let result = read_dxf(text, &options).expect("the capped array");
        assert!(result.entities.len() <= 1000);
    }

    // Thousands of damaged copies of the two fixtures - bytes changed, lines cut
    // out, lines repeated, the file cut short - each read to the end. The
    // generator is the standard's mt19937, whose sequence the standard fixes, so
    // every run damages the same way and a failure reproduces.
    #[test]
    fn damaged_input_never_crashes_and_only_ever_yields_valid_geometry() {
        let mut random = Mt19937::new(20260924);
        for name in ["r12_survey.dxf", "r2000_site.dxf"] {
            let original = fixture(name);
            assert!(!original.is_empty(), "{name}");
            for _ in 0..1500 {
                let mut damaged = original.clone();
                let edits = 1 + below(&mut random, 8);
                for _ in 0..edits {
                    if damaged.is_empty() {
                        break;
                    }
                    let at = below(&mut random, damaged.len());
                    match below(&mut random, 5) {
                        // a byte changed to anything
                        0 => damaged[at] = (random.next_u32() & 0xFF) as u8,
                        // a line cut out
                        1 => {
                            let stop = line_end(&damaged, at);
                            damaged = [&damaged[..at], &damaged[stop..]].concat();
                        }
                        // a line repeated
                        2 => {
                            let stop = line_end(&damaged, at);
                            damaged = [&damaged[..at], &damaged[at..stop], &damaged[at..]].concat();
                        }
                        // cut short
                        3 => damaged.truncate(at),
                        // a digit where a digit was, so the pairs stay aligned
                        _ if damaged[at].is_ascii_digit() => {
                            damaged[at] = b'0' + below(&mut random, 10) as u8;
                        }
                        _ => {}
                    }
                }
                expect_sound(&read_dxf(damaged.as_slice(), &ImportOptions::default()));
            }
        }
    }
}
