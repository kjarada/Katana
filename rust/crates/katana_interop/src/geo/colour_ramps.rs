//! The colour ramps terrain shading draws with (docs/terrain.md, "Shading"):
//! the built-in ones, a person's own GDAL colour-map file, and a ramp spread
//! over a range of values as the colour-map text GDAL's `raster color-map`
//! reads - so the colours a legend lists are, value for value, the ones the
//! picture was painted with. The port of
//! include/katana/interop/geo/colour_ramps.hpp and
//! src/katana_interop/geo/colour_ramps.cpp.
//!
//! The built-in ramps are the one table a surface's display style names too
//! (katana_terrain::colour_ramps, docs/surface_display.md "The ramps"): this
//! module only gives each of its rows the place of every stop along the
//! range, which is what GDAL's colour map reads.
//!
//! ```text
//! terrain     deep blue, green, sand, brown, snow: relief
//! earth       pale yellow to dark brown
//! diverging   blue, white in the middle, red: a difference, cut and fill
//!             (on design minus existing, cut is blue and fill red)
//! slope       green, yellow, red: gentle to steep
//! grey        dark to light in equal steps: a band drawn as it is ("plain")
//! spectral    blue through green and yellow to red
//! ```

use std::cmp::Ordering;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::text::{
    format_exact_real, is_ascii_space, parse_finite_double, parse_integer, split_lines,
};
use katana_core::text_encoding::{decode_text, utf8_or_windows1252};
use katana_core::{ErrorCode, Result};
use katana_math::cpp;
use katana_terrain::colour_ramps::{colour_ramp_names, colour_ramps, find_colour_ramp};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RampStop {
    /// 0 at the range's start, 1 at its end
    pub position: f64,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColourRamp {
    pub name: String,
    /// positions ascending, the first 0 and the last 1
    pub stops: Vec<RampStop>,
}

/// One colour at one value: what a legend lists.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LegendEntry {
    pub value: f64,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

// The ramps are the terrain layer's one table - the one a surface's display
// style names - as GDAL's colour map wants them: each stop with its place
// along the range. Built from it once, so a stop here is the style's stop
// (docs/surface_display.md, "The ramps").
fn ramps() -> &'static [ColourRamp] {
    static ALL: OnceLock<Vec<ColourRamp>> = OnceLock::new();
    ALL.get_or_init(|| {
        colour_ramps()
            .iter()
            .map(|named| {
                let last = named.stops.len() - 1;
                ColourRamp {
                    name: String::from(named.name),
                    stops: named
                        .stops
                        .iter()
                        .enumerate()
                        .map(|(i, stop)| RampStop {
                            // Evenly spaced: i / 4 is exact in binary (0, .25,
                            // .5, .75, 1).
                            position: i as f64 / last as f64,
                            r: stop.r,
                            g: stop.g,
                            b: stop.b,
                        })
                        .collect(),
                }
            })
            .collect()
    })
}

// static_cast<std::uint8_t>(std::clamp(value, 0.0, 255.0)).
fn channel(value: f64) -> u8 {
    cpp::clamp(value, 0.0, 255.0) as u8
}

// The words of a colour-map line: blanks, tabs and commas separate.
fn words_of(line: &[u8]) -> Vec<Vec<u8>> {
    line.split(|&byte| is_ascii_space(byte) || byte == b',')
        .filter(|word| !word.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

// path::generic_string(): the path with '/' on every platform.
fn generic_text(path: &Path) -> String {
    let text = utf8_or_windows1252(path_to_utf8(path));
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// The built-in ramp of that name (any case); None for none. Every ramp has
/// the table's five stops, at 0, 1/4, 1/2, 3/4 and 1.
pub fn built_in_ramp(name: &str) -> Option<&'static ColourRamp> {
    // The one lookup: the table finds the ramp (any case), this finds its row.
    let found = find_colour_ramp(name)?;
    let index = colour_ramps()
        .iter()
        .position(|ramp| std::ptr::eq(ramp, found))?;
    ramps().get(index)
}

/// Their names, in the table's order.
pub fn built_in_ramp_names() -> Vec<String> {
    colour_ramp_names()
        .iter()
        .map(|&name| String::from(name))
        .collect()
}

/// The ramp's stops at their values over [low, high].
pub fn spread(ramp: &ColourRamp, low: f64, high: f64) -> Vec<LegendEntry> {
    ramp.stops
        .iter()
        .map(|stop| {
            // The ends exactly: low + 1 * (high - low) need not be high.
            let value = if stop.position <= 0.0 {
                low
            } else if stop.position >= 1.0 {
                high
            } else {
                low + stop.position * (high - low)
            };
            LegendEntry {
                value,
                r: stop.r,
                g: stop.g,
                b: stop.b,
            }
        })
        .collect()
}

/// The entries as GDAL's colour-map text, a line per entry - "value r g b
/// 255", each value written exactly, so a cell holding the range's end takes
/// the ramp's end colour exactly - then, when `no_data`, "nv 0 0 0 0": no-data
/// transparent. GDAL interpolates between the lines.
pub fn colour_map_text(entries: &[LegendEntry], no_data: bool) -> String {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&format!(
            "{} {} {} {} 255\n",
            format_exact_real(entry.value),
            entry.r,
            entry.g,
            entry.b
        ));
    }
    if no_data {
        text.push_str("nv 0 0 0 0\n");
    }
    text
}

/// A person's colour-map file as legend entries: the lines GDAL's colour-map
/// reads, "value r g b [a]" (blanks, tabs or commas between), a value given
/// as a percentage placed on [low, high] as GDAL places it; "nv" lines and
/// named colours are skipped (a legend lists values). NotFound for a file that
/// is not there; InvalidArgument for one with no value line.
pub fn read_colour_map(file: &Path, low: f64, high: f64) -> Result<Vec<LegendEntry>> {
    let Ok(mut stream) = File::open(file) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no colour-map file there",
            generic_text(file),
        ));
    };
    // What the C++'s istreambuf_iterator reads: up to the first failure (a
    // folder opens and reads nothing), the bytes read before it kept.
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    let decoded = decode_text(bytes)?;
    let mut entries = Vec::new();
    for line in split_lines(decoded.text.as_bytes()) {
        let words = words_of(line);
        if words.len() < 4 {
            continue;
        }
        // A value given as a percentage, placed on [low, high]; GDAL's colour
        // map places it so.
        let value = match words[0].strip_suffix(b"%") {
            Some(percent) => {
                parse_finite_double(percent).map(|percent| low + percent / 100.0 * (high - low))
            }
            None => parse_finite_double(&words[0]),
        };
        let r = parse_integer(&words[1]);
        let g = parse_integer(&words[2]);
        let b = parse_integer(&words[3]);
        let (Some(value), Some(r), Some(g), Some(b)) = (value, r, g, b) else {
            continue; // nv, a named colour, a comment
        };
        entries.push(LegendEntry {
            value,
            r: channel(r as f64),
            g: channel(g as f64),
            b: channel(b as f64),
        });
    }
    if entries.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the colour-map file has no line of a value and its colour",
            generic_text(file),
        ));
    }
    // Stable, by `a.value < b.value`, as the C++'s std::ranges::stable_sort.
    entries.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(Ordering::Equal));
    Ok(entries)
}
