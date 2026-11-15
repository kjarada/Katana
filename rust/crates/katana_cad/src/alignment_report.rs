//! What an alignment reports: its setting-out table and its design profile's
//! elements (docs/cad.md, "The Alignment Manager";
//! include/katana/cad/alignment_report.hpp, src/katana_cad/alignment_report.cpp).
//!
//! ONE function per report, read by both front ends: ALIGN STATIONS and ALIGN
//! PROFILE print these, and Terrain > Alignment Manager shows them in its
//! tables. A number that appeared only in the dialog would be a second answer
//! nobody tests; a station the dialog listed and the verb did not would be
//! worse.
//!
//! Nothing is stored. Like a parcel's report, the table is worked out from
//! the solved alignment when asked for, so it cannot disagree with the PIs it
//! came from.

use std::cmp::Ordering;

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};
use katana_geometry::alignment::kind_of;
use katana_geometry::{
    AlignmentElementKind, Point2, ProfileElement, ProfileElementKind, SolvedAlignment,
    SolvedProfile, VerticalAlignment,
};
use katana_math::{HALF_PI, RAD_TO_DEG, TWO_PI, cpp, nearly_zero, tolerance};
use katana_survey::angles::format_dms;

use crate::code_edit::csv_field;
use crate::survey_tools::c_fixed;

/// One row of a setting-out table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingOutStation {
    pub station: f64,
    pub point: Point2,
    /// Radians counter-clockwise from +x, as SolvedAlignment gives it.
    pub direction: f64,
    /// The same direction as a surveyor reads it: radians clockwise from grid
    /// north, in [0, 2 pi).
    pub azimuth: f64,
    /// Signed, positive turning left; 0 on a tangent. On a spiral it is the
    /// curvature at this station, which is what the peg there is set out on.
    pub curvature: f64,
    /// What the station is, when it is where the geometry changes character:
    /// "start" and "end"; "TS", "SC", "CS", "ST" through a spiralled curve;
    /// "TC" and "CT" either side of a simple curve; "PI" at a corner with no
    /// curve; the two letters of the elements met ("SS", "CC") where two
    /// curves run back to back. Empty for a station that is on the interval
    /// only.
    pub key: String,
}

// More rows than a person can use: a mistyped interval on a long road, which
// would otherwise build a table of millions and hang the dialog showing it.
const MAXIMUM_STATIONS: usize = 100_000;

// Fixed decimals, locale-independent, and never "-0.000": a coordinate a hair
// below zero reads as zero, as it is to the precision shown. std::to_chars
// into 64 characters, as the C++ calls it: a number whose text does not fit
// is "overflow".
fn fixed(value: f64, decimals: i32) -> String {
    let mut text = c_fixed(value, decimals);
    if value.is_finite() && text.len() > 64 {
        return String::from("overflow");
    }
    if text.starts_with('-') && text.bytes().all(|byte| matches!(byte, b'-' | b'0' | b'.')) {
        text.remove(0);
    }
    text
}

fn letter_of(kind: AlignmentElementKind) -> char {
    match kind {
        AlignmentElementKind::Tangent => 'T',
        AlignmentElementKind::Spiral => 'S',
        AlignmentElementKind::Arc => 'C',
    }
}

// The name of the joint between two elements: the letters of the two kinds
// met, which is how TS, SC, CS, ST, TC and CT are made - except two tangents,
// which meet at a PI with no curve.
fn joint_name(before: AlignmentElementKind, after: AlignmentElementKind) -> String {
    if before == AlignmentElementKind::Tangent && after == AlignmentElementKind::Tangent {
        return String::from("PI");
    }
    [letter_of(before), letter_of(after)].iter().collect()
}

// Azimuth clockwise from grid north, in [0, 2 pi), from a direction
// counter-clockwise from +x.
fn azimuth_of(direction: f64) -> f64 {
    let mut azimuth = (HALF_PI - direction) % TWO_PI;
    if azimuth < 0.0 {
        azimuth += TWO_PI;
    }
    // Due north computed a hair west of north is north: a direction within
    // the angular tolerance of a whole turn is parallel to it. (fmod of a
    // value a hair below a turn, plus a turn, can also round up to 2 pi.)
    if azimuth >= TWO_PI - tolerance::ANGULAR {
        0.0
    } else {
        azimuth
    }
}

struct Candidate {
    station: f64,
    key: String,
}

/// Every station `interval` apart from the alignment's start, and every key
/// station (SolvedAlignment::key_stations), ascending, each once: a key
/// station the interval lands on is one row, carrying its key. A setting-out
/// table without the TS, SC, CS and ST is not one - those are the points that
/// get pegged - so the key stations are always there.
///
/// Fails with InvalidArgument for an interval that is not positive and
/// finite, and for one that would give more than 100 000 stations (a mistyped
/// 0.001 on a 20 km road), naming the count.
pub fn setting_out_stations(
    alignment: &SolvedAlignment,
    interval: f64,
) -> Result<Vec<SettingOutStation>> {
    if !interval.is_finite() || !(interval > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the interval must be positive",
            format!("interval={}", fixed(interval, 3)),
        ));
    }
    let start = alignment.start_station();
    let end = alignment.end_station();
    // Counted before a row is made: a table that is refused for its size
    // must not be built first.
    let interval_count = ((end - start) / interval).floor() + 1.0;
    if interval_count > MAXIMUM_STATIONS as f64 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "that interval gives too many stations",
            format!(
                "{} stations, the limit is {MAXIMUM_STATIONS}",
                fixed(interval_count, 0)
            ),
        ));
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    let keys = alignment.key_stations();
    let elements = alignment.elements();
    // key_stations is the start and then every element's end, in order, so
    // key k is the joint between element k-1 and element k.
    for (k, &station) in keys.iter().enumerate() {
        let key = if k == 0 {
            String::from("start")
        } else if k == keys.len() - 1 {
            String::from("end")
        } else {
            joint_name(kind_of(&elements[k - 1]), kind_of(&elements[k]))
        };
        candidates.push(Candidate { station, key });
    }
    // Multiplied rather than accumulated, so the hundredth station is not
    // off by a hundred roundings.
    for i in 0usize.. {
        let station = start + i as f64 * interval;
        if !(station < end) {
            break;
        }
        candidates.push(Candidate {
            station,
            key: String::new(),
        });
    }
    // Stable, by `a.station < b.station`, as the C++'s stable_sort.
    candidates.sort_by(|a, b| a.station.partial_cmp(&b.station).unwrap_or(Ordering::Equal));
    // A station the interval lands on to within the geometric tolerance IS
    // the key station there: one row, which keeps the key and the key's own
    // station.
    let mut merged: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        if let Some(last) = merged.last_mut()
            && candidate.station - last.station <= tolerance::GEOMETRIC
        {
            if last.key.is_empty() && !candidate.key.is_empty() {
                *last = candidate;
            }
            continue;
        }
        merged.push(candidate);
    }

    let mut rows = Vec::with_capacity(merged.len());
    for candidate in merged {
        // The last key is the sum of the element lengths and the end station
        // the total length, which can differ in the last bit; a query just
        // past the end would answer nothing.
        let station = cpp::clamp(candidate.station, start, end);
        let point = alignment.point_at_station(station);
        let direction = alignment.direction_at_station(station);
        let curvature = alignment.curvature_at_station(station);
        let (Some(point), Some(direction), Some(curvature)) = (point, direction, curvature) else {
            // Every station here is inside the alignment, so this is a
            // defect to report, never a row to leave out quietly.
            return Err(make_error(
                ErrorCode::Internal,
                "a station on the alignment has no position",
                format!("station={}", fixed(station, 6)),
            ));
        };
        rows.push(SettingOutStation {
            station,
            point,
            direction,
            azimuth: azimuth_of(direction),
            curvature,
            key: candidate.key,
        });
    }
    Ok(rows)
}

/// An azimuth (radians clockwise from north) in degrees, minutes and whole
/// seconds, the precision a setting-out sheet quotes: 61°21'08". What the
/// reply, the CSV and the Alignment Manager's table all print.
pub fn format_azimuth(azimuth: f64) -> String {
    match format_dms(azimuth, 0) {
        // Finite by construction everywhere this is called; a number is
        // still better than a blank if that ever stops being true.
        Err(_) => fixed(azimuth * RAD_TO_DEG, 6),
        Ok(mut text) => {
            // Within half a second of a whole turn rounds to 360 degrees,
            // which is north and is written 0.
            if text.starts_with("360") {
                text.replace_range(0..3, "0");
            }
            text
        }
    }
}

/// The reply of ALIGN STATIONS: a key=value record per station,
///
/// ```text
/// station=128.540 x=100.000 y=50.000 direction=90.000 azimuth=0°00'00" radius=straight key=ST
/// ```
///
/// to three decimals; `direction` in degrees counter-clockwise from +x, the
/// azimuth in whole seconds; `radius=50.000 turn=left` on a curve. The key is
/// left out on a station that has none, as absent is not blank.
pub fn format_setting_out(stations: &[SettingOutStation]) -> String {
    let mut text = String::new();
    for row in stations {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!(
            "station={} x={} y={} direction={} azimuth={}",
            fixed(row.station, 3),
            fixed(row.point.x, 3),
            fixed(row.point.y, 3),
            fixed(row.direction * RAD_TO_DEG, 3),
            format_azimuth(row.azimuth)
        ));
        if row.curvature == 0.0 {
            text.push_str(" radius=straight");
        } else {
            text.push_str(&format!(
                " radius={}{}",
                fixed(1.0 / row.curvature.abs(), 3),
                if row.curvature > 0.0 {
                    " turn=left"
                } else {
                    " turn=right"
                }
            ));
        }
        if !row.key.is_empty() {
            text.push_str(&format!(" key={}", row.key));
        }
    }
    text
}

/// The table as CSV (RFC 4180: CRLF, quoted where a field needs it) with a
/// header row - chainage, easting, northing, the azimuth in decimal degrees
/// and in degrees, minutes and seconds, the radius (blank on a tangent,
/// negative turning right) and the key - for a field book or a spreadsheet.
pub fn setting_out_csv(stations: &[SettingOutStation]) -> String {
    let mut text =
        String::from("Chainage,Easting,Northing,Azimuth (deg),Azimuth (DMS),Radius,Key\r\n");
    for row in stations {
        text.push_str(&format!(
            "{},{},{},{},{},{},{}\r\n",
            fixed(row.station, 3),
            fixed(row.point.x, 3),
            fixed(row.point.y, 3),
            fixed(row.azimuth * RAD_TO_DEG, 6),
            csv_field(&format_azimuth(row.azimuth)),
            if row.curvature == 0.0 {
                String::new()
            } else {
                fixed(1.0 / row.curvature, 3)
            },
            csv_field(&row.key)
        ));
    }
    text
}

/// The K value of a vertical curve: its length over the algebraic difference
/// of its grades in percent, the metres of curve per 1% change of grade that
/// road standards set minimums on. None for a tangent, or a curve whose
/// grades do not differ.
pub fn curve_k(element: &ProfileElement) -> Option<f64> {
    if element.kind != ProfileElementKind::Curve {
        return None;
    }
    let change_percent = (element.end_grade - element.start_grade).abs() * 100.0;
    // math::nearlyZero's default epsilon, tolerance::kAbsolute.
    if nearly_zero(change_percent, tolerance::ABSOLUTE) {
        return None;
    }
    Some(element.length / change_percent)
}

/// The reply of ALIGN PROFILE: the PVIs, then each element with its grades
/// (and K on a curve), then the high and low points.
pub fn format_profile_report(definition: &VerticalAlignment, profile: &SolvedProfile) -> String {
    // A stream in std::fixed with precision 3: the C library's "%.3f" text.
    let f = |value: f64| c_fixed(value, 3);
    let mut out = String::from("  PVIs:");
    for pvi in &definition.pvis {
        out.push_str(&format!("  {} @ {}", f(pvi.station), f(pvi.elevation)));
        if pvi.curve_length > 0.0 {
            out.push_str(&format!(" L={}", f(pvi.curve_length)));
        }
    }
    out.push('\n');
    for element in profile.elements() {
        let curve = element.kind == ProfileElementKind::Curve;
        out.push_str(&format!(
            "  {}  {:>10} to {:>10}  grade {:>7}%",
            if curve { "curve  " } else { "tangent" },
            f(element.start_station),
            f(element.start_station + element.length),
            f(element.start_grade * 100.0)
        ));
        if curve {
            out.push_str(&format!(" to {:>7}%", f(element.end_grade * 100.0)));
            if let Some(k) = curve_k(element) {
                out.push_str(&format!("  K={}", f(k)));
            }
        }
        out.push('\n');
    }
    for point in profile.high_low_points() {
        out.push_str(&format!(
            "  {} at {} @ {}\n",
            if point.high {
                "high point"
            } else {
                "low point "
            },
            f(point.station),
            f(point.elevation)
        ));
    }
    out.pop();
    out
}
