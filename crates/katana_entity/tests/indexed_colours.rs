//! The AutoCAD Colour Index table (indexed_colours.rs) against values from
//! outside this program:
//!
//! * 1-9: the named colours AutoCAD's colour dialog shows, red 255,0,0,
//!   yellow, green, cyan, blue, magenta, white (the foreground), grey 128 and
//!   grey 192 - 8 and 9 as ezdxf's AutoCAD 2020 model space palette gives
//!   them too (src/ezdxf/colors.py, 0x808080 and 0xC0C0C0).
//! * 10-255: the published table, acadrgb.txt
//!   (xarch.tu-graz.ac.at/autocad/docs/acadrgb.txt, as reproduced at
//!   gohtx.com/acadcolors.php), the entries quoted below by hand.
//! * The table's STRUCTURE, worked out independently of its numbers: index
//!   10 + 10h + s is hue 15h degrees, brightness (255, 189, 129, 104, 79)[s /
//!   2], full or a third saturated as s is even or odd. Every entry lies
//!   within a unit and a quarter of that in each channel - which a row
//!   copied into the wrong place would not.

use katana_entity::Color;
use katana_entity::indexed_colours::{
    COLOUR_BY_BLOCK, COLOUR_BY_LAYER, COLOUR_FOREGROUND, indexed_colour, nearest_indexed_colour,
};

fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color::new(r, g, b, 255))
}

/// The structure above, rounded to the nearest: the hue's pure components by
/// the six sectors of 60 degrees, each 15 degrees a quarter of one.
fn by_structure(index: i32) -> [f64; 3] {
    const VALUES: [f64; 5] = [255.0, 189.0, 129.0, 104.0, 79.0];
    let offset = index - 10;
    let hue = offset / 10;
    let step = offset % 10;
    let value = VALUES[(step / 2) as usize];
    let saturation = if step % 2 == 0 { 1.0 } else { 1.0 / 3.0 };
    let f = f64::from(hue % 4) / 4.0;
    let pure = match hue / 4 {
        0 => [1.0, f, 0.0],
        1 => [1.0 - f, 1.0, 0.0],
        2 => [0.0, 1.0, f],
        3 => [0.0, 1.0 - f, 1.0],
        4 => [f, 0.0, 1.0],
        _ => [1.0, 0.0, 1.0 - f],
    };
    pure.map(|component| value * (1.0 - saturation * (1.0 - component)))
}

mod indexed_colours {
    use super::*;

    #[test]
    fn one_to_nine_are_the_named_colours() {
        assert_eq!(indexed_colour(1), rgb(255, 0, 0));
        assert_eq!(indexed_colour(2), rgb(255, 255, 0));
        assert_eq!(indexed_colour(3), rgb(0, 255, 0));
        assert_eq!(indexed_colour(4), rgb(0, 255, 255));
        assert_eq!(indexed_colour(5), rgb(0, 0, 255));
        assert_eq!(indexed_colour(6), rgb(255, 0, 255));
        assert_eq!(indexed_colour(7), rgb(255, 255, 255));
        assert_eq!(indexed_colour(8), rgb(128, 128, 128));
        assert_eq!(indexed_colour(9), rgb(192, 192, 192));
        assert_eq!(COLOUR_FOREGROUND, 7);
    }

    #[test]
    fn the_hues_and_greys_are_the_published_tables() {
        // acadrgb.txt, quoted by hand: the first hue's five brightnesses, and
        // entries of the hues the index names of one real archive used.
        for (index, colour) in [
            (10, rgb(255, 0, 0)),
            (11, rgb(255, 170, 170)),
            (12, rgb(189, 0, 0)),
            (13, rgb(189, 126, 126)),
            (14, rgb(129, 0, 0)),
            (16, rgb(104, 0, 0)),
            (18, rgb(79, 0, 0)),
            (19, rgb(79, 53, 53)),
            (20, rgb(255, 63, 0)),
            (30, rgb(255, 127, 0)),
            (40, rgb(255, 191, 0)),
            (62, rgb(141, 189, 0)),
            (136, rgb(0, 104, 104)),
            (151, rgb(170, 212, 255)),
            (190, rgb(127, 0, 255)),
            (221, rgb(255, 170, 234)),
            (249, rgb(79, 53, 59)),
            (250, rgb(51, 51, 51)),
            (251, rgb(80, 80, 80)),
            (252, rgb(105, 105, 105)),
            (253, rgb(130, 130, 130)),
            (254, rgb(190, 190, 190)),
            (255, rgb(255, 255, 255)),
        ] {
            assert_eq!(indexed_colour(index), colour, "{index}");
        }
    }

    #[test]
    fn every_hue_lies_within_a_unit_and_a_quarter_of_the_tables_structure() {
        // The published table truncates rather than rounds, and in places
        // twice - its 189-bright quarter hue (index 22) is 46 where the
        // structure gives 189 x 0.25 = 47.25, as 63 (the full-bright 63.75,
        // truncated) x 189 / 255 = 46.7 truncated would - so an entry may lie
        // up to 1.25 below the exact structure. A row copied into the wrong
        // place lies tens of units away.
        let mut not_the_rounded_structure = 0;
        let mut a_unit_or_more_away = 0;
        for index in 10..250 {
            let colour = indexed_colour(index).expect("10 to 249 are colours");
            let worked = by_structure(index);
            let mut differs = false;
            let mut far = false;
            for (channel, exact) in [colour.r, colour.g, colour.b].into_iter().zip(worked) {
                let distance = (f64::from(channel) - exact).abs();
                assert!(
                    distance <= 1.25,
                    "{index}: {channel} against {exact} by the structure"
                );
                // Both whole numbers: they differ by a unit or not at all.
                differs |= (f64::from(channel) - exact.round()).abs() > 0.5;
                far |= distance >= 1.0;
            }
            not_the_rounded_structure += usize::from(differs);
            a_unit_or_more_away += usize::from(far);
        }
        // Counted by a script from the published values, not from this
        // table: 108 of the 240 are not the structure rounded to the nearest
        // (30's 127.5 is 127, 20's 63.75 is 63), and 18 of those lie a unit
        // or more from it (22, 24 and 26, and the same three places in hues
        // 7, 9, 15, 17 and 23). So the bound above is reached, and the table is the
        // published one, not the formula it replaced.
        assert_eq!(not_the_rounded_structure, 108);
        assert_eq!(a_unit_or_more_away, 18);
    }

    #[test]
    fn by_block_by_layer_and_out_of_range_indices_are_not_colours() {
        assert_eq!(COLOUR_BY_BLOCK, 0);
        assert_eq!(COLOUR_BY_LAYER, 256);
        for index in [0, 256, -1, 257, i32::MIN, i32::MAX] {
            assert!(indexed_colour(index).is_none(), "{index}");
        }
    }

    #[test]
    fn every_index_is_its_own_nearest_colour_and_black_is_the_foreground() {
        for index in 1..256 {
            let colour = indexed_colour(index).expect("1 to 255 are colours");
            // Several indices share an RGB (1 and 10 are both red): the
            // nearest is then the lower, which draws the same.
            assert_eq!(
                indexed_colour(nearest_indexed_colour(colour)),
                Some(colour),
                "{index}"
            );
        }
        assert_eq!(nearest_indexed_colour(Color::new(0, 0, 0, 255)), 7);
        // (250, 5, 5) is 5 from red in each channel and far from all else.
        assert_eq!(nearest_indexed_colour(Color::new(250, 5, 5, 255)), 1);
    }
}
