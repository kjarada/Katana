//! Precision input: the drafting aids every point a user gives passes through
//! (docs/drawing.md, "Precision input"; include/katana/cad/drawing/drafting.hpp):
//! the typed coordinate grammar with survey bearings and distances in degrees,
//! minutes and seconds, direct distance entry along the rubber band, length
//! and angle locks, ortho and polar tracking, and object snap tracking.
//!
//! Headless, like everything a tool decides: the plan view asks constrain()
//! what point the cursor gives, the tool host and the command line ask
//! parse_precise_point() what point some text is, and the settings they read
//! are the Document's (Document::drafting), so a view, the command line and an
//! agent's ORTHO / POLAR / SNAP / LOCK verbs all change the same state.
//!
//! ANGLES. The command line's convention is kept as the default: degrees
//! counter-clockwise from east. Survey work reads bearings - clockwise from
//! north - so the convention is a setting (AngleConvention::Bearing, the UNITS
//! ANGLE BEARING verb), and a QUADRANT bearing (N45d30'15"E) is a bearing
//! whatever the setting, since it cannot be read any other way. Angles are
//! typed as decimal degrees (45.5), or degrees, minutes and seconds with d or
//! the degree sign and ' and " (45d30'15", 45°30'15.25", 45d30'); internally
//! everything is radians counter-clockwise from east.
//!
//! The text is read as bytes, as the C++ reads a string_view: every mark the
//! grammar knows is ASCII, so a slice between two of them is a slice of the
//! UTF-8 it came from.

use katana_core::error::make_error;
use katana_core::text::{
    ascii_lower, format_exact_real, format_fixed, parse_finite_double, trimmed,
};
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::{Point2, Vec2};
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG, normalize_angle_signed, tolerance};

use crate::snapping::{DEFAULT_SNAP_MODES, SnapMode, SnapModes};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AngleConvention {
    /// Degrees counter-clockwise from east (the command line's).
    #[default]
    Counterclockwise,
    /// Degrees clockwise from north (whole-circle bearing).
    Bearing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DraftingSettings {
    /// Ortho: a point from a base goes straight across or straight up.
    pub ortho: bool,
    /// Polar tracking: a direction within the aperture of a multiple of the
    /// increment snaps onto it.
    pub polar: bool,
    /// Radians.
    pub polar_increment: f64,
    /// Object snap tracking: points the cursor has rested on are acquired, and
    /// the horizontal and vertical through each become paths to snap to.
    pub object_tracking: bool,
    pub angles: AngleConvention,
    /// Locks for the next point from a base: its direction (radians,
    /// counter-clockwise from east) and its distance. Typed as <angle and
    /// =distance at a point prompt; they apply until cleared.
    pub angle_lock: Option<f64>,
    pub length_lock: Option<f64>,
    /// The running object snaps.
    pub snap_enabled: bool,
    pub snap_modes: SnapModes,
}

impl Default for DraftingSettings {
    fn default() -> Self {
        Self {
            ortho: false,
            polar: false,
            polar_increment: 15.0 * DEG_TO_RAD,
            object_tracking: false,
            angles: AngleConvention::Counterclockwise,
            angle_lock: None,
            length_lock: None,
            snap_enabled: true,
            snap_modes: DEFAULT_SNAP_MODES,
        }
    }
}

/// U+00B0 in UTF-8.
const DEGREE_SIGN: &[u8] = b"\xC2\xB0";
const DEGREE_SIGN_TEXT: &str = "\u{B0}";

fn shown(text: &[u8]) -> String {
    String::from_utf8_lossy(text).into_owned()
}

fn not_an_angle(text: &[u8]) -> Error {
    Error::new(
        ErrorCode::ParseFailure,
        format!(
            "'{}' is not an angle; type degrees as 45.5 or 45d30'15\", or a bearing as N45d30'15\"E",
            shown(text)
        ),
    )
}

/// The text with the degree sign spelt 'd', so one scanner reads both.
fn normalised_angle(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(DEGREE_SIGN) {
            out.push(b'd');
            i += DEGREE_SIGN.len();
            continue;
        }
        match text[i] {
            b'D' => out.push(b'd'),
            b' ' => {}
            other => out.push(other),
        }
        i += 1;
    }
    out
}

fn is_mark(byte: u8) -> bool {
    matches!(byte, b'd' | b'\'' | b'"')
}

fn degrees(text: &[u8]) -> Result<f64> {
    let s = normalised_angle(trimmed(text));
    if s.is_empty() {
        return Err(not_an_angle(text));
    }
    if !s.iter().copied().any(is_mark) {
        return parse_finite_double(&s).ok_or_else(|| not_an_angle(text));
    }
    let mut sign = 1.0;
    let mut rest: &[u8] = &s;
    if rest[0] == b'-' || rest[0] == b'+' {
        sign = if rest[0] == b'-' { -1.0 } else { 1.0 };
        rest = &rest[1..];
    }
    let mut parts = [0.0_f64; 3];
    const MARKS: [u8; 3] = *b"d'\"";
    let mut next = 0;
    while !rest.is_empty() {
        let at = rest.iter().position(|&byte| is_mark(byte));
        let number = match at {
            Some(at) => &rest[..at],
            None => rest,
        };
        // A trailing number with no mark takes the next unit.
        let which = match at {
            None => next,
            Some(at) => MARKS
                .iter()
                .position(|&mark| mark == rest[at])
                .unwrap_or(MARKS.len()),
        };
        if which < next || which > 2 {
            return Err(not_an_angle(text));
        }
        let value = match parse_finite_double(number) {
            Some(value) if !(value < 0.0) => value,
            _ => return Err(not_an_angle(text)),
        };
        parts[which] = value;
        next = which + 1;
        rest = match at {
            None => &[],
            Some(at) => &rest[at + 1..],
        };
    }
    if parts[1] >= 60.0 || parts[2] >= 60.0 {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!("'{}' has 60 or more minutes or seconds", shown(text)),
        ));
    }
    Ok(sign * (parts[0] + parts[1] / 60.0 + parts[2] / 3600.0))
}

// ---- angles ------------------------------------------------------------------------------

/// Degrees from "45.5", "45d30'15\"", "45°30'15.25\"", "45d30'" or "45d";
/// a leading minus is kept. ParseFailure otherwise.
pub fn parse_degrees(text: &str) -> Result<f64> {
    degrees(text.as_bytes())
}

fn direction(text: &[u8], convention: AngleConvention) -> Result<f64> {
    let trimmed_text = trimmed(text);
    if trimmed_text.len() >= 3 {
        let first = ascii_lower(trimmed_text[0]);
        let last = ascii_lower(trimmed_text[trimmed_text.len() - 1]);
        if (first == b'n' || first == b's') && (last == b'e' || last == b'w') {
            let inside = degrees(&trimmed_text[1..trimmed_text.len() - 1]);
            let inside = match inside {
                Ok(inside) if !(inside < 0.0) && !(inside > 90.0) => inside,
                _ => {
                    return Err(Error::new(
                        ErrorCode::ParseFailure,
                        format!(
                            "'{}' is not a quadrant bearing; the angle from north or south is 0 \
                             to 90 degrees",
                            shown(text)
                        ),
                    ));
                }
            };
            let mut bearing = inside;
            if first == b's' && last == b'e' {
                bearing = 180.0 - inside;
            } else if first == b's' && last == b'w' {
                bearing = 180.0 + inside;
            } else if first == b'n' && last == b'w' {
                bearing = 360.0 - inside;
            }
            return Ok(normalize_angle_signed((90.0 - bearing) * DEG_TO_RAD));
        }
    }
    let value = degrees(trimmed_text)?;
    // Signed, (-pi, pi]: one number for one direction, however it was typed.
    Ok(normalize_angle_signed(
        (if convention == AngleConvention::Bearing {
            90.0 - value
        } else {
            value
        }) * DEG_TO_RAD,
    ))
}

/// A direction, radians counter-clockwise from east in (-pi, pi], from typed
/// text: a quadrant bearing ("N45d30'15\"E", "S 12.5 W") always as a bearing;
/// anything else as degrees in `convention`.
pub fn parse_direction(text: &str, convention: AngleConvention) -> Result<f64> {
    direction(text.as_bytes(), convention)
}

/// "%0<width>.<decimals>f" of a value that is not negative.
fn zero_padded(value: f64, width: usize, decimals: i32) -> String {
    let text = format_fixed(value, decimals);
    if text.len() >= width {
        return text;
    }
    let mut padded = "0".repeat(width - text.len());
    padded.push_str(&text);
    padded
}

/// Degrees as D°MM'SS" (seconds to `decimals` places, rounding carried).
pub fn format_dms(degrees: f64, decimals: i32) -> String {
    let decimals = decimals.clamp(0, 6);
    let scale = 10.0_f64.powf(f64::from(decimals));
    let negative = degrees < 0.0;
    // In whole units of the last place shown, so rounding carries into the
    // minutes and degrees (59.9995" at 3 places is the next minute).
    let units = (degrees.abs() * 3600.0 * scale).round();
    let per_minute = 60.0 * scale;
    let per_degree = 3600.0 * scale;
    let d = (units / per_degree).floor();
    let m = ((units - d * per_degree) / per_minute).floor();
    let s = (units - d * per_degree - m * per_minute) / scale;
    let sign = if negative { "-" } else { "" };
    let seconds = if decimals == 0 {
        zero_padded(s, 2, 0)
    } else {
        zero_padded(s, 3 + decimals as usize, decimals)
    };
    format!(
        "{sign}{}{DEGREE_SIGN_TEXT}{}'{seconds}\"",
        format_fixed(d, 0),
        zero_padded(m, 2, 0)
    )
}

/// The whole-circle bearing of a direction, 0 to 360 degrees clockwise from
/// north, as D°MM'SS" (seconds to `decimals` places, rounding carried).
pub fn format_bearing(radians: f64, decimals: i32) -> String {
    let mut bearing = (90.0 - radians * RAD_TO_DEG) % 360.0;
    if bearing < 0.0 {
        bearing += 360.0;
    }
    let text = format_dms(bearing, decimals);
    // A bearing that rounds up to 360 is north.
    if text.starts_with("360") {
        return format_dms(0.0, decimals);
    }
    text
}

/// A direction as a quadrant bearing, N45°30'15"E.
pub fn format_quadrant_bearing(radians: f64, decimals: i32) -> String {
    let mut bearing = (90.0 - radians * RAD_TO_DEG) % 360.0;
    if bearing < 0.0 {
        bearing += 360.0;
    }
    if bearing <= 90.0 {
        return format!("N{}E", format_dms(bearing, decimals));
    }
    if bearing <= 180.0 {
        return format!("S{}E", format_dms(180.0 - bearing, decimals));
    }
    if bearing <= 270.0 {
        return format!("S{}W", format_dms(bearing - 180.0, decimals));
    }
    format!("N{}W", format_dms(360.0 - bearing, decimals))
}

// ---- typed points -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PrecisePoint {
    pub point: Point2,
    /// A height given as the third coordinate.
    pub z: Option<f64>,
    /// Typed with @: the point is from `last`, and a z is a CHANGE of height
    /// (dz) to the height of whatever `last` is, which only the caller knows.
    pub relative: bool,
}

/// True when `text` looks like a point (a comma, or the @ of relative input).
pub fn looks_like_point(text: &str) -> bool {
    let input = trimmed(text.as_bytes());
    input.contains(&b',') || input.first() == Some(&b'@')
}

/// A typed point:
///
/// ```text
///   x,y[,z]                      absolute
///   @dx,dy[,dz]                  relative to `last`
///   @distance<direction          polar from `last`, the direction by
///                                parse_direction (a DMS angle, a quadrant
///                                bearing, or degrees in the convention)
/// ```
///
/// InvalidState for relative input with no `last`, ParseFailure otherwise.
pub fn parse_precise_point(
    text: &str,
    last: Option<Point2>,
    settings: &DraftingSettings,
) -> Result<PrecisePoint> {
    let input = trimmed(text.as_bytes());
    let relative = input.first() == Some(&b'@');
    let body = if relative { &input[1..] } else { input };
    let base = match (relative, last) {
        (true, None) => {
            return Err(make_error(
                ErrorCode::InvalidState,
                "a relative point needs a previous point",
                text,
            ));
        }
        (_, last) => last,
    };
    let mut out = PrecisePoint {
        relative,
        ..PrecisePoint::default()
    };
    if let Some(polar) = body.iter().position(|&byte| byte == b'<') {
        let Some(base) = base.filter(|_| relative) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "polar points are relative: @distance<angle",
                text,
            ));
        };
        let Some(distance) = parse_finite_double(trimmed(&body[..polar])) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "expected @distance<angle",
                text,
            ));
        };
        let towards = direction(&body[polar + 1..], settings.angles)?;
        out.point = base + Vec2::new(towards.cos(), towards.sin()) * distance;
        return Ok(out);
    }
    let mut values = [0.0_f64; 3];
    let mut count = 0;
    let mut rest = body;
    loop {
        let comma = rest.iter().position(|&byte| byte == b',');
        let token = trimmed(match comma {
            Some(comma) => &rest[..comma],
            None => rest,
        });
        let value = parse_finite_double(token);
        let Some(value) = value.filter(|_| count != 3) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "expected a point as x,y or x,y,z",
                text,
            ));
        };
        values[count] = value;
        count += 1;
        let Some(comma) = comma else {
            break;
        };
        rest = &rest[comma + 1..];
    }
    if count < 2 {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "expected a point as x,y or x,y,z",
            text,
        ));
    }
    let value = Vec2::new(values[0], values[1]);
    out.point = match base.filter(|_| relative) {
        Some(base) => base + value,
        None => value,
    };
    if count == 3 {
        out.z = Some(values[2]);
    }
    Ok(out)
}

/// Direct distance entry: the point `distance` from `base` towards `cursor`
/// (or along the angle lock when there is one; east when the cursor is on the
/// base).
pub fn direct_distance(
    base: Point2,
    cursor: Point2,
    distance: f64,
    settings: &DraftingSettings,
) -> Point2 {
    let mut direction = Vec2::new(1.0, 0.0);
    if let Some(lock) = settings.angle_lock {
        direction = Vec2::new(lock.cos(), lock.sin());
    } else if cursor.distance_to(base) > tolerance::GEOMETRIC {
        direction = (cursor - base).normalized();
    }
    base + direction * distance
}

// ---- constraining the cursor -----------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConstrainedPoint {
    pub point: Point2,
    /// What constrained it, for the view's tooltip: "Ortho", "Polar 45°",
    /// "Angle lock 30°", "Length lock 10", "Tracking"; empty when nothing did.
    pub label: String,
    /// The direction a tracking or polar path runs from its base, to draw.
    pub path_from: Option<Point2>,
}

/// The point the cursor gives from `base` under the settings: the angle lock
/// (else ortho, else polar within `aperture` of a multiple of the increment)
/// fixes the direction, and the length lock the distance. With no base only
/// the cursor itself.
pub fn constrain(
    base: Option<Point2>,
    cursor: Point2,
    settings: &DraftingSettings,
    aperture: f64,
) -> ConstrainedPoint {
    let mut out = ConstrainedPoint {
        point: cursor,
        label: String::new(),
        path_from: None,
    };
    let Some(base) = base else {
        return out;
    };
    let offset = cursor - base;
    let mut label = String::new();
    if let Some(lock) = settings.angle_lock {
        let u = Vec2::new(lock.cos(), lock.sin());
        out.point = base + u * offset.dot(u);
        label = format!("Angle lock {}", format_dms(lock * RAD_TO_DEG, 0));
        out.path_from = Some(base);
    } else if settings.ortho {
        out.point = if offset.x.abs() >= offset.y.abs() {
            Point2::new(cursor.x, base.y)
        } else {
            Point2::new(base.x, cursor.y)
        };
        label = String::from("Ortho");
    } else if settings.polar && settings.polar_increment > 0.0 && offset.length() > 0.0 {
        let angle = offset.angle();
        let snapped = (angle / settings.polar_increment).round() * settings.polar_increment;
        let u = Vec2::new(snapped.cos(), snapped.sin());
        let along = offset.dot(u);
        let foot = base + u * along;
        if along > 0.0 && foot.distance_to(cursor) <= aperture {
            out.point = foot;
            let mut degrees = (snapped * RAD_TO_DEG) % 360.0;
            if degrees < 0.0 {
                degrees += 360.0;
            }
            label = format!("Polar {}", format_dms(degrees, 0));
            out.path_from = Some(base);
        }
    }
    if let Some(length) = settings.length_lock {
        let mut direction = out.point - base;
        if !(direction.length() > tolerance::GEOMETRIC) {
            direction = match settings.angle_lock {
                Some(lock) => Vec2::new(lock.cos(), lock.sin()),
                None => Vec2::new(1.0, 0.0),
            };
        }
        out.point = base + direction.normalized() * length;
        if !label.is_empty() {
            label.push_str(", ");
        }
        label.push_str("Length lock ");
        label.push_str(&format_exact_real(length));
    }
    out.label = label;
    out
}

/// Object snap tracking: the point on a horizontal or vertical path through
/// one of `acquired` (or where two such paths cross) within `aperture` of the
/// cursor, nearest first; None when the cursor is on none.
pub fn track_acquired(
    acquired: &[Point2],
    cursor: Point2,
    aperture: f64,
) -> Option<ConstrainedPoint> {
    let mut best: Option<ConstrainedPoint> = None;
    let mut best_distance = f64::INFINITY;
    let mut best_is_crossing = false;
    let mut offer = |p: Point2, from: Point2, crossing: bool| {
        let d = p.distance_to(cursor);
        if d > aperture {
            return;
        }
        // A crossing of two paths is the more particular answer.
        if (crossing && !best_is_crossing) || (crossing == best_is_crossing && d < best_distance) {
            best = Some(ConstrainedPoint {
                point: p,
                label: String::from(if crossing {
                    "Tracking crossing"
                } else {
                    "Tracking"
                }),
                path_from: Some(from),
            });
            best_distance = d;
            best_is_crossing = crossing;
        }
    };
    for (i, &a) in acquired.iter().enumerate() {
        if a.distance_to(cursor) <= aperture {
            continue; // on the acquired point itself: the snap's, not a path's
        }
        offer(Point2::new(cursor.x, a.y), a, false);
        offer(Point2::new(a.x, cursor.y), a, false);
        for (j, other) in acquired.iter().enumerate() {
            if j != i {
                offer(Point2::new(a.x, other.y), a, true);
            }
        }
    }
    best
}

/// The points object snap tracking has acquired: each point-like object snap
/// the cursor lands on (an end, a middle, a centre, a crossing, a node, a
/// quadrant) while tracking is on, most recent first, at most MOST of them -
/// AutoCAD's limit, and as many paths as a user can follow. Landing on an
/// acquired point again moves it to the front. The view keeps one and clears
/// it when no tool wants a point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackingPoints {
    points: Vec<Point2>,
}

impl TrackingPoints {
    pub const MOST: usize = 7;

    /// True when `mode` is a snap whose point is worth tracking from: not the
    /// along-a-line snaps (Nearest, Perpendicular, Tangent, Extension,
    /// Parallel) nor the grid.
    pub fn tracks(mode: SnapMode) -> bool {
        matches!(
            mode,
            SnapMode::Endpoint
                | SnapMode::Midpoint
                | SnapMode::Center
                | SnapMode::Intersection
                | SnapMode::Quadrant
                | SnapMode::Node
                | SnapMode::ApparentIntersection
        )
    }

    pub fn acquire(&mut self, point: Point2, tolerance: f64) {
        self.points.retain(|p| !(p.distance_to(point) <= tolerance));
        self.points.insert(0, point);
        self.points.truncate(Self::MOST);
    }

    pub fn clear(&mut self) {
        self.points.clear();
    }

    pub fn points(&self) -> &[Point2] {
        &self.points
    }
}

// ---- one-shot snaps ------------------------------------------------------------------------------

/// From: the base point plus a typed offset ("@3,4", "@5<30").
pub fn from_base(base: Point2, offset: &str, settings: &DraftingSettings) -> Result<Point2> {
    let mut text = shown(trimmed(offset.as_bytes()));
    if !text.starts_with('@') {
        text.insert(0, '@'); // an offset is relative to the base
    }
    Ok(parse_precise_point(&text, Some(base), settings)?.point)
}

/// Midpoint between two points.
pub fn mid_between(a: Point2, b: Point2) -> Point2 {
    (a + b) * 0.5
}
