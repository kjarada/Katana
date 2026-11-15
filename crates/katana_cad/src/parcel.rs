//! Parcels: the courses of a boundary as bearings and distances, its area, and
//! the words a title document carries (PLAN.MD Phase 21;
//! include/katana/cad/parcel.hpp, src/katana_cad/parcel.cpp).
//!
//! A parcel here is any closed polyline in the drawing. NOTHING IS STORED: the
//! report is computed from the geometry on demand, so it can never disagree
//! with the boundary it describes, and there is no second table to keep in
//! step when a corner is moved. Labels, when asked for, are ordinary text
//! entities the user owns afterwards - they are a snapshot, and say so by
//! being editable like any other text.
//!
//! Conventions, all inherited rather than invented here:
//!
//! * x is easting and y is northing, as everywhere in the drawing; the survey
//!   layer's Coordinate2 is (northing, easting), and the conversion happens in
//!   exactly one place, to_survey below.
//! * Azimuths are clockwise from grid north, from the survey layer's inverse.
//! * Bearings are the quadrant form surveyors read, N 45°30'15" E, from the
//!   survey layer's format_bearing, to whole seconds - the precision a deed
//!   quotes.
//! * Distances are horizontal, in model units, to three decimals.

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::TextGeometry;
use katana_entity::dimension_text::format_measurement;
use katana_entity::tables::DimensionStyle;
use katana_geometry::{Point2, Polyline2};
use katana_math::{CompensatedSum, PI, RAD_TO_DEG, normalize_angle_signed, tolerance};
use katana_survey::Coordinate2;
use katana_survey::angles::format_bearing;
use katana_survey::cogo::{inverse, polygon_centroid, polygon_signed_area};

use crate::code_edit::csv_field;
use crate::survey_tools::c_fixed;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParcelCourse {
    pub from: Point2,
    pub to: Point2,
    /// Radians clockwise from north, [0, 2 pi).
    pub azimuth: f64,
    /// Model units.
    pub distance: f64,
    /// Quadrant form, whole seconds.
    pub bearing: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParcelReport {
    /// In boundary order, closing course last.
    pub courses: Vec<ParcelCourse>,
    /// Square model units, always >= 0.
    pub area: f64,
    pub perimeter: f64,
    pub centroid: Point2,
    /// The order the boundary was drawn in.
    pub clockwise: bool,
}

// The ONE place the drawing's (x, y) becomes the survey layer's
// (northing, easting). Get this wrong and every bearing is wrong while every
// area stays right, which is why the rectangle test checks bearings.
fn to_survey(point: Point2) -> Coordinate2 {
    Coordinate2::new(point.y, point.x)
}

// Through the one measurement formatter (locale-independent, ties away from
// zero), so a parcel's "100.000" is spelled the way a dimension's is.
fn fixed(value: f64, decimals: i32) -> String {
    let style = DimensionStyle {
        decimals,
        suppress_trailing_zeros: false,
        ..DimensionStyle::default()
    };
    format_measurement(value, &style)
}

// A width for text this code cannot measure: 0.6 of the height per character
// is the usual aspect of a CAD stroke font, and only the centring depends on
// it. A label centred a little off is still readable; the code that draws
// text has the real metrics and is the wrong layer for this to depend on.
// (Characters are the C++ string's bytes.)
fn approximate_width(text: &str, height: f64) -> f64 {
    0.6 * height * text.len() as f64
}

/// Fails with InvalidGeometry for an open polyline, fewer than three distinct
/// vertices, or a degenerate (collinear) boundary; InvalidArgument for a
/// non-finite coordinate. Consecutive coincident vertices are dropped rather
/// than reported as zero-length courses with no bearing.
pub fn parcel_report(boundary: &Polyline2) -> Result<ParcelReport> {
    if !boundary.closed {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a parcel boundary must be a closed polyline",
        ));
    }
    let mut corners: Vec<Point2> = Vec::with_capacity(boundary.vertices.len());
    for &vertex in &boundary.vertices {
        if !vertex.x.is_finite() || !vertex.y.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a parcel corner is not finite",
            ));
        }
        // A corner clicked twice is one corner, not a zero-length course
        // with no bearing.
        if corners
            .last()
            .is_none_or(|last| last.distance_to(vertex) >= tolerance::COORDINATE)
        {
            corners.push(vertex);
        }
    }
    if corners.len() > 1
        && corners[0].distance_to(corners[corners.len() - 1]) < tolerance::COORDINATE
    {
        corners.pop(); // the closing vertex repeated the first
    }
    if corners.len() < 3 {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "a parcel needs at least three distinct corners",
            format!("{} found", corners.len()),
        ));
    }

    let ring: Vec<Coordinate2> = corners.iter().map(|&corner| to_survey(corner)).collect();
    let signed_area = polygon_signed_area(&ring)?;
    if signed_area.abs() <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the boundary encloses no area: its corners are collinear",
        ));
    }
    let centroid = polygon_centroid(&ring)?;

    let mut report = ParcelReport {
        clockwise: signed_area < 0.0,
        area: signed_area.abs(),
        centroid: Point2::new(centroid.easting, centroid.northing),
        ..ParcelReport::default()
    };
    let mut perimeter = CompensatedSum::new();
    for (i, &from) in corners.iter().enumerate() {
        let to = corners[(i + 1) % corners.len()];
        let leg = inverse(to_survey(from), to_survey(to))?;
        // Whole seconds: the precision a deed quotes.
        let bearing = format_bearing(leg.azimuth, 0)?;
        report.courses.push(ParcelCourse {
            from,
            to,
            azimuth: leg.azimuth,
            distance: leg.distance,
            bearing,
        });
        perimeter.add(leg.distance);
    }
    report.perimeter = perimeter.value();
    Ok(report)
}

/// The report's last line: "area 5000.000 m2 (0.5000 ha), perimeter 300.000
/// m, centroid 50.000,25.000, drawn counter-clockwise".
pub fn format_parcel_summary(report: &ParcelReport) -> String {
    // A stream in std::fixed with precision 3: the C library's "%.3f" text.
    let f = |value: f64| c_fixed(value, 3);
    format!(
        "area {} m2 ({} ha), perimeter {} m, centroid {},{}, drawn {}",
        f(report.area),
        f(report.area / 10000.0),
        f(report.perimeter),
        f(report.centroid.x),
        f(report.centroid.y),
        if report.clockwise {
            "clockwise"
        } else {
            "counter-clockwise"
        }
    )
}

/// What PARCEL id prints and Survey > Parcel Report shows: a row per course -
/// its number, the corner it runs from, its bearing and its distance - then
/// format_parcel_summary's line. One formatter for both, so the dialog and
/// the command line cannot come to differ.
pub fn format_parcel_report(report: &ParcelReport) -> String {
    let f = |value: f64| c_fixed(value, 3);
    let mut out = String::from("  course  from                      bearing           distance\n");
    for (index, course) in (1usize..).zip(&report.courses) {
        out.push_str(&format!(
            "  {index:>4}    {:>10},{:>10}   {}   {:>10}\n",
            f(course.from.x),
            f(course.from.y),
            course.bearing,
            f(course.distance)
        ));
    }
    out.push_str("  ");
    out.push_str(&format_parcel_summary(report));
    out
}

/// The courses as CSV (RFC 4180: CRLF, a field quoted where it needs it, as
/// csv_field does) with a header record: the course number, the corners it
/// runs from and to, its azimuth in decimal degrees, its bearing and its
/// distance - for a spreadsheet or a plan's schedule.
pub fn parcel_courses_csv(report: &ParcelReport) -> String {
    // Coordinates may be negative in a local grid, which the measurement
    // formatter (fixed() above) is not for; std::format's "{:.Nf}" is the
    // locale-free one that takes a sign.
    let number = |value: f64, decimals: i32| {
        let mut text = c_fixed(value, decimals);
        if text.starts_with('-') && !text.bytes().any(|byte| (b'1'..=b'9').contains(&byte)) {
            text.remove(0); // "-0.000" is zero to the precision shown
        }
        text
    };
    let mut text = String::from(
        "Course,From easting,From northing,To easting,To northing,Azimuth (deg),Bearing,\
         Distance\r\n",
    );
    for (index, course) in (1usize..).zip(&report.courses) {
        text.push_str(&format!(
            "{index},{},{},{},{},{},{},{}\r\n",
            number(course.from.x, 3),
            number(course.from.y, 3),
            number(course.to.x, 3),
            number(course.to.y, 3),
            number(course.azimuth * RAD_TO_DEG, 6),
            csv_field(&course.bearing),
            number(course.distance, 3)
        ));
    }
    text
}

/// The description a deed carries:
///
/// ```text
///   "<name>: Beginning at E 100.000 N 200.000; thence N 45°30'15" E, 120.000
///    m; thence ...; to the point of beginning. Containing 5000.000 m²
///    (0.5000 ha)."
/// ```
///
/// One course per "thence", in order. The area is given in both square metres
/// and hectares because titles quote whichever the jurisdiction uses.
pub fn legal_description(report: &ParcelReport, name: &str) -> String {
    let mut text = String::from(name);
    if !text.is_empty() {
        text.push_str(": ");
    }
    // The C++ reads the first course unchecked; a report always has three.
    let start = report
        .courses
        .first()
        .map_or_else(Point2::default, |course| course.from);
    text.push_str(&format!(
        "Beginning at E {} N {}",
        fixed(start.x, 3),
        fixed(start.y, 3)
    ));
    for course in &report.courses {
        text.push_str(&format!(
            "; thence {}, {} m",
            course.bearing,
            fixed(course.distance, 3)
        ));
    }
    text.push_str(&format!(
        " to the point of beginning. Containing {} m\u{b2} ({} ha).",
        fixed(report.area, 3),
        fixed(report.area / 10000.0, 4)
    ));
    text
}

/// Text entities labelling each course with its bearing over its distance,
/// placed just inside the boundary at the course midpoint and rotated to read
/// along the course - flipped where needed so no label is upside down - plus
/// the area at the centroid. `height` is the text height in model units.
/// Fails with InvalidArgument for a non-positive height.
pub fn parcel_labels(report: &ParcelReport, height: f64) -> Result<Vec<TextGeometry>> {
    if !(height > 0.0) || !height.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the label height must be positive",
        ));
    }
    let mut labels = Vec::with_capacity(report.courses.len() + 1);
    // Clear of the line by a fraction of the text height, so a label neither
    // touches the boundary nor floats away from it.
    let gap = 0.6 * height;

    for course in &report.courses {
        let mid = (course.from + course.to) * 0.5;
        let along = (course.to.y - course.from.y).atan2(course.to.x - course.from.x);
        // Readable: along the course, but never past a quarter turn either
        // way, so a course running west or south has its label flipped to
        // read left to right or upward like every other.
        let mut rotation = normalize_angle_signed(along);
        if rotation > PI / 2.0 || rotation <= -PI / 2.0 {
            rotation = normalize_angle_signed(rotation + PI);
        }
        // Inside the boundary: to the left of the course when the boundary
        // runs counter-clockwise, to the right when clockwise.
        let side = if report.clockwise { -1.0 } else { 1.0 };
        let inward = Point2::new(-along.sin() * side, along.cos() * side);
        // The text's own up direction after any flip. When it points away
        // from the inside, the glyphs would hang outside the boundary, so the
        // baseline moves in by a further text height to keep them in.
        let up = Point2::new(-rotation.sin(), rotation.cos());
        let inset = gap
            + if up.x * inward.x + up.y * inward.y < 0.0 {
                height
            } else {
                0.0
            };

        let text = format!("{}  {}", course.bearing, fixed(course.distance, 3));
        let width = approximate_width(&text, height);
        let reading_direction = Point2::new(rotation.cos(), rotation.sin());
        labels.push(TextGeometry {
            position: mid + inward * inset - reading_direction * (0.5 * width),
            text,
            height,
            rotation,
            ..TextGeometry::default()
        });
    }

    let text = format!("{} m\u{b2}", fixed(report.area, 3));
    let position = Point2::new(
        report.centroid.x - 0.5 * approximate_width(&text, height),
        report.centroid.y - 0.5 * height,
    );
    labels.push(TextGeometry {
        position,
        text,
        height,
        rotation: 0.0,
        ..TextGeometry::default()
    });
    Ok(labels)
}
