//! The records IMPORT and EXPORT reply with (docs/interop.md, "Replies"),
//! the port of src/katana_app/import_records.hpp/.cpp. The DXF steps, the
//! survey and update verbs and the geoprocessing executor's IMPORT and EXPORT
//! write the same words, so a script reads one reply whichever verb ran, and
//! an agent reads the same fields from a DXF as from a GeoPackage.
//!
//! ```text
//! imported file=site.dxf kind=dxf entities=9 bounds=0,0,30,20 ...
//! placed placement=local east=-180 north=0 text="LOCAL: moved as one piece by ..."
//! tally element=LINE read=3 imported=3
//! exported file=site.dxf kind=dxf driver=DXF entities=9 ...
//! warning text="..."
//! ```

use std::path::{Path, PathBuf};

use katana_cad::scope_verbs::record_value;
use katana_cad::{ImportPlacement, ImportPlacementMode, ImportShift};
use katana_core::path_text::{generic_path_utf8, path_from_utf8};
use katana_core::text::{format_exact_real, trimmed};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_geometry::Box2;

/// A file's path as a record and a line give it: UTF-8, forward slashes. A
/// path whose bytes are not UTF-8 (a POSIX name) is decoded as the port
/// decodes such a name everywhere, where the C++ hands its bytes on.
pub fn path_text(path: &Path) -> String {
    utf8_or_windows1252(generic_path_utf8(path))
}

/// A line's UTF-8 path as the path it names, whatever the code page.
pub fn path_from_text(utf8: &str) -> PathBuf {
    path_from_utf8(utf8)
}

/// The rest of a line after its verb, blanks trimmed and one pair of
/// surrounding quotes removed: EXPORT's and INFO's path, which works quoted
/// or, holding blanks, not.
pub fn rest_of_line(line: &str) -> String {
    let body = trimmed(line.as_bytes());
    let mut body = match body.iter().position(|&byte| byte == b' ' || byte == b'\t') {
        Some(blank) => trimmed(&body[blank..]),
        None => &[],
    };
    if body.len() >= 2 && body[0] == b'"' && body[body.len() - 1] == b'"' {
        body = &body[1..body.len() - 1];
    }
    // Every cut is at an ASCII byte of UTF-8 text, so nothing is replaced.
    String::from_utf8_lossy(body).into_owned()
}

/// A value as a record carries it: cad's record value - plain, or quoted
/// when it holds a blank, a quote or an '=' - and nothing at all for nothing.
pub fn record_text(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        record_value(value)
    }
}

/// A number in the shortest text that reads back exactly.
pub fn record_number(value: f64) -> String {
    // + 0.0: a -0 written as "-0" reads as a move where there is none.
    format_exact_real(value + 0.0)
}

/// "x0,y0,x1,y1"; "" for an empty box, which has no corners to give.
pub fn bounds_text(box2: &Box2) -> String {
    if box2.empty() {
        return String::new();
    }
    format!(
        "{},{},{},{}",
        record_number(box2.min.x),
        record_number(box2.min.y),
        record_number(box2.max.x),
        record_number(box2.max.y)
    )
}

/// Where a placement put what was read, and the sentence that says so:
/// `placed placement=alongside east=820 north=2000 text="ALONGSIDE: moved ..."`.
/// east and north are what was ADDED to every coordinate, as OFFSET=dE,dN
/// gives them, empty when nothing moved (ALONGSIDE into an empty drawing).
/// "" for Keep, which moves nothing and says nothing.
pub fn placed_record(placement: &ImportPlacement, placed: &ImportShift) -> String {
    let mode = match placement.mode {
        ImportPlacementMode::Keep => return String::new(),
        ImportPlacementMode::Local => "local",
        ImportPlacementMode::Alongside => "alongside",
        ImportPlacementMode::Offset => "offset",
    };
    // The readers SUBTRACT the shift (cad's import_placement); the record
    // says the move, which is what was added.
    let east = placed
        .shift
        .map(|shift| record_number(-shift.x))
        .unwrap_or_default();
    let north = placed
        .shift
        .map(|shift| record_number(-shift.y))
        .unwrap_or_default();
    format!(
        "placed placement={mode} east={east} north={north} text={}",
        record_text(&placed.said)
    )
}

/// `tally element=<what> read=<n> imported=<n>`: one kind of element a file
/// held and how much of it came in.
pub fn tally_record(element: &str, read: usize, imported: usize) -> String {
    format!(
        "tally element={} read={read} imported={imported}",
        record_text(element)
    )
}

/// `warning text="..."`
pub fn warning_text(text: &str) -> String {
    format!("warning text={}", record_text(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use katana_geometry::{Point2, Vec2};

    // The rest of a line: trimmed, its verb gone, one pair of quotes removed.
    #[test]
    fn the_rest_of_a_line_is_its_path_with_one_pair_of_quotes_removed() {
        assert_eq!(
            rest_of_line("  EXPORT   \"site plan.dxf\"  "),
            "site plan.dxf"
        );
        assert_eq!(rest_of_line("EXPORT site plan.dxf"), "site plan.dxf");
        assert_eq!(rest_of_line("EXPORT"), "");
        assert_eq!(rest_of_line("INFO\t\"\""), "");
    }

    // -0 is 0 in a record: nothing moved.
    #[test]
    fn a_negative_zero_is_recorded_as_zero() {
        assert_eq!(record_number(-0.0), "0");
    }

    #[test]
    fn an_empty_box_has_no_bounds_text() {
        assert_eq!(bounds_text(&Box2::default()), "");
        let box2 = Box2 {
            min: Point2 { x: 0.0, y: 0.0 },
            max: Point2 { x: 30.0, y: 20.0 },
        };
        assert_eq!(bounds_text(&box2), "0,0,30,20");
    }

    // The record says the move added, the negation of the shift subtracted.
    #[test]
    fn a_placement_records_the_move_added_and_keep_records_nothing() {
        let shift = ImportShift {
            shift: Some(Vec2 { x: 180.0, y: 0.0 }),
            said: String::from("LOCAL: moved"),
        };
        let local = ImportPlacement {
            mode: ImportPlacementMode::Local,
            ..ImportPlacement::default()
        };
        assert_eq!(
            placed_record(&local, &shift),
            "placed placement=local east=-180 north=0 text=\"LOCAL: moved\""
        );
        assert_eq!(placed_record(&ImportPlacement::default(), &shift), "");
        let nothing = ImportShift {
            shift: None,
            said: String::from("nothing"),
        };
        let alongside = ImportPlacement {
            mode: ImportPlacementMode::Alongside,
            ..ImportPlacement::default()
        };
        assert_eq!(
            placed_record(&alongside, &nothing),
            "placed placement=alongside east= north= text=nothing"
        );
    }

    #[test]
    fn a_tally_and_a_warning_carry_their_text_as_record_values() {
        assert_eq!(
            tally_record("LINE", 3, 2),
            "tally element=LINE read=3 imported=2"
        );
        assert_eq!(warning_text("two words"), "warning text=\"two words\"");
        assert_eq!(warning_text(""), "warning text=");
    }
}
