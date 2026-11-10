//! What the DEM verbs share (docs/terrain.md, "DEMs from GDAL";
//! docs/geoprocessing.md, "T6", "T7"), the port of
//! src/katana_app/geo/dem_support.hpp/.cpp: RASTER GRID (grid_verbs.rs) and
//! RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT and DIFFERENCE
//! (dem_verbs.rs). They are curated verbs over the one executor: each reads
//! its own words with these helpers, its sources and targets with the ONE
//! source and target parsers (bindings.rs) and its scope with the ONE scope
//! parser, and builds RunRequests; nothing here reads a line another way.
//!
//! [`positive_option`] here says "<key>= is a positive number", where
//! terrain_verbs' says "<key> is greater than zero", and [`join_records`] is a
//! third joining of records beside terrain_verbs' and gdal_verbs': as the C++
//! has them (docs/rust.md "Not done"). Each call takes the one the C++
//! resolves it to.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_gis::GdalDataset;
use katana_gis::processing::DatasetValue;

use super::bindings::Tokens;
use super::geo_verbs::{Context, system_message};
use super::seams;
use crate::import_records::{path_from_text, path_text};

/// A UTF-8 path as the file system's: import_records' path_from_text, the
/// app's one reading of a line's path.
pub fn path_of_utf8(utf8: &str) -> PathBuf {
    path_from_text(utf8)
}

/// A path as UTF-8 text: import_records' path_text.
pub fn utf8_of_path(path: &Path) -> String {
    path_text(path)
}

/// A curated verb's words, parted: its own key=value options and the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerbWords {
    /// By the key as the verb names it (lower case), the value as typed.
    pub options: BTreeMap<String, String>,
    /// Everything else, in order and with its quoting: the scope, the
    /// sources, NAME, TO, the flags.
    pub rest: Tokens,
}

/// Takes the words from `from` on that are `key=value` with a key of `keys`
/// (any case) as options, wherever they stand, and leaves the rest in order.
/// The keys a verb takes are never a WHERE key (TYPE, LAYER, STYLE, COLOUR,
/// PROP, TEXT), so an option after a WHERE filter is the verb's and not a
/// condition the scope parser would refuse; the scope parser still reads
/// every scope word. InvalidArgument naming it for an option given twice or
/// with no value. A quoted word is never an option.
pub fn split_options(tokens: &Tokens, from: usize, keys: &[&str], verb: &str) -> Result<VerbWords> {
    let mut words = VerbWords::default();
    for (word, &quoted) in tokens.words.iter().zip(&tokens.quoted).skip(from) {
        let option = match word.find('=') {
            Some(equals) if !quoted && equals != 0 => {
                let key = word[..equals].to_ascii_lowercase();
                keys.contains(&key.as_str()).then_some((key, equals))
            }
            _ => None,
        };
        let Some((key, equals)) = option else {
            words.rest.words.push(word.clone());
            words.rest.quoted.push(quoted);
            continue;
        };
        let text = String::from(&word[equals + 1..]);
        if text.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{verb}: {key}= needs a value"),
                word.clone(),
            ));
        }
        match words.options.entry(key) {
            Entry::Occupied(slot) => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{verb} takes {}= once", slot.key()),
                    word.clone(),
                ));
            }
            Entry::Vacant(slot) => {
                slot.insert(text);
            }
        }
    }
    Ok(words)
}

/// A value of an option that must be a positive, finite number:
/// InvalidArgument naming the option otherwise (the C++'s
/// `positiveOption(std::string_view, const std::string&)`).
pub fn positive_option(key: &str, text: &str) -> Result<f64> {
    match parse_finite_double(text) {
        Some(number) if number > 0.0 => Ok(number),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= is a positive number"),
            format!("{key}={text}"),
        )),
    }
}

/// x0,y0,x1,y1 with x0 < x1 and y0 < y1, min before max whichever corners
/// came first: InvalidArgument naming the option otherwise.
pub fn box_option(key: &str, text: &str) -> Result<[f64; 4]> {
    let refused = || {
        make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= is four numbers, x0,y0,x1,y1"),
            format!("{key}={text}"),
        )
    };
    let mut corners = [0.0; 4];
    let mut start = 0;
    for (i, corner) in corners.iter_mut().enumerate() {
        let comma = text[start..].find(',').map(|at| start + at);
        let last = i + 1 == 4;
        if comma.is_none() != last {
            return Err(refused());
        }
        let end = comma.unwrap_or(text.len());
        *corner = parse_finite_double(&text[start..end]).ok_or_else(refused)?;
        start = end + 1;
    }
    // Min before max, whichever corners were typed first, as AREA reads them.
    if corners[0] > corners[2] {
        corners.swap(0, 2);
    }
    if corners[1] > corners[3] {
        corners.swap(1, 3);
    }
    if !(corners[2] > corners[0]) || !(corners[3] > corners[1]) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= encloses no area"),
            format!("{key}={text}"),
        ));
    }
    Ok(corners)
}

/// The records of a reply, one a line, an empty one left out.
pub fn join_records(records: &[String]) -> String {
    let mut text = String::new();
    for record in records {
        if !record.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text += record;
        }
    }
    text
}

/// Band 1 of a raster at full precision, read whole on the thread that asks:
/// a grid handed over in memory as it is, a file opened for this read alone
/// (never a dataset another run holds, plan.risks). Its no-data value when it
/// declares one: absent is not a value.
#[derive(Clone, Debug, PartialEq)]
pub struct BandValues {
    pub width: i32,
    pub height: i32,
    pub geotransform: [f64; 6],
    pub no_data: Option<f64>,
    /// width * height, row major, top row first.
    pub values: Vec<f64>,
}

impl Default for BandValues {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            geotransform: [0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
            no_data: None,
            values: Vec::new(),
        }
    }
}

/// Band 1 of the raster at `file` (the C++'s `readBandOne(const path&)`).
pub fn read_band_one_from_file(file: &Path) -> Result<BandValues> {
    let dataset = GdalDataset::open(file)?;
    let info = dataset.raster_info()?;
    let values = dataset.read_band(1)?;
    Ok(BandValues {
        width: info.width,
        height: info.height,
        geotransform: info.geotransform,
        no_data: info.no_data_value,
        values,
    })
}

/// Band 1 of a dataset (the C++'s `readBandOne(const DatasetValue&)`): a grid
/// as it is, a file read through GDAL.
pub fn read_band_one(dataset: &DatasetValue) -> Result<BandValues> {
    match dataset {
        DatasetValue::Raster(grid) => {
            let Some(band) = grid.bands.first() else {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "the raster has no band",
                ));
            };
            Ok(BandValues {
                width: grid.info.width,
                height: grid.info.height,
                geotransform: grid.info.geotransform,
                no_data: grid.no_data.first().copied().flatten(),
                values: band.clone(),
            })
        }
        DatasetValue::Path(path) => read_band_one_from_file(&path_of_utf8(&path.path)),
        DatasetValue::Features(_) => Err(Error::new(
            ErrorCode::InvalidArgument,
            "a feature set is not a raster",
        )),
    }
}

/// Whether a cell of `band` holds a value: finite and not the no-data value.
pub fn holds_value(band: &BandValues, cell: f64) -> bool {
    // A no-data value is compared exactly, as GDAL compares it: it is the bit
    // pattern the file holds, not a measurement.
    cell.is_finite() && !band.no_data.is_some_and(|no_data| cell == no_data)
}

/// The area of one cell of a north-up or rotated grid: |gt1 gt5 - gt2 gt4|.
pub fn cell_area(geotransform: &[f64; 6]) -> f64 {
    (geotransform[1] * geotransform[5] - geotransform[2] * geotransform[4]).abs()
}

/// A raster file kept as a reference raster read from where it is, under
/// `name` or the first of name-2, name-3 ... no raster has: one a line wrote
/// where it said (save=<file>), which is the person's and so neither moved
/// nor copied, or one already in the derived folder (a mosaic's VRT). The
/// record says so as a derived raster's does, persisted as the caller says:
///
/// ```text
/// output arg=output kind=raster target=reference id=2 name=dem raster=40x30
///   file="C:/work/dem.tif" persisted=yes
/// ```
///
/// The C++'s default `arg` is "output".
pub fn keep_in_place(
    context: &mut Context<'_>,
    file: &Path,
    name: &str,
    derivation: &str,
    persisted: bool,
    arg: &str,
) -> Result<String> {
    let mut unique = String::from(name);
    let mut copy = 2;
    while seams::reference_name_taken(context, &unique) {
        unique = format!("{name}-{copy}");
        copy += 1;
    }
    Ok(seams::keep_derived_raster(context, file, &unique, derivation, persisted, arg)?.record)
}

// raster_info_of, the facts of a raster dataset, is terrain_verbs.rs's.

/// `from` renamed to `to`, or copied and removed when they are on different
/// volumes; the folder made. FileExportFailure naming `to`.
pub fn move_file(from: &Path, to: &Path) -> Status {
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if let Err(error) = std::fs::copy(from, to) {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("could not keep the file: {}", system_message(&error)),
            utf8_of_path(to),
        ));
    }
    let _ = std::fs::remove_file(from);
    Ok(())
}
