//! Where imported data lands (include/katana/cad/import_placement.hpp;
//! docs/interop.md, "Placing an import"): at its own coordinates, moved so
//! its lower-left corner sits at 0,0, moved onto the drawing, or moved by a
//! given offset. One definition for every importer and every front end - the
//! window's IMPORT line and its import dialogs, katana_cli's and katana_mcp's
//! IMPORT - so a placement cannot mean one thing typed and another chosen in
//! a dialog.
//!
//! Here, in the CAD core, because both front ends read the word off an IMPORT
//! line (the interpreter's import argument) and the native DXF import needs
//! it in a build without GDAL, where interop's advise_placement does not
//! exist.
//!
//! The readers take the move as an origin shift they SUBTRACT from every
//! coordinate (VectorImportOptions::origin_shift and its kin), so the file is
//! read again with it rather than moved afterwards: one reader applies one
//! shift to every kind of geometry it holds.

use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double};
use katana_core::{ErrorCode, Result};
use katana_geometry::{Box2, Vec2};

const OFFSET_PREFIX: &str = "OFFSET=";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImportPlacementMode {
    /// Its own coordinates (the window still asks when it lands far away).
    #[default]
    Keep,
    /// Lower-left corner to 0,0: LOCAL.
    Local,
    /// Lower-left corner onto the drawing's: ALONGSIDE.
    Alongside,
    /// Moved by a given east and north: OFFSET=dE,dN.
    Offset,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImportPlacement {
    pub mode: ImportPlacementMode,
    /// For Offset: what is ADDED to every coordinate, east then north.
    pub offset: Vec2,
}

// A coordinate as the log prints one: to the millimetre, as the extents are
// (the C++'s snprintf "%.3f"). + 0.0 turns the -0 a subtraction can leave
// into 0, so a shift of nothing is never "-0.000".
fn millimetres(value: f64) -> String {
    let value = value + 0.0;
    if value.is_nan() {
        return String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        });
    }
    if value.is_infinite() {
        return String::from(if value < 0.0 { "-inf" } else { "inf" });
    }
    format!("{value:.3}")
}

fn pair(value: Vec2) -> String {
    format!("{},{}", millimetres(value.x), millimetres(value.y))
}

/// The word an IMPORT line ends in for `placement`: "LOCAL", "ALONGSIDE",
/// "OFFSET=dE,dN" (each number the shortest text that reads back exactly),
/// or "" for Keep, which needs none.
pub fn placement_word(placement: &ImportPlacement) -> String {
    match placement.mode {
        ImportPlacementMode::Keep => String::new(),
        ImportPlacementMode::Local => String::from("LOCAL"),
        ImportPlacementMode::Alongside => String::from("ALONGSIDE"),
        ImportPlacementMode::Offset => format!(
            "{OFFSET_PREFIX}{},{}",
            format_exact_real(placement.offset.x),
            format_exact_real(placement.offset.y)
        ),
    }
}

/// The placement `word` names, in any case: LOCAL, ALONGSIDE or
/// OFFSET=dE,dN. None when it names none, so the word is part of a file
/// name; InvalidArgument for an OFFSET= whose value is not two finite
/// numbers, which is a mistyped placement and not a file.
pub fn parse_placement_word(word: &str) -> Result<Option<ImportPlacement>> {
    if equals_ignoring_case(word.as_bytes(), b"LOCAL") {
        return Ok(Some(ImportPlacement {
            mode: ImportPlacementMode::Local,
            offset: Vec2::default(),
        }));
    }
    if equals_ignoring_case(word.as_bytes(), b"ALONGSIDE") {
        return Ok(Some(ImportPlacement {
            mode: ImportPlacementMode::Alongside,
            offset: Vec2::default(),
        }));
    }
    let bytes = word.as_bytes();
    if bytes.len() < OFFSET_PREFIX.len()
        || !equals_ignoring_case(&bytes[..OFFSET_PREFIX.len()], OFFSET_PREFIX.as_bytes())
    {
        return Ok(None);
    }
    // The prefix matched ASCII bytes, so its end is a character boundary.
    let value = &word[OFFSET_PREFIX.len()..];
    let comma = value.find(',');
    let east = comma.and_then(|at| parse_finite_double(&value[..at]));
    let north = comma.and_then(|at| parse_finite_double(&value[at + 1..]));
    let (Some(east), Some(north)) = (east, north) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "OFFSET= takes the east and north to move by, as OFFSET=dE,dN",
            word,
        ));
    };
    Ok(Some(ImportPlacement {
        mode: ImportPlacementMode::Offset,
        offset: Vec2::new(east, north),
    }))
}

/// What a placement does to one import, worked out once the data has been
/// read at its own coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportShift {
    /// The origin shift to read the data again with - subtracted from every
    /// coordinate - or None when it stays where it is: Keep; Alongside into
    /// an empty drawing, which has nothing to sit beside; and data with no
    /// extent, which has nothing to move.
    pub shift: Option<Vec2>,
    /// What the log says it did, or why it did nothing; "" for Keep.
    pub said: String,
}

/// `drawing` is the drawing's extent before the import, `incoming` the data's
/// at its own coordinates. Alongside is the move the far-apart question's
/// Shift Alongside makes (interop's advise_placement suggested shift): the
/// data's lower-left corner onto the drawing's.
pub fn resolve_import_shift(
    placement: &ImportPlacement,
    drawing: &Box2,
    incoming: &Box2,
) -> ImportShift {
    let mut result = ImportShift::default();
    if placement.mode == ImportPlacementMode::Keep {
        return result;
    }
    let word = if placement.mode == ImportPlacementMode::Offset {
        String::from("OFFSET")
    } else {
        placement_word(placement)
    };
    if incoming.empty() {
        result.said =
            format!("{word}: the file holds nothing with a position, so there is nothing to move.");
        return result;
    }
    match placement.mode {
        ImportPlacementMode::Keep => {}
        ImportPlacementMode::Local => {
            result.shift = Some(Vec2::new(incoming.min.x, incoming.min.y));
            result.said = format!(
                "LOCAL: moved as one piece by {}, so its lower-left corner sits at 0,0.",
                pair(Vec2::new(0.0 - incoming.min.x, 0.0 - incoming.min.y))
            );
        }
        ImportPlacementMode::Alongside => {
            if drawing.empty() {
                result.said = String::from(
                    "ALONGSIDE: the drawing is empty, so there is nothing to sit beside; the data keeps its own coordinates.",
                );
            } else {
                result.shift = Some(Vec2::new(
                    incoming.min.x - drawing.min.x,
                    incoming.min.y - drawing.min.y,
                ));
                result.said = format!(
                    "ALONGSIDE: moved as one piece by {}, so its lower-left corner sits on the drawing's, at {}.",
                    pair(Vec2::new(
                        drawing.min.x - incoming.min.x,
                        drawing.min.y - incoming.min.y
                    )),
                    pair(Vec2::new(drawing.min.x, drawing.min.y))
                );
            }
        }
        ImportPlacementMode::Offset => {
            result.shift = Some(Vec2::new(
                0.0 - placement.offset.x,
                0.0 - placement.offset.y,
            ));
            result.said = format!("OFFSET: moved as one piece by {}.", pair(placement.offset));
        }
    }
    result
}
