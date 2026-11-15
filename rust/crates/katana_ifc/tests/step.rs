//! Twin of tests/ifc/test_step.cpp: the ISO 10303-21 encodings and the IFC
//! GlobalId, each against a worked example. The expected GlobalIds are
//! IfcOpenShell's (ifcopenshell.guid.compress of the same 128 bits), the
//! independent implementation of the encoding the IFC documentation gives.

#![allow(
    clippy::excessive_precision,
    reason = "The literals are the C++ test's, digit for digit."
)]

pub mod support;

use std::collections::BTreeSet;

use katana_ifc::{
    character_count, compress_guid, decode_step_string, guid_for, step_real, step_string,
};

mod ifc_guid {
    use super::*;

    #[test]
    fn compresses_as_the_ifc_documentation_and_ifc_open_shell_do() {
        assert_eq!(compress_guid(0, 0), "0000000000000000000000");
        assert_eq!(compress_guid(0, 1), "0000000000000000000001");
        assert_eq!(compress_guid(!0, !0), "3$$$$$$$$$$$$$$$$$$$$$");
        assert_eq!(
            compress_guid(0x0123_4567_89AB_CDEF, 0x0123_4567_89AB_CDEF),
            "018qLdYQlDxm4ZHMU9gytl"
        );
    }

    #[test]
    fn a_key_gives_the_same_global_id_every_time_and_different_keys_different_ones() {
        let alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
        assert_eq!(
            guid_for("project", "entity/41"),
            guid_for("project", "entity/41")
        );
        let mut seen = BTreeSet::new();
        for i in 0..20000 {
            let guid = guid_for("project", &format!("entity/{i}"));
            // PJS003: 22 characters of the alphabet, the first 0 to 3.
            assert_eq!(guid.len(), 22);
            assert!("0123".contains(&guid[..1]), "{guid}");
            assert!(guid.chars().all(|c| alphabet.contains(c)), "{guid}");
            assert!(seen.insert(guid), "collision at {i}");
        }
        // The space keeps two projects apart, and a boundary cannot move
        // between the space and the key.
        assert_ne!(guid_for("a", "bc"), guid_for("ab", "c"));
        assert_ne!(
            guid_for("one project", "entity/1"),
            guid_for("another", "entity/1")
        );
    }
}

mod ifc_step_string {
    use super::*;

    // ISO 10303-21:2016, 6.4.3: a quote is doubled, a backslash doubled, and
    // a character outside the basic alphabet is \X2\ hex UTF-16 \X0\ within
    // the Basic Multilingual Plane, \X4\ hex UTF-32 \X0\ beyond it.
    #[test]
    fn encodes_quotes_backslashes_and_unicode_as_the_standard_says() {
        assert_eq!(step_string("LOT 42"), "'LOT 42'");
        assert_eq!(step_string("It's"), "'It''s'");
        assert_eq!(step_string("a\\b"), "'a\\\\b'");
        assert_eq!(step_string("45\u{b0}"), "'45\\X2\\00B0\\X0\\'"); // U+00B0
        assert_eq!(step_string("\u{b1}\u{e9}"), "'\\X2\\00B100E9\\X0\\'"); // one run
        assert_eq!(step_string("\u{1F600}"), "'\\X4\\0001F600\\X0\\'"); // U+1F600
        assert_eq!(step_string("a\nb"), "'a\\X2\\000A\\X0\\b'");
        // A byte that is not UTF-8 is the replacement character, not a guess.
        assert_eq!(step_string(b"\xFF"), "'\\X2\\FFFD\\X0\\'");
    }

    #[test]
    fn decoding_undoes_encoding_and_reads_the_other_escapes_too() {
        for text in [
            "LOT 42",
            "It's",
            "a\\b",
            "45\u{b0}",
            "\u{1F600} and \u{e9}",
            "a\nb",
            "",
        ] {
            let encoded = step_string(text);
            assert_eq!(
                decode_step_string(&encoded[1..encoded.len() - 1]),
                text,
                "{encoded}"
            );
        }
        // \X\hh is an ISO 8859-1 byte, \S\ the upper half of the alphabet
        // (0x30 + 128 = 0xB0, the degree sign), and a surrogate pair in \X2\
        // one character beyond the Basic Multilingual Plane.
        assert_eq!(decode_step_string("45\\X\\B0"), "45\u{b0}");
        assert_eq!(decode_step_string("45\\S\\0"), "45\u{b0}");
        assert_eq!(decode_step_string("\\X2\\D83DDE00\\X0\\"), "\u{1F600}");
        // Raw UTF-8, which other writers put in, is kept; a raw Latin-1 byte
        // is read as Latin-1.
        assert_eq!(decode_step_string("caf\u{e9}"), "caf\u{e9}");
        assert_eq!(decode_step_string(b"caf\xE9"), "caf\u{e9}");
    }

    #[test]
    fn counts_characters_not_bytes() {
        assert_eq!(character_count("abc"), 3);
        assert_eq!(character_count("45\u{b0}"), 3);
        assert_eq!(character_count("\u{1F600}"), 1);
    }
}

mod ifc_step_real {
    use super::*;

    // STEP's REAL: digits, a point, digits, optionally E and an exponent; the
    // point is required.
    #[test]
    fn writes_the_shortest_text_that_reads_back_and_always_a_point() {
        assert_eq!(step_real(1.0), "1.");
        assert_eq!(step_real(-2.0), "-2.");
        assert_eq!(step_real(0.1), "0.1");
        assert_eq!(step_real(1e-5), "1.E-05");
        assert_eq!(step_real(1e20), "1.E+20");
        assert_eq!(step_real(-0.0), "0.");
        assert_eq!(step_real(6250000.15), "6250000.15");
        for value in [6250000.123456789, 1.0 / 3.0, 334016.08, -1e-300, 2.5e-8] {
            let text = step_real(value);
            assert!(text.contains('.'), "{text}");
            // "1.E-05" is a number to the reader too, so it reads back whole.
            let back: f64 = text
                .parse()
                .unwrap_or_else(|error| panic!("{text}: {error}"));
            assert_eq!(back, value, "{text}");
        }
    }
}
