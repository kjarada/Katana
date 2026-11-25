//! The bytes of a V7 design (docs/dgn.md, "The format"): each number form
//! against bytes worked out by hand from its definition, and the orientation
//! of a 3D element against dgnlib's own quaternion.

pub mod support;

use katana_dgn::codes::{
    DEFAULT_COLOURS, colour_at, element_name, int32, int32_bytes, nearest_colour, radix50_text,
    range_bytes, vax_bytes, vax_double, word,
};
use katana_dgn::plane::{quaternion_in_plan, rotation_quaternion, rotation_in_plan};
use katana_entity::Color;
use katana_math::numerics::{DEG_TO_RAD, PI};
use support::{Random, near};

mod numbers {
    use super::*;

    #[test]
    fn an_integer_is_two_little_endian_words_the_high_word_first() {
        // 0x12345678: the high word 0x1234 as 34 12, then 0x5678 as 78 56
        // (dgnlibp.h:70, p[2] + p[3] << 8 + p[1] << 24 + p[0] << 16).
        assert_eq!(int32_bytes(0x1234_5678), [0x34, 0x12, 0x78, 0x56]);
        assert_eq!(int32(&[0x34, 0x12, 0x78, 0x56], 0), Some(0x1234_5678));
        // 65 538 = 0x0001_0002.
        assert_eq!(int32(&[0x01, 0x00, 0x02, 0x00], 0), Some(65_538));
        assert_eq!(int32_bytes(-1), [0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(int32_bytes(i32::MIN), [0x00, 0x80, 0x00, 0x00]);
        // Past the end of the bytes there is none.
        assert_eq!(int32(&[0x01, 0x00, 0x02], 0), None);
        assert_eq!(int32(&[0; 8], 6), None);
        assert_eq!(word(&[0x34, 0x12], 0), Some(0x1234));
        assert_eq!(word(&[0x34], 0), None);
    }

    #[test]
    fn a_range_corner_is_its_integer_with_the_sign_bit_flipped() {
        // Binary offset (dgnwrite.cpp:795): the plane's lowest coordinate is 0.
        assert_eq!(range_bytes(0), int32_bytes(i32::MIN));
        assert_eq!(range_bytes(i32::MIN), int32_bytes(0));
        // -1 ^ 0x8000_0000 = 0x7FFF_FFFF.
        assert_eq!(range_bytes(-1), [0xFF, 0x7F, 0xFF, 0xFF]);
    }

    #[test]
    fn vax_reals_of_hand_worked_values_read_and_write_exactly() {
        // A D-float is (-1)^s x 0.1f x 2^(e - 128): its first word holds the
        // sign, the 8-bit exponent and the fraction's top 7 bits.
        //   1.0 = 0.5 x 2^1: e = 129, the first longword 0x4080_0000.
        //   3.0 = 0.75 x 2^2: e = 130, the fraction's first bit set: 0x4140_0000.
        //  -2.0 = 0.5 x 2^2, negative: 0xC100_0000.
        let cases: [(f64, [u8; 8]); 4] = [
            (1.0, [0x80, 0x40, 0, 0, 0, 0, 0, 0]),
            (3.0, [0x40, 0x41, 0, 0, 0, 0, 0, 0]),
            (-2.0, [0x00, 0xC1, 0, 0, 0, 0, 0, 0]),
            (0.0, [0; 8]),
        ];
        for (value, bytes) in cases {
            assert_eq!(vax_bytes(value), Some(bytes), "{value}");
            assert_eq!(vax_double(&bytes, 0), Some(value), "{value}");
        }
        // The double after 1.0, 1 + 2^-52: its one fraction bit is the
        // D-float's 52nd, three places up from the 55th, so 8 in the low
        // longword - 00 00 08 00 in its words' order.
        let next = f64::from_bits(1.0_f64.to_bits() + 1);
        let bytes = [0x80, 0x40, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00];
        assert_eq!(vax_bytes(next), Some(bytes));
        assert_eq!(vax_double(&bytes, 0), Some(next));
    }

    #[test]
    fn every_double_the_format_holds_comes_back_bit_for_bit() {
        // A D-float has three more fraction bits than a double and an
        // exponent from 2^-128 to 2^127: every double in that range is one.
        // The values: survey coordinates in units of resolution, and doubles
        // of every exponent the range has, drawn from a fixed sequence.
        let mut values = vec![3_300_001_234.0, -62_500_000_000.0, 0.1, 1e-30, 1e30, -7.25e-12];
        let mut random = Random::new(0x5EED);
        let mut exponents = std::collections::BTreeSet::new();
        while values.len() < 20_000 {
            let exponent = 895 + random.below(1149 - 895 + 1) as u64; // 2^-128 .. 2^126
            let fraction = random.next_value() & ((1 << 52) - 1);
            let sign = random.next_value() & 1;
            values.push(f64::from_bits((sign << 63) | (exponent << 52) | fraction));
            exponents.insert(exponent);
        }
        // The draw reaches every exponent of the range at either end.
        assert!(exponents.contains(&895) && exponents.contains(&1149), "{}", exponents.len());
        for value in values {
            let bytes = vax_bytes(value).unwrap_or_else(|| panic!("{value:e} is held"));
            let back = vax_double(&bytes, 0).expect("a number");
            assert_eq!(back.to_bits(), value.to_bits(), "{value:e}");
        }
    }

    #[test]
    fn what_the_format_cannot_hold_is_refused_or_flushed_to_zero() {
        assert_eq!(vax_bytes(f64::NAN), None);
        assert_eq!(vax_bytes(f64::INFINITY), None);
        // 2^127 is past the largest D-float, (1 - 2^-56) x 2^127.
        assert_eq!(vax_bytes(2.0_f64.powi(127)), None);
        // Below 2^-128 a D-float has nothing: zero, as GDAL writes it.
        assert_eq!(vax_bytes(1e-40), Some([0; 8]));
        assert_eq!(vax_bytes(f64::MIN_POSITIVE / 4.0), Some([0; 8]));
        // The reserved operand - sign set, exponent 0 - is no number, and a
        // zero exponent with the sign clear is zero whatever its fraction.
        assert_eq!(vax_double(&[0x00, 0x80, 0, 0, 0, 0, 0, 0], 0), None);
        assert_eq!(vax_double(&[0x12, 0x00, 0x34, 0x56, 0, 0, 0, 0], 0), Some(0.0));
        assert_eq!(vax_double(&[0x80, 0x40, 0, 0], 0), None);
    }

    #[test]
    fn radix50_names_a_cell_three_characters_a_word() {
        // A digit of base 40: blank 0, A-Z 1-26, '$' 27, '.' 28, 0-9 30-39.
        // "TRE": 20 x 1600 + 18 x 40 + 5 = 32 725; "E1 ": 5 x 1600 + 31 x 40.
        assert_eq!(radix50_text(32_725), "TRE");
        assert_eq!(radix50_text(9_240), "E1 ");
        assert_eq!(radix50_text(0), "   ");
        // "$.9": 27 x 1600 + 28 x 40 + 39.
        assert_eq!(radix50_text(44_359), "$.9");
        // A first digit past 39 is no character: 65 535 = 40 x 1600 + 38 x
        // 40 + 15, so a blank, '8' and 'O'.
        assert_eq!(radix50_text(65_535), " 8O");
    }

    #[test]
    fn element_names_name_the_kinds_and_a_number_otherwise() {
        assert_eq!(element_name(3), "Line");
        assert_eq!(element_name(12), "Complex Chain");
        assert_eq!(element_name(17), "Text");
        assert_eq!(element_name(99), "Type 99");
    }
}

mod colours {
    use super::*;

    #[test]
    fn the_default_table_is_microstations_eight_colours_first() {
        // dgnhelp.cpp's abyDefaultPCT: white, blue, green, red, yellow,
        // magenta, orange, cyan, then shades; the background last.
        let first: Vec<Color> = (0..8).map(|index| colour_at(&DEFAULT_COLOURS, index)).collect();
        let rgb = |r, g, b| Color::new(r, g, b, 255);
        assert_eq!(
            first,
            vec![
                rgb(255, 255, 255),
                rgb(0, 0, 255),
                rgb(0, 255, 0),
                rgb(255, 0, 0),
                rgb(255, 255, 0),
                rgb(255, 0, 255),
                rgb(255, 127, 0),
                rgb(0, 255, 255),
            ]
        );
        assert_eq!(DEFAULT_COLOURS[255], [28, 0, 100]);
    }

    #[test]
    fn the_nearest_colour_is_the_closest_drawing_colour_never_the_background() {
        // (250, 5, 5) is 25 + 25 + 25 = 75 from red (3) and 100 + 25 + 25 =
        // 150 from the darker red at 19.
        assert_eq!(nearest_colour(&DEFAULT_COLOURS, Color::new(250, 5, 5, 255)), 3);
        let background = colour_at(&DEFAULT_COLOURS, 255);
        assert_ne!(nearest_colour(&DEFAULT_COLOURS, background), 255);
    }
}

mod orientation {
    use super::*;

    #[test]
    fn a_turn_written_as_dgnlib_writes_it_reads_back_as_the_turn() {
        // A quarter turn: cos(-45 deg) and sin(-45 deg) of 2^31 - 1
        // (dgnhelp.cpp:1084): 0.70710678118654752 x 2 147 483 647 =
        // 1 518 500 249.28, rounded.
        assert_eq!(
            rotation_quaternion(90.0 * DEG_TO_RAD),
            [1_518_500_249, 0, 0, -1_518_500_249]
        );
        for degrees in (-175..=180).step_by(5) {
            let radians = f64::from(degrees) * DEG_TO_RAD;
            let read = quaternion_in_plan(rotation_quaternion(radians)).expect("in plan");
            assert!(!read.mirrored);
            // A quaternion is held to 2^-31; the angle comes back within a
            // few of that.
            let difference = (read.angle - radians + PI).rem_euclid(2.0 * PI) - PI;
            near(difference, 0.0, 1e-8);
        }
    }

    #[test]
    fn a_plane_turned_over_is_mirrored_and_a_tilted_one_is_out_of_plan() {
        // Half a turn about X: (0, 1, 0, 0), the plane seen from below.
        let over = quaternion_in_plan([0, i32::MAX, 0, 0]).expect("in plan");
        assert!(over.mirrored);
        near(over.angle, 0.0, 1e-12);
        // An eighth of a turn about X tilts the plane by 45 degrees.
        let half = (22.5 * DEG_TO_RAD).cos() * f64::from(i32::MAX);
        let side = (22.5 * DEG_TO_RAD).sin() * f64::from(i32::MAX);
        assert_eq!(
            quaternion_in_plan([half.round() as i32, side.round() as i32, 0, 0]),
            None
        );
        // No quaternion at all is no turn.
        let none = quaternion_in_plan([0; 4]).expect("in plan");
        assert!(!none.mirrored);
        near(none.angle, 0.0, 0.0);
        // A 2D rotation: 1/360000 degree a step, 16 200 000 steps 45 degrees.
        near(rotation_in_plan(16_200_000).angle, PI / 4.0, 1e-15);
    }
}
