//! The small encodings DXF uses for colour, lineweight and text, in one place
//! so that the reader and the writer cannot disagree about them - and exposed
//! so that tests check them against values worked out by hand.

use std::sync::LazyLock;

use katana_entity::Color;
use katana_math::numerics::PI;

// ---- colour ----------------------------------------------------------------

/// Group 62 values that are not colours.
pub const COLOUR_BY_BLOCK: i32 = 0;
pub const COLOUR_BY_LAYER: i32 = 256;
/// Index 7 is the drawing's foreground: white on a dark background, black on a
/// light one. Katana draws it white.
pub const COLOUR_FOREGROUND: i32 = 7;

fn rgb(r: i32, g: i32, b: i32) -> Color {
    Color::new(r as u8, g as u8, b as u8, 255)
}

/// Indexed colours 10-249: hue (index - 10) / 10 in steps of 15 degrees, then
/// within the hue five brightnesses, each full and then a third saturated.
fn computed_hue(index: i32) -> Color {
    const VALUES: [f64; 5] = [255.0, 189.0, 129.0, 104.0, 79.0];
    let offset = index - 10;
    let hue = offset / 10;
    let step = offset % 10;
    let value = VALUES[(step / 2) as usize];
    let saturation = if step % 2 == 0 { 1.0 } else { 1.0 / 3.0 };
    // The hue's pure components, 0 to 1, by the usual six sectors of 60
    // degrees; 15 degrees is a quarter of a sector, so f is exact.
    let sector = hue / 4;
    let f = f64::from(hue % 4) / 4.0;
    let (r, g, b) = match sector {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    // std::lround: halves away from zero, as f64::round.
    let channel = |pure: f64| (value * (1.0 - saturation * (1.0 - pure))).round() as i32;
    rgb(channel(r), channel(g), channel(b))
}

static TABLE: LazyLock<[Color; 256]> = LazyLock::new(|| {
    const NAMED: [[i32; 3]; 10] = [
        [0, 0, 0],
        [255, 0, 0],
        [255, 255, 0],
        [0, 255, 0],
        [0, 255, 255],
        [0, 0, 255],
        [255, 0, 255],
        [255, 255, 255],
        [128, 128, 128],
        [192, 192, 192],
    ];
    const GREYS: [i32; 6] = [51, 80, 105, 130, 190, 255];
    let mut out = [Color::default(); 256];
    for (i, colour) in out.iter_mut().enumerate().skip(1) {
        let index = i as i32;
        *colour = if index <= 9 {
            let named = NAMED[i];
            rgb(named[0], named[1], named[2])
        } else if index >= 250 {
            let grey = GREYS[i - 250];
            rgb(grey, grey, grey)
        } else {
            computed_hue(index)
        };
    }
    out
});

/// The RGB of indexed colour 1-255. None for 0 (ByBlock), 256 (ByLayer) and
/// anything outside the range.
///
/// 1-9 are the named colours (red, yellow, green, cyan, blue, magenta, white,
/// grey 128, grey 192), 250-255 a grey ramp, and 10-249 twenty-four hues 15
/// degrees apart, each at five brightnesses (255, 189, 129, 104, 79), full and
/// then a third saturated. The hues are computed rather than tabulated: the
/// published table rounds some of them a unit or two differently, which no
/// screen shows and which cannot move a colour to a different index.
pub fn indexed_colour(index: i32) -> Option<Color> {
    if !(1..=255).contains(&index) {
        return None;
    }
    Some(TABLE[index as usize])
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
    for (i, candidate) in TABLE.iter().enumerate().skip(1) {
        let dr = i64::from(candidate.r) - i64::from(colour.r);
        let dg = i64::from(candidate.g) - i64::from(colour.g);
        let db = i64::from(candidate.b) - i64::from(colour.b);
        let distance = dr * dr + dg * dg + db * db;
        if distance < best_distance {
            // strict: the lower index wins a tie
            best_distance = distance;
            best = i as i32;
            if distance == 0 {
                break;
            }
        }
    }
    best
}

/// Group 420: 0x00RRGGBB.
pub fn true_colour(value: i64) -> Color {
    rgb(
        ((value >> 16) & 0xFF) as i32,
        ((value >> 8) & 0xFF) as i32,
        (value & 0xFF) as i32,
    )
}

// ---- angles ------------------------------------------------------------------

/// A DXF angle is in degrees; the reader turns one into radians by this one
/// multiplication, and the writer's exact_degrees is chosen against it.
pub const RADIANS_PER_DEGREE: f64 = PI / 180.0;

/// Degrees that the reader turns back into exactly `radians`. The nearest
/// double to radians * 180 / pi does not always survive the multiplication
/// back; a neighbour a unit or two in the last place away usually does, and is
/// then the one written, so that a text's rotation goes out and comes back
/// the same double. Not always: some radians no degrees reach, since below a
/// power of two the products are spaced more widely than the doubles. The
/// nearest then, which is never further off than the answer given.
pub fn exact_degrees(radians: f64) -> f64 {
    const DEGREES_PER_RADIAN: f64 = 180.0 / PI;
    let nearest = radians * DEGREES_PER_RADIAN;
    if !nearest.is_finite() || nearest * RADIANS_PER_DEGREE == radians {
        return nearest;
    }
    let mut above = nearest;
    let mut below = nearest;
    for _ in 0..8 {
        above = above.next_up();
        if above * RADIANS_PER_DEGREE == radians {
            return above;
        }
        below = below.next_down();
        if below * RADIANS_PER_DEGREE == radians {
            return below;
        }
    }
    nearest
}

// ---- lineweight --------------------------------------------------------------

/// A layer's or entity's group 370 is one of a fixed set of hundredths of a
/// millimetre (0, 5, 9, 13 ... 211), or -1 ByLayer, -2 ByBlock, -3 default.
/// The nearest of the set to `millimetres`.
pub fn nearest_lineweight(millimetres: f64) -> i32 {
    const WEIGHTS: [i32; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    if !millimetres.is_finite() || millimetres <= 0.0 {
        return 0;
    }
    let hundredths = millimetres * 100.0;
    let mut best = WEIGHTS[0];
    let mut best_distance = f64::INFINITY;
    for weight in WEIGHTS {
        let distance = (hundredths - f64::from(weight)).abs();
        if distance < best_distance {
            best_distance = distance;
            best = weight;
        }
    }
    best
}

// ---- text --------------------------------------------------------------------

/// The C++'s appendUtf8: a surrogate half is not a character.
fn append_utf8(out: &mut Vec<u8>, code_point: u32) {
    let cp = if (0xD800..=0xDFFF).contains(&code_point) {
        0xFFFD
    } else {
        code_point
    };
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push((0xC0 | (cp >> 6)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else if cp < 0x10000 {
        out.push((0xE0 | (cp >> 12)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    } else {
        out.push((0xF0 | (cp >> 18)) as u8);
        out.push((0x80 | ((cp >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
    }
}

fn hex_digit(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => i32::from(c - b'0'),
        b'A'..=b'F' => i32::from(c - b'A') + 10,
        b'a'..=b'f' => i32::from(c - b'a') + 10,
        _ => -1,
    }
}

/// \U+XXXX at `i` (the backslash): the bytes consumed, 0 when it is not one,
/// and the character appended.
fn unicode_escape(raw: &[u8], i: usize, out: &mut Vec<u8>) -> usize {
    if i + 7 > raw.len() {
        return 0;
    }
    if (raw[i + 1] != b'U' && raw[i + 1] != b'u') || raw[i + 2] != b'+' {
        return 0;
    }
    let mut cp: u32 = 0;
    for &c in &raw[i + 3..i + 7] {
        let digit = hex_digit(c);
        if digit < 0 {
            return 0;
        }
        cp = cp * 16 + digit as u32;
    }
    append_utf8(out, cp);
    7
}

/// %%x at `i`: the bytes consumed, 0 when it is not a code.
fn percent_code(raw: &[u8], i: usize, out: &mut Vec<u8>) -> usize {
    if i + 2 >= raw.len() || raw[i] != b'%' || raw[i + 1] != b'%' {
        return 0;
    }
    let code = raw[i + 2];
    match code {
        b'd' | b'D' => {
            append_utf8(out, 0x00B0);
            return 3;
        }
        b'p' | b'P' => {
            append_utf8(out, 0x00B1);
            return 3;
        }
        b'c' | b'C' => {
            append_utf8(out, 0x2205);
            return 3;
        }
        b'%' => {
            out.push(b'%');
            return 3;
        }
        // A toggle Katana text has no way to show.
        b'u' | b'U' | b'o' | b'O' | b'k' | b'K' => return 3,
        _ => {}
    }
    if code.is_ascii_digit() {
        let mut end = i + 2;
        let mut value: u32 = 0;
        while end < raw.len() && end < i + 5 && raw[end].is_ascii_digit() {
            value = value * 10 + u32::from(raw[end] - b'0');
            end += 1;
        }
        if (1..256).contains(&value) {
            append_utf8(out, value);
        }
        return end - i;
    }
    0
}

/// Valid UTF-8 in is valid UTF-8 out of plain_text: every code it reads is
/// ASCII and is replaced by whole characters, and every other byte is copied.
/// The lossy branch is never taken; it is there so that no input can panic.
fn into_text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

/// A TEXT string as it reads: %%d, %%p and %%c are the degree, plus-minus and
/// diameter signs, %%% a percent sign, %%nnn the character with that code,
/// %%u, %%o and %%k (underline, overline, strike-through toggles) nothing, and
/// \U+XXXX the character it names.
pub fn plain_text(raw: &str) -> String {
    let raw = raw.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i];
        if c == b'%' {
            let used = percent_code(raw, i, &mut out);
            if used != 0 {
                i += used;
                continue;
            }
        } else if c == b'\\' {
            let used = unicode_escape(raw, i, &mut out);
            if used != 0 {
                i += used;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    into_text(out)
}

/// An MTEXT string as it reads. \P (and \N, a column break) end a paragraph and
/// become a line break; \~ is a no-break space; \\ \{ \} are the characters;
/// \S1/2; (a stacked fraction) is "1/2", with ^ and # read as /; the font,
/// height, colour, width, tracking, slant, alignment and paragraph codes
/// (\f \F \H \C \c \W \T \Q \A \p ... up to their ';') and the toggles
/// (\L \l \O \o \K \k) are dropped; a brace that is not escaped groups and is
/// dropped. The TEXT codes above apply too.
///
/// Bytes, as the C++'s std::string is: \M+nXXXX drops eight bytes whatever
/// they are, so a malformed one can cut a character, and the reader then
/// refuses that line as the model refuses a text that is not UTF-8. Every
/// other code leaves UTF-8 as it found it.
pub fn plain_mtext(raw: &str) -> Vec<u8> {
    let raw = raw.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    // Skips past the ';' that ends a code's argument; to the end if none does.
    let past_semicolon = |from: usize| -> usize {
        raw.get(from..)
            .and_then(|rest| rest.iter().position(|&c| c == b';'))
            .map_or(raw.len(), |at| from + at + 1)
    };
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i];
        if c == b'{' || c == b'}' {
            i += 1;
            continue;
        }
        if c == b'%' {
            let used = percent_code(raw, i, &mut out);
            if used != 0 {
                i += used;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == b'^' && i + 1 < raw.len() {
            let next = raw[i + 1];
            if next == b'J' {
                out.push(b'\n');
                i += 2;
                continue;
            }
            if next == b'I' {
                out.push(b' ');
                i += 2;
                continue;
            }
            if next == b'M' {
                i += 2;
                continue;
            }
            if next == b' ' {
                out.push(b'^');
                i += 2;
                continue;
            }
        }
        if c != b'\\' || i + 1 >= raw.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let code = raw[i + 1];
        match code {
            b'P' | b'N' => {
                out.push(b'\n');
                i += 2;
            }
            b'~' => {
                append_utf8(&mut out, 0x00A0);
                i += 2;
            }
            b'\\' | b'{' | b'}' => {
                out.push(code);
                i += 2;
            }
            b'L' | b'l' | b'O' | b'o' | b'K' | b'k' | b'X' => {
                i += 2;
            }
            b'S' => {
                let end = past_semicolon(i + 2);
                let stop = if end > i + 2 && end <= raw.len() && raw[end - 1] == b';' {
                    end - 1
                } else {
                    end
                };
                let mut k = i + 2;
                while k < stop {
                    let s = raw[k];
                    if s == b'^' || s == b'#' {
                        out.push(b'/');
                    } else if s == b'\\' && k + 1 < stop {
                        k += 1;
                        out.push(raw[k]);
                    } else {
                        out.push(s);
                    }
                    k += 1;
                }
                i = end;
            }
            b'U' | b'u' => {
                let used = unicode_escape(raw, i, &mut out);
                i += if used != 0 { used } else { 2 };
            }
            b'M' | b'm' => {
                // \M+nXXXX: a double-byte character in a named code page. Rare,
                // and not decodable without the page; dropped rather than shown
                // as its code.
                i = raw.len().min(i + 8);
            }
            b'f' | b'F' | b'H' | b'h' | b'C' | b'c' | b'W' | b'w' | b'T' | b't' | b'Q' | b'q'
            | b'A' | b'a' | b'p' => {
                i = past_semicolon(i + 2);
            }
            _ => {
                // An escape this does not know: the character without the
                // backslash is the likelier reading than both.
                out.push(code);
                i += 2;
            }
        }
    }
    out
}

/// The inverse of plain_text for a string going into a file: ASCII as it is,
/// the three signs as %%d %%p %%c, a percent sign that another follows as %%%,
/// every other character as \U+XXXX (and one beyond the basic plane as '?',
/// which R2000 has no escape for). Control characters become spaces.
pub fn encode_text(utf8: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let utf8 = utf8.as_bytes();
    let mut out = String::with_capacity(utf8.len() + 8);
    let mut i = 0;
    while i < utf8.len() {
        let byte = utf8[i];
        let (cp, length): (u32, usize) = if byte < 0x80 {
            (u32::from(byte), 1)
        } else if (byte & 0xE0) == 0xC0 && i + 1 < utf8.len() {
            (
                (u32::from(byte & 0x1F) << 6) | u32::from(utf8[i + 1] & 0x3F),
                2,
            )
        } else if (byte & 0xF0) == 0xE0 && i + 2 < utf8.len() {
            (
                (u32::from(byte & 0x0F) << 12)
                    | (u32::from(utf8[i + 1] & 0x3F) << 6)
                    | u32::from(utf8[i + 2] & 0x3F),
                3,
            )
        } else if (byte & 0xF8) == 0xF0 && i + 3 < utf8.len() {
            // Beyond the basic plane: no R2000 escape reaches it.
            (0x10000, 4)
        } else {
            (u32::from(b'?'), 1)
        };
        i += length;
        if cp < 0x20 || cp == 0x7F {
            out.push(' ');
        } else if cp == u32::from(b'%') {
            // A percent sign another follows would start a code on reading.
            let another = i < utf8.len() && utf8[i] == b'%';
            out.push_str(if another { "%%%" } else { "%" });
        } else if cp < 0x80 {
            out.push(char::from(cp as u8));
        } else if cp == 0x00B0 {
            out.push_str("%%d");
        } else if cp == 0x00B1 {
            out.push_str("%%p");
        } else if cp == 0x2205 {
            out.push_str("%%c");
        } else if cp < 0x10000 {
            out.push_str("\\U+");
            out.push(char::from(HEX[((cp >> 12) & 0xF) as usize]));
            out.push(char::from(HEX[((cp >> 8) & 0xF) as usize]));
            out.push(char::from(HEX[((cp >> 4) & 0xF) as usize]));
            out.push(char::from(HEX[(cp & 0xF) as usize]));
        } else {
            out.push('?');
        }
    }
    out
}
