//! The bytes of a MicroStation V7 design file (the Intergraph Standard File
//! Format, ISFF): the numbers, the element kinds and the default colours,
//! exposed so that the tests check them against hand-worked values.
//!
//! The layout is the one GDAL's dgnlib reads and writes (Frank Warmerdam's
//! library, MIT/X licence, in GDAL 3.13.2 as `ogr/ogrsf_frmts/dgn/`); every
//! offset below names the line of dgnlib it was read from. dgnlib is the one
//! open implementation that has written files MicroStation opens for twenty
//! years, which is why it is the reference and not a reading of the format
//! of our own.
//!
//! Three encodings, none of them the machine's:
//!
//! ```text
//!   a 32-bit integer   two little-endian 16-bit words, the HIGH word first
//!                      (the PDP-11's order, dgnlibp.h:70 DGN_INT32);
//!   a real             a VAX D-float: sign, an 8-bit exponent biased by 128
//!                      and a 55-bit fraction after a hidden 0.1, in four
//!                      little-endian words, the most significant first
//!                      (GDAL's port/cpl_vax.cpp);
//!   a range corner     the integer with its sign bit flipped - "binary
//!                      offset", 0 at the bottom of the design plane
//!                      (dgnwrite.cpp:795 DGNWriteBounds).
//! ```

use katana_entity::Color;

// ---- element kinds (dgnlib.h:596-625) ---------------------------------------------------

pub const TYPE_CELL_LIBRARY: u8 = 1;
pub const TYPE_CELL_HEADER: u8 = 2;
pub const TYPE_LINE: u8 = 3;
pub const TYPE_LINE_STRING: u8 = 4;
pub const TYPE_GROUP_DATA: u8 = 5;
pub const TYPE_SHAPE: u8 = 6;
pub const TYPE_TEXT_NODE: u8 = 7;
pub const TYPE_DIGITIZER_SETUP: u8 = 8;
pub const TYPE_DESIGN_HEADER: u8 = 9;
pub const TYPE_LEVEL_SYMBOLOGY: u8 = 10;
pub const TYPE_CURVE: u8 = 11;
pub const TYPE_COMPLEX_CHAIN: u8 = 12;
pub const TYPE_COMPLEX_SHAPE: u8 = 14;
pub const TYPE_ELLIPSE: u8 = 15;
pub const TYPE_ARC: u8 = 16;
pub const TYPE_TEXT: u8 = 17;
pub const TYPE_SURFACE: u8 = 18;
pub const TYPE_SOLID: u8 = 19;
pub const TYPE_BSPLINE_POLE: u8 = 21;
pub const TYPE_POINT_STRING: u8 = 22;
pub const TYPE_CONE: u8 = 23;
pub const TYPE_BSPLINE_SURFACE: u8 = 24;
pub const TYPE_BSPLINE_BOUNDARY: u8 = 25;
pub const TYPE_BSPLINE_KNOT: u8 = 26;
pub const TYPE_BSPLINE_CURVE: u8 = 27;
pub const TYPE_BSPLINE_WEIGHTS: u8 = 28;
pub const TYPE_SHARED_CELL_DEFINITION: u8 = 34;
pub const TYPE_SHARED_CELL: u8 = 35;
pub const TYPE_TAG_VALUE: u8 = 37;
pub const TYPE_APPLICATION: u8 = 66;

/// A group data element (type 5) on this level is the colour table, and on
/// this one a reference file attachment (dgnlib.h:677-679).
pub const LEVEL_COLOUR_TABLE: u8 = 1;
pub const LEVEL_REFERENCE_FILE: u8 = 9;

/// The property bits this module reads or writes (dgnlib.h:684-692): a shape
/// that is a hole in the area of its cell, and attribute data after the
/// element's own.
pub const PROPERTY_HOLE: u16 = 0x8000;
pub const PROPERTY_ATTRIBUTES: u16 = 0x0800;

/// Text justification: the left end of the bottom line (dgnlib.h:705), what
/// GDAL writes and what the origin of every text element is.
pub const JUSTIFY_LEFT_BOTTOM: u8 = 2;
/// MicroStation's font 1 ("WORKING"), which GDAL's writer gives text when
/// nothing else is known (ogrdgnlayer.cpp:1101, "1 is the default font for
/// DGN. Not 0").
pub const DEFAULT_FONT: u8 = 1;

// ---- the design header (type 9, dgnread.cpp:1488-1585 DGNParseTCB) -----------------------

/// The design header is the file's first element, 768 words long: its words
/// to follow are 766 (dgnopen.cpp:41, the test DGNTestOpen makes).
pub const HEADER_BYTES: usize = 1536;
pub const HEADER_WORDS_TO_FOLLOW: u16 = 766;
/// Sub-units per master unit, a 32-bit integer (dgnread.cpp:1502).
pub const HEADER_SUBUNITS_PER_MASTER: usize = 1112;
/// Units of resolution - the integer step of a coordinate - per sub-unit
/// (dgnread.cpp:1509).
pub const HEADER_UNITS_PER_SUBUNIT: usize = 1116;
/// The two characters naming the master and the sub-unit (dgnread.cpp:1505).
pub const HEADER_MASTER_NAME: usize = 1120;
pub const HEADER_SUB_NAME: usize = 1122;
/// The byte whose 0x40 bit makes the design 3D (dgnread.cpp:1497).
pub const HEADER_DIMENSION: usize = 1214;
pub const HEADER_THREE_D: u8 = 0x40;
/// The global origin, three VAX reals in units of resolution
/// (dgnread.cpp:1517): where the design's 0,0,0 is on the integer plane.
pub const HEADER_GLOBAL_ORIGIN: usize = 1240;

/// The word that ends the design (dgnread.cpp:83, dgnwrite.cpp:243).
pub const END_OF_DESIGN: [u8; 2] = [0xFF, 0xFF];

/// The most vertices a line string or shape holds (dgnwrite.cpp:844
/// DGNCreateMultiPointElem refuses more); a longer one is a complex chain or
/// shape of line strings.
pub const MAXIMUM_VERTICES: usize = 101;
/// The most bytes of text one text element holds: its count is one byte
/// (dgnread.cpp:736).
pub const MAXIMUM_TEXT_BYTES: usize = 255;
/// An angle is a 32-bit integer of 1/360000 degree (dgnread.cpp:673).
pub const ANGLE_STEPS_PER_DEGREE: f64 = 360_000.0;
/// A text's height and width are integers of 6/1000 of a unit of resolution
/// (dgnread.cpp:749: `value * scale * 6.0 / 1000.0`).
pub const TEXT_SIZE_STEP: f64 = 6.0 / 1000.0;
/// The most words a complex group says it spans: its total length is one
/// 16-bit word (dgnread.cpp:820).
pub const MAXIMUM_GROUP_WORDS: usize = 65_535;

/// The signatures a file begins with: a 2D and a 3D design file (the design
/// header's first word, dgnopen.cpp:40-44), a cell library (dgnopen.cpp:35)
/// and an OLE compound document, the container a V8 design file is
/// (Microsoft's [MS-CFB] 2.2, the header signature).
pub const SIGNATURE_2D: [u8; 4] = [0x08, 0x09, 0xFE, 0x02];
pub const SIGNATURE_3D: [u8; 4] = [0xC8, 0x09, 0xFE, 0x02];
pub const SIGNATURE_CELL_LIBRARY: [u8; 4] = [0x08, 0x05, 0x17, 0x00];
pub const SIGNATURE_COMPOUND_DOCUMENT: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// What a person calls an element kind: the tally's words.
pub fn element_name(kind: u8) -> String {
    let name = match kind {
        TYPE_CELL_LIBRARY => "Cell Library",
        TYPE_CELL_HEADER => "Cell",
        TYPE_LINE => "Line",
        TYPE_LINE_STRING => "Line String",
        TYPE_GROUP_DATA => "Group Data",
        TYPE_SHAPE => "Shape",
        TYPE_TEXT_NODE => "Text Node",
        TYPE_DIGITIZER_SETUP => "Digitizer Setup",
        TYPE_DESIGN_HEADER => "Design Header",
        TYPE_LEVEL_SYMBOLOGY => "Level Symbology",
        TYPE_CURVE => "Curve",
        TYPE_COMPLEX_CHAIN => "Complex Chain",
        TYPE_COMPLEX_SHAPE => "Complex Shape",
        TYPE_ELLIPSE => "Ellipse",
        TYPE_ARC => "Arc",
        TYPE_TEXT => "Text",
        TYPE_SURFACE => "3D Surface",
        TYPE_SOLID => "3D Solid",
        TYPE_BSPLINE_POLE => "B-spline Poles",
        TYPE_POINT_STRING => "Point String",
        TYPE_CONE => "Cone",
        TYPE_BSPLINE_SURFACE => "B-spline Surface",
        TYPE_BSPLINE_BOUNDARY => "B-spline Boundary",
        TYPE_BSPLINE_KNOT => "B-spline Knots",
        TYPE_BSPLINE_CURVE => "B-spline Curve",
        TYPE_BSPLINE_WEIGHTS => "B-spline Weights",
        TYPE_SHARED_CELL_DEFINITION => "Shared Cell Definition",
        TYPE_SHARED_CELL => "Shared Cell",
        TYPE_TAG_VALUE => "Tag Value",
        TYPE_APPLICATION => "Application",
        _ => return format!("Type {kind}"),
    };
    String::from(name)
}

// ---- numbers -----------------------------------------------------------------------------

/// The little-endian 16-bit word at `at`.
pub fn word(bytes: &[u8], at: usize) -> Option<u16> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]))
}

pub fn put_word(value: u16, out: &mut [u8], at: usize) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

/// The 32-bit integer at `at`: its high word first, each word little-endian
/// (dgnlibp.h:70, `p[2] + (p[3] << 8) + (p[1] << 24) + (p[0] << 16)`).
pub fn int32(bytes: &[u8], at: usize) -> Option<i32> {
    let four = bytes.get(at..at.checked_add(4)?)?;
    Some(i32::from_le_bytes([four[2], four[3], four[0], four[1]]))
}

/// `value` as four bytes, the inverse of int32 (dgnlibp.h:71-82,
/// DGN_WRITE_INT32).
pub fn int32_bytes(value: i32) -> [u8; 4] {
    let little = value.to_le_bytes();
    [little[2], little[3], little[0], little[1]]
}

pub fn put_int32(value: i32, out: &mut [u8], at: usize) {
    out[at..at + 4].copy_from_slice(&int32_bytes(value));
}

/// 2 to the `exponent`, exactly, for an exponent a normal double has: its
/// bits are the biased exponent alone.
fn power_of_two(exponent: i32) -> f64 {
    f64::from_bits(((exponent + 1023) as u64) << 52)
}

/// The VAX D-float at `at`; None when the bytes are not there, or are the
/// reserved operand (sign set, exponent 0), which no number is.
///
/// A D-float is (-1)^s x 0.1f x 2^(e - 128) with a 55-bit f: the mantissa
/// 2^55 + f times 2^(e - 184). The mantissa has 56 bits and a double 53, so
/// it is rounded to the nearest (Rust's integer to float conversion); GDAL's
/// CPLVaxToIEEEDouble keeps a sticky bit instead (cpl_vax.cpp), which can
/// differ in the last place. Every real this module writes has the low three
/// bits clear, and reads back exactly either way.
pub fn vax_double(bytes: &[u8], at: usize) -> Option<f64> {
    let high = int32(bytes, at)? as u32;
    let low = int32(bytes, at.checked_add(4)?)? as u32;
    let negative = high >> 31 == 1;
    let exponent = (high >> 23) & 0xFF;
    if exponent == 0 {
        // Any fraction with exponent 0 and the sign clear is VAX's zero.
        return if negative { None } else { Some(0.0) };
    }
    let fraction = (u64::from(high & 0x7F_FFFF) << 32) | u64::from(low);
    let mantissa = (1_u64 << 55) | fraction;
    // exponent is 1..=255, so the power is 2^-183 ..= 2^71: a normal double,
    // and the product is exact.
    let magnitude = mantissa as f64 * power_of_two(exponent as i32 - 184);
    Some(if negative { -magnitude } else { magnitude })
}

/// `value` as a VAX D-float: exact for every double the format can hold
/// (cpl_vax.cpp CPLIEEEToVaxDouble shifts the 52-bit fraction three places
/// into the 55). A value below 2^-128 in size is written as 0, as GDAL
/// writes it; None for one too large (2^127 and above) and for one that is
/// not finite, which the format has no form for.
pub fn vax_bytes(value: f64) -> Option<[u8; 8]> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some([0; 8]);
    }
    let bits = value.to_bits();
    let sign = (bits >> 63) as u32;
    let biased = ((bits >> 52) & 0x7FF) as i32;
    // A subnormal double (biased 0) is far below the least D-float.
    let exponent = biased - 1023 + 129;
    if biased == 0 || exponent < 1 {
        return Some([0; 8]);
    }
    if exponent > 255 {
        return None;
    }
    let fraction = bits & ((1_u64 << 52) - 1);
    let high = (sign << 31) | ((exponent as u32) << 23) | ((fraction >> 29) as u32);
    let low = ((fraction & ((1_u64 << 29) - 1)) << 3) as u32;
    let mut out = [0_u8; 8];
    put_int32(high as i32, &mut out, 0);
    put_int32(low as i32, &mut out, 4);
    Some(out)
}

/// A 32-bit range corner (dgnwrite.cpp:795-802): the integer with its sign
/// bit flipped, so the lowest coordinate of the plane is 0.
pub fn range_bytes(value: i32) -> [u8; 4] {
    int32_bytes(value ^ i32::MIN)
}

/// Three characters of a cell's name: RADIX-50, each 0-39 a digit of a
/// base-40 number (dgnhelp.cpp:212-240 DGNRad50ToAscii): blank, A-Z, '$',
/// '.', an unused code read as a blank, and 0-9. A word above 63 999 has a
/// first digit past 39, which is no character; it is read as a blank.
pub fn radix50_text(value: u16) -> String {
    let mut text = String::new();
    let mut rest = value;
    for divisor in [1600_u16, 40, 1] {
        let digit = rest / divisor;
        rest -= digit * divisor;
        text.push(match digit {
            1..=26 => char::from(b'A' + (digit as u8 - 1)),
            27 => '$',
            28 => '.',
            30..=39 => char::from(b'0' + (digit as u8 - 30)),
            _ => ' ',
        });
    }
    text
}

// ---- colours -----------------------------------------------------------------------------

/// The colours an element's index names when the file has no colour table
/// of its own: dgnlib's default palette (dgnhelp.cpp:17-103,
/// `abyDefaultPCT`), copied. 0-7 are MicroStation's white, blue, green, red,
/// yellow, magenta, orange and cyan; 255 is the background.
pub const DEFAULT_COLOURS: [[u8; 3]; 256] = [
    [255, 255, 255], [0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 0], [255, 0, 255],
    [255, 127, 0], [0, 255, 255], [64, 64, 64], [192, 192, 192], [254, 0, 96], [160, 224, 0],
    [0, 254, 160], [128, 0, 160], [176, 176, 176], [0, 240, 240], [240, 240, 240], [0, 0, 240],
    [0, 240, 0], [240, 0, 0], [240, 240, 0], [240, 0, 240], [240, 122, 0], [0, 240, 240],
    [240, 240, 240], [0, 0, 240], [0, 240, 0], [240, 0, 0], [240, 240, 0], [240, 0, 240],
    [240, 122, 0], [0, 225, 225], [225, 225, 225], [0, 0, 225], [0, 225, 0], [225, 0, 0],
    [225, 225, 0], [225, 0, 225], [225, 117, 0], [0, 225, 225], [225, 225, 225], [0, 0, 225],
    [0, 225, 0], [225, 0, 0], [225, 225, 0], [225, 0, 225], [225, 117, 0], [0, 210, 210],
    [210, 210, 210], [0, 0, 210], [0, 210, 0], [210, 0, 0], [210, 210, 0], [210, 0, 210],
    [210, 112, 0], [0, 210, 210], [210, 210, 210], [0, 0, 210], [0, 210, 0], [210, 0, 0],
    [210, 210, 0], [210, 0, 210], [210, 112, 0], [0, 195, 195], [195, 195, 195], [0, 0, 195],
    [0, 195, 0], [195, 0, 0], [195, 195, 0], [195, 0, 195], [195, 107, 0], [0, 195, 195],
    [195, 195, 195], [0, 0, 195], [0, 195, 0], [195, 0, 0], [195, 195, 0], [195, 0, 195],
    [195, 107, 0], [0, 180, 180], [180, 180, 180], [0, 0, 180], [0, 180, 0], [180, 0, 0],
    [180, 180, 0], [180, 0, 180], [180, 102, 0], [0, 180, 180], [180, 180, 180], [0, 0, 180],
    [0, 180, 0], [180, 0, 0], [180, 180, 0], [180, 0, 180], [180, 102, 0], [0, 165, 165],
    [165, 165, 165], [0, 0, 165], [0, 165, 0], [165, 0, 0], [165, 165, 0], [165, 0, 165],
    [165, 97, 0], [0, 165, 165], [165, 165, 165], [0, 0, 165], [0, 165, 0], [165, 0, 0],
    [165, 165, 0], [165, 0, 165], [165, 97, 0], [0, 150, 150], [150, 150, 150], [0, 0, 150],
    [0, 150, 0], [150, 0, 0], [150, 150, 0], [150, 0, 150], [150, 92, 0], [0, 150, 150],
    [150, 150, 150], [0, 0, 150], [0, 150, 0], [150, 0, 0], [150, 150, 0], [150, 0, 150],
    [150, 92, 0], [0, 135, 135], [135, 135, 135], [0, 0, 135], [0, 135, 0], [135, 0, 0],
    [135, 135, 0], [135, 0, 135], [135, 87, 0], [0, 135, 135], [135, 135, 135], [0, 0, 135],
    [0, 135, 0], [135, 0, 0], [135, 135, 0], [135, 0, 135], [135, 87, 0], [0, 120, 120],
    [120, 120, 120], [0, 0, 120], [0, 120, 0], [120, 0, 0], [120, 120, 0], [120, 0, 120],
    [120, 82, 0], [0, 120, 120], [120, 120, 120], [0, 0, 120], [0, 120, 0], [120, 0, 0],
    [120, 120, 0], [120, 0, 120], [120, 82, 0], [0, 105, 105], [105, 105, 105], [0, 0, 105],
    [0, 105, 0], [105, 0, 0], [105, 105, 0], [105, 0, 105], [105, 77, 0], [0, 105, 105],
    [105, 105, 105], [0, 0, 105], [0, 105, 0], [105, 0, 0], [105, 105, 0], [105, 0, 105],
    [105, 77, 0], [0, 90, 90], [90, 90, 90], [0, 0, 90], [0, 90, 0], [90, 0, 0],
    [90, 90, 0], [90, 0, 90], [90, 72, 0], [0, 90, 90], [90, 90, 90], [0, 0, 90],
    [0, 90, 0], [90, 0, 0], [90, 90, 0], [90, 0, 90], [90, 72, 0], [0, 75, 75],
    [75, 75, 75], [0, 0, 75], [0, 75, 0], [75, 0, 0], [75, 75, 0], [75, 0, 75],
    [75, 67, 0], [0, 75, 75], [75, 75, 75], [0, 0, 75], [0, 75, 0], [75, 0, 0],
    [75, 75, 0], [75, 0, 75], [75, 67, 0], [0, 60, 60], [60, 60, 60], [0, 0, 60],
    [0, 60, 0], [60, 0, 0], [60, 60, 0], [60, 0, 60], [60, 62, 0], [0, 60, 60],
    [60, 60, 60], [0, 0, 60], [0, 60, 0], [60, 0, 0], [60, 60, 0], [60, 0, 60],
    [60, 62, 0], [0, 45, 45], [45, 45, 45], [0, 0, 45], [0, 45, 0], [45, 0, 0],
    [45, 45, 0], [45, 0, 45], [45, 57, 0], [0, 45, 45], [45, 45, 45], [0, 0, 45],
    [0, 45, 0], [45, 0, 0], [45, 45, 0], [45, 0, 45], [45, 57, 0], [0, 30, 30],
    [30, 30, 30], [0, 0, 30], [0, 30, 0], [30, 0, 0], [30, 30, 0], [30, 0, 30],
    [30, 52, 0], [0, 30, 30], [30, 30, 30], [0, 0, 30], [0, 30, 0], [30, 0, 0],
    [30, 30, 0], [30, 0, 30], [192, 192, 192], [28, 0, 100],
];

/// The index of the background in a colour table: drawn in nothing.
pub const BACKGROUND_INDEX: usize = 255;

/// The colour `index` names in `table`, opaque.
pub fn colour_at(table: &[[u8; 3]; 256], index: u8) -> Color {
    let [r, g, b] = table[usize::from(index)];
    Color::new(r, g, b, 255)
}

/// The drawing colour of `table` (0-254, never the background) nearest
/// `colour` by the sum of the squared channel differences; the first of
/// equals.
pub fn nearest_colour(table: &[[u8; 3]; 256], colour: Color) -> u8 {
    let mut best = 0_usize;
    let mut best_distance = u32::MAX;
    for (index, entry) in table.iter().enumerate().take(BACKGROUND_INDEX) {
        let distance = [
            (entry[0], colour.r),
            (entry[1], colour.g),
            (entry[2], colour.b),
        ]
        .iter()
        .map(|&(a, b)| u32::from(a.abs_diff(b)).pow(2))
        .sum::<u32>();
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }
    best as u8
}
