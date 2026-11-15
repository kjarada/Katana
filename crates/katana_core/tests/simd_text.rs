use katana_core::cpu_features::{
    SimdLevel, active_simd_level, detected_simd_level, set_simd_level,
};
use katana_core::text_encoding::{TextEncoding, decode_text, is_valid_utf8};
use std::sync::Mutex;

static LEVEL_LOCK: Mutex<()> = Mutex::new(());

fn at_simd_level<T>(level: SimdLevel, body: impl FnOnce() -> T) -> T {
    let _lock = LEVEL_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    struct Restore(SimdLevel);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_simd_level(self.0).unwrap();
        }
    }
    let _restore = Restore(set_simd_level(level).unwrap());
    assert_eq!(active_simd_level(), level);
    body()
}

fn simd_levels() -> Vec<SimdLevel> {
    let detected = detected_simd_level();
    if detected == SimdLevel::Scalar {
        vec![SimdLevel::Scalar]
    } else {
        vec![SimdLevel::Scalar, detected]
    }
}

fn kernel_level() -> SimdLevel {
    let level = detected_simd_level();
    assert_ne!(
        level,
        SimdLevel::Scalar,
        "a real SIMD backend is required for this parity test"
    );
    level
}

fn utf16(units: &[u16], little: bool, mark: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    for unit in mark
        .then_some(0xfeffu16)
        .into_iter()
        .chain(units.iter().copied())
    {
        bytes.extend_from_slice(&if little {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        });
    }
    bytes
}

#[derive(Debug, Eq, PartialEq)]
struct Outcome {
    ok: bool,
    text: String,
    encoding: TextEncoding,
    guessed: bool,
}

fn decode_at(level: SimdLevel, bytes: &[u8]) -> Outcome {
    at_simd_level(level, || match decode_text(bytes) {
        Ok(decoded) => Outcome {
            ok: true,
            text: decoded.text,
            encoding: decoded.encoding,
            guessed: decoded.guessed,
        },
        Err(error) => Outcome {
            ok: false,
            text: error.describe(),
            encoding: TextEncoding::Utf8,
            guessed: false,
        },
    })
}

fn valid_at(level: SimdLevel, bytes: &[u8]) -> bool {
    at_simd_level(level, || is_valid_utf8(bytes))
}

fn with(size: usize, at: usize, bytes: &[u8]) -> Vec<u8> {
    let mut text = vec![b'a'; size];
    text[at..at + bytes.len()].copy_from_slice(bytes);
    text
}

struct Generator(u32);
impl Generator {
    fn next(&mut self) -> usize {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as usize
    }
}

mod simd_text {
    use super::*;

    #[test]
    fn narrowing_writes_only_the_ascii_prefix_and_keeps_buffer_guards() {
        use katana_core::text_kernels::{narrow_ascii_utf16, narrow_ascii_utf16_scalar};
        for level in simd_levels() {
            for little in [true, false] {
                let units = [
                    vec![u16::from(b'a'); 35],
                    vec![0x0080],
                    vec![u16::from(b'b'); 37],
                ]
                .concat();
                let bytes = utf16(&units, little, false);
                let mut output = vec![0x55; units.len() + 2];
                let mut reference = output.clone();
                let count = at_simd_level(level, || {
                    narrow_ascii_utf16(&bytes, units.len(), little, &mut output[1..])
                });
                assert_eq!(count, 35);
                assert_eq!(
                    narrow_ascii_utf16_scalar(&bytes, units.len(), little, &mut reference[1..]),
                    35
                );
                assert_eq!(output, reference);
                assert_eq!(output[0], 0x55);
                assert!(output[1..36].iter().all(|byte| *byte == b'a'));
                assert!(output[36..].iter().all(|byte| *byte == 0x55));
                assert_eq!(
                    at_simd_level(level, || narrow_ascii_utf16(
                        &[],
                        0,
                        little,
                        &mut output[1..]
                    )),
                    0
                );
            }
        }
    }

    #[test]
    fn an_ascii_run_longer_than_a_block_is_narrowed_byte_for_byte_at_every_level() {
        let units = [
            vec![u16::from(b'A'); 70],
            vec![0x00e9],
            vec![u16::from(b'b'); 5],
        ]
        .concat();
        let bytes = utf16(&units, true, true);
        let expected = format!("{}\u{00e9}bbbbb", "A".repeat(70));
        for level in simd_levels() {
            let outcome = decode_at(level, &bytes);
            assert!(outcome.ok, "{}", outcome.text);
            assert_eq!(outcome.text, expected, "{level}");
        }
    }

    #[test]
    fn big_endian_ascii_is_taken_from_the_second_byte_of_each_unit() {
        let units = [
            vec![u16::from(b'x'); 33],
            vec![0x00e9],
            vec![u16::from(b'y'); 40],
        ]
        .concat();
        let bytes = utf16(&units, false, true);
        let expected = format!("{}\u{00e9}{}", "x".repeat(33), "y".repeat(40));
        let kernel = decode_at(kernel_level(), &bytes);
        assert!(kernel.ok, "{}", kernel.text);
        assert_eq!(kernel.text, expected);
        assert_eq!(kernel.encoding, TextEncoding::Utf16BigEndian);
        assert_eq!(kernel, decode_at(SimdLevel::Scalar, &bytes));
    }

    #[test]
    fn a_unit_is_ascii_only_when_below0x80_in_both_byte_orders() {
        for (unit, encoded) in [
            (0x007f, b"\x7f".as_slice()),
            (0x0080, b"\xc2\x80"),
            (0x0100, b"\xc4\x80"),
            (0x7f00, b"\xe7\xbc\x80"),
        ] {
            for little in [true, false] {
                let units = [
                    vec![u16::from(b'q'); 35],
                    vec![unit],
                    vec![u16::from(b'r'); 28],
                ]
                .concat();
                let bytes = utf16(&units, little, true);
                let kernel = decode_at(kernel_level(), &bytes);
                assert!(kernel.ok, "{}", kernel.text);
                let expected = [vec![b'q'; 35], encoded.to_vec(), vec![b'r'; 28]].concat();
                assert_eq!(kernel.text.as_bytes(), expected, "{unit:x} {little}");
                assert_eq!(kernel, decode_at(SimdLevel::Scalar, &bytes));
            }
        }
    }

    #[test]
    fn a_malformed_surrogate_is_reported_at_the_same_byte_at_every_level() {
        let low_alone = utf16(
            &[
                vec![u16::from(b'a'); 37],
                vec![0xdc00],
                vec![u16::from(b'a'); 30],
            ]
            .concat(),
            true,
            true,
        );
        let low_scalar = decode_at(SimdLevel::Scalar, &low_alone);
        assert!(!low_scalar.ok);
        assert!(
            low_scalar.text.contains("low surrogate with no high"),
            "{}",
            low_scalar.text
        );
        assert!(low_scalar.text.contains("[byte 74]"), "{}", low_scalar.text);
        assert_eq!(decode_at(kernel_level(), &low_alone), low_scalar);
        let high_alone = utf16(
            &[
                vec![u16::from(b'a'); 40],
                vec![0xd800, u16::from(b'a')],
                vec![u16::from(b'a'); 30],
            ]
            .concat(),
            true,
            true,
        );
        let high_scalar = decode_at(SimdLevel::Scalar, &high_alone);
        assert!(!high_scalar.ok);
        assert!(
            high_scalar.text.contains("[byte 80]"),
            "{}",
            high_scalar.text
        );
        assert_eq!(decode_at(kernel_level(), &high_alone), high_scalar);
        let truncated = utf16(
            &[vec![u16::from(b'a'); 64], vec![0xd83d]].concat(),
            true,
            true,
        );
        let truncated_scalar = decode_at(SimdLevel::Scalar, &truncated);
        assert!(!truncated_scalar.ok);
        assert!(
            truncated_scalar.text.contains("ends in the middle"),
            "{}",
            truncated_scalar.text
        );
        assert_eq!(decode_at(kernel_level(), &truncated), truncated_scalar);
    }

    #[test]
    fn decoding_gives_the_same_text_or_the_same_error_at_every_level() {
        let breakers = [
            0x00b0, 0x00e9, 0x0080, 0x07ff, 0x0800, 0x20ac, 0xfffd, 0xd83d, 0xde00, 0x7f41,
        ];
        let mut generator = Generator(2026);
        let mut compared = 0;
        for round in 0..3000 {
            let mut units = Vec::new();
            let pieces = 1 + generator.next() % 6;
            for _ in 0..pieces {
                let run = generator.next() % 100;
                for _ in 0..run {
                    units.push((0x20 + generator.next() % 0x5f) as u16);
                }
                let breaker = breakers[generator.next() % breakers.len()];
                units.push(breaker);
                if breaker == 0xd83d && !generator.next().is_multiple_of(4) {
                    units.push(0xde00);
                }
            }
            let little = generator.next().is_multiple_of(2);
            let mark = !generator.next().is_multiple_of(3);
            let bytes = utf16(&units, little, mark);
            assert_eq!(
                decode_at(kernel_level(), &bytes),
                decode_at(SimdLevel::Scalar, &bytes),
                "round {round}"
            );
            compared += 1;
        }
        assert_eq!(compared, 3000);
    }

    #[test]
    fn utf8_validation_gives_the_same_answer_at_every_level() {
        let cases = [
            (vec![b'a'; 100], true),
            (
                [vec![b'a'; 100], b"\xc3\xa9".to_vec(), vec![b'b'; 40]].concat(),
                true,
            ),
            (
                [vec![b'a'; 70], b"\x80".to_vec(), vec![b'a'; 10]].concat(),
                false,
            ),
            ([vec![b'a'; 64], b"\xc3".to_vec()].concat(), false),
            (
                [vec![b'a'; 33], b"\xed\xa0\x80".to_vec(), vec![b'a'; 40]].concat(),
                false,
            ),
            (
                [vec![b'a'; 31], b"\xff".to_vec(), vec![b'a'; 40]].concat(),
                false,
            ),
            ([vec![b'a'; 127], b"\xe2\x82\xac".to_vec()].concat(), true),
        ];
        for (text, valid) in cases {
            assert_eq!(valid_at(SimdLevel::Scalar, &text), valid, "{}", text.len());
            assert_eq!(valid_at(kernel_level(), &text), valid, "{}", text.len());
        }
        for at in 0..200 {
            for insert in [b"\x80".as_slice(), b"\xc3\xa9"] {
                let text = [vec![b'z'; at], insert.to_vec(), vec![b'z'; 200 - at]].concat();
                let scalar = valid_at(SimdLevel::Scalar, &text);
                assert_eq!(scalar, insert.len() == 2, "{at}");
                assert_eq!(valid_at(kernel_level(), &text), scalar, "{at}");
            }
        }
    }

    #[test]
    fn text_dense_with_multibyte_characters_validates_alike_at_every_level() {
        for run in 0..=40 {
            let mut text = Vec::new();
            while text.len() < 160 {
                text.extend_from_slice(b"\xc3\xa9");
                text.extend(vec![b'a'; run]);
            }
            for cut in text.len() - 40..=text.len() {
                let prefix = &text[..cut];
                let whole = !(cut > 0 && prefix.last() == Some(&0xc3));
                let lone = [prefix.to_vec(), b"a\x80".to_vec(), vec![b'b'; run]].concat();
                let truncated = [prefix.to_vec(), vec![b'b'; run], b"\xc3".to_vec()].concat();
                for level in simd_levels() {
                    assert_eq!(
                        valid_at(level, prefix),
                        whole,
                        "run {run} cut {cut} at {level}"
                    );
                    assert!(!valid_at(level, &lone), "run {run} cut {cut} at {level}");
                    assert!(
                        !valid_at(level, &truncated),
                        "run {run} cut {cut} at {level}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_validator_finds_each_kind_of_error_worked_by_hand_at_a_block_edge() {
        let cases = [
            (
                "a four-byte character across the 64-byte step",
                with(100, 62, &[0xf0, 0x9f, 0x98, 0x80]),
                true,
            ),
            (
                "a three-byte character across the final window",
                with(100, 66, &[0xe2, 0x82, 0xac]),
                true,
            ),
            (
                "a two-byte character that ends the text",
                with(100, 98, &[0xc3, 0xa9]),
                true,
            ),
            (
                "a code point above U+10FFFF",
                with(100, 62, &[0xf4, 0x90, 0x80, 0x80]),
                false,
            ),
            (
                "the lead F5",
                with(100, 63, &[0xf5, 0x80, 0x80, 0x80]),
                false,
            ),
            (
                "an overlong three-byte form",
                with(100, 31, &[0xe0, 0x9f, 0xbf]),
                false,
            ),
            (
                "an overlong four-byte form",
                with(100, 30, &[0xf0, 0x8f, 0xbf, 0xbf]),
                false,
            ),
            (
                "an overlong two-byte form",
                with(100, 31, &[0xc1, 0xbf]),
                false,
            ),
            (
                "a surrogate half",
                with(100, 63, &[0xed, 0xa0, 0x80]),
                false,
            ),
            ("a continuation with no lead", with(100, 64, &[0x80]), false),
            (
                "a two-byte character with a second continuation",
                with(100, 31, &[0xc3, 0xa9, 0xa9]),
                false,
            ),
            (
                "a three-byte character cut short by ASCII",
                with(100, 62, &[0xe2, 0x82]),
                false,
            ),
            (
                "a lead open before a step of ASCII",
                with(128, 63, &[0xe2]),
                false,
            ),
            (
                "a four-byte character with the text ending after three bytes",
                with(100, 97, &[0xf0, 0x9f, 0x98]),
                false,
            ),
            (
                "a lead that ends a text of exactly one step",
                with(64, 63, &[0xc3]),
                false,
            ),
            (
                "a two-byte character that ends a text of exactly one step",
                with(64, 62, &[0xc3, 0xa9]),
                true,
            ),
            (
                "a two-byte character across a 16-byte edge in the step",
                with(100, 15, &[0xc3, 0xa9]),
                true,
            ),
            (
                "a three-byte character across the NEON final window",
                with(100, 82, &[0xe2, 0x82, 0xac]),
                true,
            ),
            (
                "a lead open before a 16-byte block of ASCII",
                with(100, 79, &[0xe2]),
                false,
            ),
            (
                "a four-byte character across the 16-byte edge after the step",
                with(100, 77, &[0xf0, 0x9f, 0x98, 0x80]),
                true,
            ),
        ];
        for (what, text, valid) in cases {
            assert_eq!(valid_at(SimdLevel::Scalar, &text), valid, "{what}");
            if kernel_level() != SimdLevel::Scalar {
                assert_eq!(valid_at(kernel_level(), &text), valid, "{what}");
            }
        }
    }

    #[test]
    fn every_pair_of_bytes_at_every_block_edge_is_judged_as_the_byte_loop_judges_it() {
        let places = [0, 15, 31, 47, 63, 67, 79, 83, 95, 98];
        let mut differ = 0;
        let mut valid = 0;
        let mut examples = Vec::new();
        for first in 0..=255u8 {
            for second in 0..=255u8 {
                for at in places {
                    let text = with(100, at, &[first, second]);
                    let scalar = valid_at(SimdLevel::Scalar, &text);
                    valid += usize::from(scalar);
                    if valid_at(kernel_level(), &text) != scalar {
                        differ += 1;
                        if differ <= 10 {
                            examples.push(format!("bytes {first} {second} at {at}"));
                        }
                    }
                }
            }
        }
        assert_eq!(differ, 0, "{examples:?}");
        assert_eq!(valid, 18304 * 10);
    }

    #[test]
    fn every_lead_with_each_kind_of_follower_is_judged_as_the_byte_loop_judges_it() {
        let followers = [
            0x41, 0x7f, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xbf, 0xc0, 0xc2, 0xdf, 0xe0, 0xed, 0xf0,
            0xf4, 0xff,
        ];
        let firsts: Vec<u8> = [0x80, 0xbf].into_iter().chain(0xc0..=0xff).collect();
        let mut differ = 0;
        let mut valid = 0;
        let mut examples = Vec::new();
        let mut compare = |text: Vec<u8>| {
            let scalar = valid_at(SimdLevel::Scalar, &text);
            valid += usize::from(scalar);
            if valid_at(kernel_level(), &text) != scalar {
                differ += 1;
                if differ <= 10 {
                    examples.push(
                        text.into_iter()
                            .filter(|byte| *byte != b'a')
                            .collect::<Vec<_>>(),
                    );
                }
            }
        };
        for first in firsts {
            for second in followers {
                for third in followers {
                    compare(with(100, 97, &[first, second, third]));
                    for fourth in followers {
                        compare(with(100, 62, &[first, second, third, fourth]));
                    }
                }
            }
        }
        assert_eq!(differ, 0, "{examples:?}");
        assert!(valid > 0);
    }

    #[test]
    fn generated_text_with_damage_is_judged_as_the_byte_loop_judges_it() {
        let pieces: &[&[u8]] = &[
            b"a",
            b" ",
            b"\r\n",
            b"\xc2\x80",
            b"\xc3\xa9",
            b"\xdf\xbf",
            b"\xe0\xa0\x80",
            b"\xe2\x82\xac",
            b"\xed\x9f\xbf",
            b"\xee\x80\x80",
            b"\xef\xbf\xbf",
            b"\xf0\x90\x80\x80",
            b"\xf0\x9f\x98\x80",
            b"\xf4\x8f\xbf\xbf",
        ];
        let mut generator = Generator(924);
        let mut compared = 0;
        let mut valid = 0;
        for round in 0..6000 {
            let mut text = Vec::new();
            let size = 64 + generator.next() % 337;
            while text.len() < size {
                if generator.next().is_multiple_of(3) {
                    text.extend(vec![b'q'; generator.next() % 40]);
                } else {
                    text.extend_from_slice(pieces[generator.next() % pieces.len()]);
                }
            }
            let damage = generator.next() % 3;
            for _ in 0..damage {
                let at = generator.next() % text.len();
                match generator.next() % 4 {
                    0 => text[at] = generator.next() as u8,
                    1 => {
                        text.remove(at);
                    }
                    2 => text.insert(at, (0x80 + generator.next() % 0x80) as u8),
                    _ => text.resize(at.max(64), 0),
                }
            }
            let scalar = valid_at(SimdLevel::Scalar, &text);
            valid += usize::from(scalar);
            assert_eq!(valid_at(kernel_level(), &text), scalar, "round {round}");
            compared += 1;
        }
        assert_eq!(compared, 6000);
        assert!(valid > 1000);
        assert!(valid < 5000);
    }
}
