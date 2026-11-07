//! The twin of tests/dxf/test_codes.cpp: the indexed colours, true colour,
//! lineweights, the text codes and the exact degrees, against values worked
//! out by hand.

use katana_dxf::{
    RADIANS_PER_DEGREE, encode_text, exact_degrees, indexed_colour, nearest_indexed_colour,
    nearest_lineweight, plain_mtext, plain_text, true_colour,
};
use katana_entity::Color;
use katana_math::numerics::PI;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

mod dxf_codes {
    use super::*;

    #[test]
    fn indexed_colours_one_to_nine_are_the_named_colours() {
        assert_eq!(indexed_colour(1), Some(rgb(255, 0, 0)));
        assert_eq!(indexed_colour(2), Some(rgb(255, 255, 0)));
        assert_eq!(indexed_colour(3), Some(rgb(0, 255, 0)));
        assert_eq!(indexed_colour(4), Some(rgb(0, 255, 255)));
        assert_eq!(indexed_colour(5), Some(rgb(0, 0, 255)));
        assert_eq!(indexed_colour(6), Some(rgb(255, 0, 255)));
        assert_eq!(indexed_colour(7), Some(rgb(255, 255, 255)));
        assert_eq!(indexed_colour(8), Some(rgb(128, 128, 128)));
        assert_eq!(indexed_colour(9), Some(rgb(192, 192, 192)));
    }

    #[test]
    fn by_block_by_layer_and_out_of_range_indices_are_not_colours() {
        assert!(indexed_colour(0).is_none());
        assert!(indexed_colour(256).is_none());
        assert!(indexed_colour(-1).is_none());
    }

    #[test]
    fn the_hue_range_steps_fifteen_degrees_at_five_brightnesses() {
        // Index 10 is hue 0 at full brightness: red. 11 is the same hue a third
        // saturated: 255 * (1 - (1 - 0) / 3) = 170 in the other channels.
        assert_eq!(indexed_colour(10), Some(rgb(255, 0, 0)));
        assert_eq!(indexed_colour(11), Some(rgb(255, 170, 170)));
        // 12 is the second brightness, 189.
        assert_eq!(indexed_colour(12), Some(rgb(189, 0, 0)));
        // Hue 4 (60 degrees) is yellow, hue 8 green, hue 16 blue.
        assert_eq!(indexed_colour(50), Some(rgb(255, 255, 0)));
        assert_eq!(indexed_colour(90), Some(rgb(0, 255, 0)));
        assert_eq!(indexed_colour(170), Some(rgb(0, 0, 255)));
        // Hue 1 (15 degrees): red full, green a quarter of 255 = 63.75 -> 64.
        assert_eq!(indexed_colour(20), Some(rgb(255, 64, 0)));
        // The grey ramp.
        assert_eq!(indexed_colour(250), Some(rgb(51, 51, 51)));
        assert_eq!(indexed_colour(255), Some(rgb(255, 255, 255)));
    }

    #[test]
    fn every_indexed_colour_is_its_own_nearest_indexed_colour() {
        for index in 1..256 {
            let colour = indexed_colour(index).expect("1 to 255 are colours");
            // Several indices share an RGB (1 and 10 are both red); the nearest is
            // then the lower index, which draws the same.
            assert_eq!(
                indexed_colour(nearest_indexed_colour(colour)),
                Some(colour),
                "{index}"
            );
        }
    }

    #[test]
    fn black_is_written_as_the_foreground_colour() {
        assert_eq!(nearest_indexed_colour(rgb(0, 0, 0)), 7);
    }

    #[test]
    fn a_colour_between_indices_goes_to_the_nearest() {
        // (250, 5, 5) is 5.0 from red (index 1) in each of three channels and
        // much further from anything else; red's lowest index is 1.
        assert_eq!(nearest_indexed_colour(rgb(250, 5, 5)), 1);
    }

    #[test]
    fn true_colour_is_red_green_blue_in_the_low_twenty_four_bits() {
        // 16744448 = 0xFF8000.
        assert_eq!(true_colour(16744448), rgb(255, 128, 0));
        assert_eq!(true_colour(255), rgb(0, 0, 255));
    }

    #[test]
    fn lineweights_snap_to_the_nearest_standard_weight() {
        assert_eq!(nearest_lineweight(0.25), 25);
        assert_eq!(nearest_lineweight(0.26), 25); // 1 from 25, 4 from 30
        assert_eq!(nearest_lineweight(0.28), 30); // 3 from 25, 2 from 30
        assert_eq!(nearest_lineweight(0.5), 50);
        assert_eq!(nearest_lineweight(5.0), 211); // past the heaviest
        assert_eq!(nearest_lineweight(0.0), 0);
        assert_eq!(nearest_lineweight(-1.0), 0);
    }

    #[test]
    fn percent_codes_become_the_signs_they_name() {
        assert_eq!(plain_text("45%%d"), "45\u{00B0}");
        assert_eq!(plain_text("%%p0.005"), "\u{00B1}0.005");
        assert_eq!(plain_text("%%c300"), "\u{2205}300");
        assert_eq!(plain_text("100%%%"), "100%");
        assert_eq!(plain_text("%%uUNDER%%u"), "UNDER");
        assert_eq!(plain_text("%%065"), "A");
        // A lone percent sign is a percent sign.
        assert_eq!(plain_text("50%"), "50%");
    }

    #[test]
    fn unicode_escapes_become_the_characters_they_name() {
        assert_eq!(plain_text("B\\U+00F6schung"), "B\u{00F6}schung");
        // Not four hex digits: left as it is.
        assert_eq!(plain_text("\\U+00G0"), "\\U+00G0");
    }

    #[test]
    fn m_text_formatting_codes_are_stripped() {
        assert_eq!(
            plain_mtext("{\\fArial|b1|i0|c0|p34;SITE}"),
            "SITE".as_bytes()
        );
        assert_eq!(plain_mtext("\\H2.5;BIG\\H1x;small"), "BIGsmall".as_bytes());
        assert_eq!(plain_mtext("\\C1;red\\C256;"), "red".as_bytes());
        assert_eq!(
            plain_mtext("\\LUnder\\l \\Oover\\o"),
            "Under over".as_bytes()
        );
        assert_eq!(plain_mtext("\\A1;\\pxqc;centred"), "centred".as_bytes());
        assert_eq!(plain_mtext("one\\Ptwo"), "one\ntwo".as_bytes());
        assert_eq!(plain_mtext("a\\~b"), "a\u{00A0}b".as_bytes());
        assert_eq!(plain_mtext("\\\\server\\{x\\}"), "\\server{x}".as_bytes());
        assert_eq!(plain_mtext("\\S1/2;"), "1/2".as_bytes());
        assert_eq!(plain_mtext("\\S+0.1^-0.1;"), "+0.1/-0.1".as_bytes());
        assert_eq!(
            plain_mtext("\\U+00B0 and %%d"),
            "\u{00B0} and \u{00B0}".as_bytes()
        );
    }

    #[test]
    fn encoded_text_is_ascii_that_reads_back_as_the_original() {
        assert_eq!(encode_text("LOT 42"), "LOT 42");
        assert_eq!(encode_text("45\u{00B0}"), "45%%d");
        assert_eq!(encode_text("\u{00B1}\u{2205}"), "%%p%%c");
        assert_eq!(encode_text("B\u{00F6}schung"), "B\\U+00F6schung");
        // A percent sign another follows would begin a code: "%%%" is one.
        assert_eq!(encode_text("50%%d"), "50%%%%d");
        assert_eq!(plain_text(&encode_text("50%%d")), "50%%d");
        // A line break cannot be in a TEXT string.
        assert_eq!(encode_text("a\tb"), "a b");
        for sample in [
            "R\u{00E9}sum\u{00E9} \u{00B0} \u{00B1} \u{2205} 100%",
            "\u{6E2C}\u{91CF}",
        ] {
            assert_eq!(plain_text(&encode_text(sample)), sample);
        }
    }

    #[test]
    fn exact_degrees_never_read_back_worse_than_the_nearest_and_often_exactly() {
        // The reader multiplies by pi/180. Not every double is the product of a
        // double and pi/180 - below a power of two the products are spaced more
        // widely than the doubles - so exactness cannot always be had; but the
        // degrees chosen never read back further off than the nearest degrees
        // would, and read back exactly more often. Across a turn in 10 000 steps:
        let mut exact = 0;
        let mut nearest_exact = 0;
        for i in 0..10000 {
            let radians = 2.0 * PI * f64::from(i) / 10000.0;
            let chosen = exact_degrees(radians) * RADIANS_PER_DEGREE;
            let nearest = radians * (180.0 / PI) * RADIANS_PER_DEGREE;
            assert!((chosen - radians).abs() <= (nearest - radians).abs(), "{i}");
            exact += i32::from(chosen == radians);
            nearest_exact += i32::from(nearest == radians);
        }
        assert!(exact > nearest_exact);
    }
}
