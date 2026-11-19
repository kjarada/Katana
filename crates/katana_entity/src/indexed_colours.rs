//! The AutoCAD Colour Index: 255 numbered colours, and the RGB behind each.
//!
//! Two readers name a colour by its index. DXF's group 62 is one; the archive
//! format's colour NAMES "acad 001" to "acad 255" are the other (its meshes
//! use them). The DXF and archive layers are siblings that may not see each
//! other (tools/check_layering.cmake), so the table lives here, below both:
//! one table, so that index 30 cannot be one orange in a DXF and another in an
//! archive. It began as katana_dxf's, which still offers these names.
//!
//! THE SOURCE. 10-249 and the grey ramp 250-255 are the published table,
//! "acadrgb.txt" (xarch.tu-graz.ac.at/autocad/docs/acadrgb.txt, as
//! reproduced at gohtx.com/acadcolors.php), copied, not computed. 1-9 are the
//! named colours AutoCAD shows today: that older table gives 8 and 9 as 65
//! and 128 grey, where the current program (and ezdxf's AutoCAD 2020 model
//! space palette, src/ezdxf/colors.py) gives 128 and 192.
//!
//! Until 2026-11-19 the hues were COMPUTED - 24 hues 15 degrees apart, five
//! brightnesses (255, 189, 129, 104, 79), full and a third saturated - which
//! is the table's structure, but not its numbers: the published table
//! truncates, in places twice, so rounded to the nearest 108 of the 240 came
//! out a unit above it in one channel (30 was 255,128,0 where the table says
//! 255,127,0). The structure is kept as a test that every entry lies within a
//! unit and a quarter of it, which is what catches a row copied into the
//! wrong place. *Rejected: ezdxf's DXF_DEFAULT_COLORS*,
//! whose 10-249 are another palette altogether (index 12 is 165,0,0 there,
//! 189,0,0 in the published table and in AutoCAD's own colour dialog).

use crate::entity::Color;

/// Group 62 values (and archive "acad 000" / "acad 256") that are not
/// colours: the entity takes its block's or its layer's.
pub const COLOUR_BY_BLOCK: i32 = 0;
pub const COLOUR_BY_LAYER: i32 = 256;
/// Index 7 is the drawing's foreground: white on a dark background, black on a
/// light one. Katana draws it white.
pub const COLOUR_FOREGROUND: i32 = 7;

// Each hue is ten entries: five brightnesses (255, 189, 129, 104, 79), each
// full and then a third saturated. Laid out by hue, which rustfmt would undo.
#[rustfmt::skip]
static TABLE: [[u8; 3]; 256] = [
    // 0 (ByBlock, never a colour), then 1-9.
    [  0,   0,   0], [255,   0,   0], [255, 255,   0], [  0, 255,   0], [  0, 255, 255],
    [  0,   0, 255], [255,   0, 255], [255, 255, 255], [128, 128, 128], [192, 192, 192],
    // 10-19: 0 degrees
    [255,   0,   0], [255, 170, 170], [189,   0,   0], [189, 126, 126], [129,   0,   0],
    [129,  86,  86], [104,   0,   0], [104,  69,  69], [ 79,   0,   0], [ 79,  53,  53],
    // 20-29: 15 degrees
    [255,  63,   0], [255, 191, 170], [189,  46,   0], [189, 141, 126], [129,  31,   0],
    [129,  96,  86], [104,  25,   0], [104,  78,  69], [ 79,  19,   0], [ 79,  59,  53],
    // 30-39: 30 degrees
    [255, 127,   0], [255, 212, 170], [189,  94,   0], [189, 157, 126], [129,  64,   0],
    [129, 107,  86], [104,  52,   0], [104,  86,  69], [ 79,  39,   0], [ 79,  66,  53],
    // 40-49: 45 degrees
    [255, 191,   0], [255, 234, 170], [189, 141,   0], [189, 173, 126], [129,  96,   0],
    [129, 118,  86], [104,  78,   0], [104,  95,  69], [ 79,  59,   0], [ 79,  73,  53],
    // 50-59: 60 degrees
    [255, 255,   0], [255, 255, 170], [189, 189,   0], [189, 189, 126], [129, 129,   0],
    [129, 129,  86], [104, 104,   0], [104, 104,  69], [ 79,  79,   0], [ 79,  79,  53],
    // 60-69: 75 degrees
    [191, 255,   0], [234, 255, 170], [141, 189,   0], [173, 189, 126], [ 96, 129,   0],
    [118, 129,  86], [ 78, 104,   0], [ 95, 104,  69], [ 59,  79,   0], [ 73,  79,  53],
    // 70-79: 90 degrees
    [127, 255,   0], [212, 255, 170], [ 94, 189,   0], [157, 189, 126], [ 64, 129,   0],
    [107, 129,  86], [ 52, 104,   0], [ 86, 104,  69], [ 39,  79,   0], [ 66,  79,  53],
    // 80-89: 105 degrees
    [ 63, 255,   0], [191, 255, 170], [ 46, 189,   0], [141, 189, 126], [ 31, 129,   0],
    [ 96, 129,  86], [ 25, 104,   0], [ 78, 104,  69], [ 19,  79,   0], [ 59,  79,  53],
    // 90-99: 120 degrees
    [  0, 255,   0], [170, 255, 170], [  0, 189,   0], [126, 189, 126], [  0, 129,   0],
    [ 86, 129,  86], [  0, 104,   0], [ 69, 104,  69], [  0,  79,   0], [ 53,  79,  53],
    // 100-109: 135 degrees
    [  0, 255,  63], [170, 255, 191], [  0, 189,  46], [126, 189, 141], [  0, 129,  31],
    [ 86, 129,  96], [  0, 104,  25], [ 69, 104,  78], [  0,  79,  19], [ 53,  79,  59],
    // 110-119: 150 degrees
    [  0, 255, 127], [170, 255, 212], [  0, 189,  94], [126, 189, 157], [  0, 129,  64],
    [ 86, 129, 107], [  0, 104,  52], [ 69, 104,  86], [  0,  79,  39], [ 53,  79,  66],
    // 120-129: 165 degrees
    [  0, 255, 191], [170, 255, 234], [  0, 189, 141], [126, 189, 173], [  0, 129,  96],
    [ 86, 129, 118], [  0, 104,  78], [ 69, 104,  95], [  0,  79,  59], [ 53,  79,  73],
    // 130-139: 180 degrees
    [  0, 255, 255], [170, 255, 255], [  0, 189, 189], [126, 189, 189], [  0, 129, 129],
    [ 86, 129, 129], [  0, 104, 104], [ 69, 104, 104], [  0,  79,  79], [ 53,  79,  79],
    // 140-149: 195 degrees
    [  0, 191, 255], [170, 234, 255], [  0, 141, 189], [126, 173, 189], [  0,  96, 129],
    [ 86, 118, 129], [  0,  78, 104], [ 69,  95, 104], [  0,  59,  79], [ 53,  73,  79],
    // 150-159: 210 degrees
    [  0, 127, 255], [170, 212, 255], [  0,  94, 189], [126, 157, 189], [  0,  64, 129],
    [ 86, 107, 129], [  0,  52, 104], [ 69,  86, 104], [  0,  39,  79], [ 53,  66,  79],
    // 160-169: 225 degrees
    [  0,  63, 255], [170, 191, 255], [  0,  46, 189], [126, 141, 189], [  0,  31, 129],
    [ 86,  96, 129], [  0,  25, 104], [ 69,  78, 104], [  0,  19,  79], [ 53,  59,  79],
    // 170-179: 240 degrees
    [  0,   0, 255], [170, 170, 255], [  0,   0, 189], [126, 126, 189], [  0,   0, 129],
    [ 86,  86, 129], [  0,   0, 104], [ 69,  69, 104], [  0,   0,  79], [ 53,  53,  79],
    // 180-189: 255 degrees
    [ 63,   0, 255], [191, 170, 255], [ 46,   0, 189], [141, 126, 189], [ 31,   0, 129],
    [ 96,  86, 129], [ 25,   0, 104], [ 78,  69, 104], [ 19,   0,  79], [ 59,  53,  79],
    // 190-199: 270 degrees
    [127,   0, 255], [212, 170, 255], [ 94,   0, 189], [157, 126, 189], [ 64,   0, 129],
    [107,  86, 129], [ 52,   0, 104], [ 86,  69, 104], [ 39,   0,  79], [ 66,  53,  79],
    // 200-209: 285 degrees
    [191,   0, 255], [234, 170, 255], [141,   0, 189], [173, 126, 189], [ 96,   0, 129],
    [118,  86, 129], [ 78,   0, 104], [ 95,  69, 104], [ 59,   0,  79], [ 73,  53,  79],
    // 210-219: 300 degrees
    [255,   0, 255], [255, 170, 255], [189,   0, 189], [189, 126, 189], [129,   0, 129],
    [129,  86, 129], [104,   0, 104], [104,  69, 104], [ 79,   0,  79], [ 79,  53,  79],
    // 220-229: 315 degrees
    [255,   0, 191], [255, 170, 234], [189,   0, 141], [189, 126, 173], [129,   0,  96],
    [129,  86, 118], [104,   0,  78], [104,  69,  95], [ 79,   0,  59], [ 79,  53,  73],
    // 230-239: 330 degrees
    [255,   0, 127], [255, 170, 212], [189,   0,  94], [189, 126, 157], [129,   0,  64],
    [129,  86, 107], [104,   0,  52], [104,  69,  86], [ 79,   0,  39], [ 79,  53,  66],
    // 240-249: 345 degrees
    [255,   0,  63], [255, 170, 191], [189,   0,  46], [189, 126, 141], [129,   0,  31],
    [129,  86,  96], [104,   0,  25], [104,  69,  78], [ 79,   0,  19], [ 79,  53,  59],
    // 250-255: the grey ramp.
    [ 51,  51,  51], [ 80,  80,  80], [105, 105, 105], [130, 130, 130], [190, 190, 190],
    [255, 255, 255],
];

fn entry(index: usize) -> Color {
    let [r, g, b] = TABLE[index];
    Color::new(r, g, b, 255)
}

/// The RGB of indexed colour 1-255. None for 0 (ByBlock), 256 (ByLayer) and
/// anything outside the range.
pub fn indexed_colour(index: i32) -> Option<Color> {
    if !(1..=255).contains(&index) {
        return None;
    }
    Some(entry(index as usize))
}

/// The index whose RGB is nearest `colour` (squared distance; the lower index
/// on a tie). Black is the foreground, 7, since no index is black and the
/// nearest dark red would be a surprise.
pub fn nearest_indexed_colour(colour: Color) -> i32 {
    if colour.r == 0 && colour.g == 0 && colour.b == 0 {
        return COLOUR_FOREGROUND;
    }
    let mut best = COLOUR_FOREGROUND;
    let mut best_distance = i64::MAX;
    for index in 1..TABLE.len() {
        let candidate = entry(index);
        let dr = i64::from(candidate.r) - i64::from(colour.r);
        let dg = i64::from(candidate.g) - i64::from(colour.g);
        let db = i64::from(candidate.b) - i64::from(colour.b);
        let distance = dr * dr + dg * dg + db * db;
        if distance < best_distance {
            // strict: the lower index wins a tie
            best_distance = distance;
            best = index as i32;
            if distance == 0 {
                break;
            }
        }
    }
    best
}
