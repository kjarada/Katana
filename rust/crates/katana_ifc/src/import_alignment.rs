//! IFC 4.3 alignment business logic -> Katana's PI and PVI definitions, and
//! the exact geometry of what has no such definition. Internal to katana_ifc;
//! import.rs reads the segments out of the file (in project coordinates,
//! metres and radians) and hands them here.

use katana_geometry::alignment::{AlignmentPi, HorizontalAlignment, SolvedAlignment};
use katana_geometry::profile::{ProfilePvi, SolvedProfile, VerticalAlignment};
use katana_geometry::spiral2::Spiral2;
use katana_math::numerics::{PI, TWO_PI};
use katana_math::{Vec2, cpp};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HorizontalSegment {
    /// IfcAlignmentHorizontalSegmentTypeEnum: "LINE", "CLOTHOID" ...
    pub(crate) segment_type: String,
    pub(crate) start: Vec2,
    /// radians counter-clockwise from +x
    pub(crate) direction: f64,
    /// signed, positive turning left; 1 / StartRadiusOfCurvature
    pub(crate) start_curvature: f64,
    pub(crate) end_curvature: f64,
    pub(crate) length: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VerticalSegment {
    /// IfcAlignmentVerticalSegmentTypeEnum
    pub(crate) segment_type: String,
    /// along the horizontal, from its start
    pub(crate) start_distance: f64,
    /// horizontal length
    pub(crate) length: f64,
    pub(crate) start_height: f64,
    pub(crate) start_grade: f64,
    pub(crate) end_grade: f64,
}

/// Below this a segment has no length (the closing segments IFC 4.3 ends
/// each layout with) and a direction or curvature no difference.
const NO_LENGTH: f64 = 1e-9;
const SAME_DIRECTION: f64 = 1e-9;

fn same_curvature(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * cpp::max(a.abs(), b.abs()) + 1e-12
}

fn unit(direction: f64) -> Vec2 {
    Vec2::new(direction.cos(), direction.sin())
}

fn perp_dot(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn non_zero(segments: &[HorizontalSegment]) -> Vec<HorizontalSegment> {
    segments
        .iter()
        .filter(|segment| segment.length > NO_LENGTH)
        .cloned()
        .collect()
}

/// The curvature of a transition at fraction u of its length, by the
/// definitions of the IFC 4.3 transition types. The cubic parabola is not a
/// function of arc length; its curvature is close to the clothoid's over the
/// short, flat transitions it is used for, and is taken as that. The Viennese
/// bend needs the cant it is designed with, which is not read; it is taken as
/// the Bloss curve its curvature follows away from the cant.
fn transition_curvature(segment_type: &str, k0: f64, k1: f64, u: f64) -> f64 {
    let d = k1 - k0;
    match segment_type {
        "BLOSSCURVE" | "VIENNESEBEND" => k0 + d * u * u * (3.0 - 2.0 * u),
        "COSINECURVE" => k0 + d * (1.0 - (PI * u).cos()) / 2.0,
        "SINECURVE" => k0 + d * (u - (TWO_PI * u).sin() / TWO_PI),
        "HELMERTCURVE" => {
            k0 + d * if u < 0.5 {
                2.0 * u * u
            } else {
                1.0 - 2.0 * (1.0 - u) * (1.0 - u)
            }
        }
        _ => k0 + d * u, // CLOTHOID, CUBIC
    }
}

fn pi_at(point: Vec2) -> AlignmentPi {
    AlignmentPi {
        point,
        radius: 0.0,
        spiral_in: 0.0,
        spiral_out: 0.0,
    }
}

/// The PI definition whose solution is these segments, or None with `why`
/// said. Only LINE, CIRCULARARC and CLOTHOID have a PI form in Katana;
/// zero-length segments are passed over. The caller checks the result by
/// solving it (check_horizontal).
pub(crate) fn reconstruct_horizontal(
    all: &[HorizontalSegment],
    start_station: f64,
    why: &mut String,
) -> Option<HorizontalAlignment> {
    let segments = non_zero(all);
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        *why = String::from("it has no segment with a length");
        return None;
    };
    for segment in &segments {
        if segment.segment_type != "LINE"
            && segment.segment_type != "CIRCULARARC"
            && segment.segment_type != "CLOTHOID"
        {
            *why = format!(
                "it has a {} transition, which Katana does not have (it has the clothoid)",
                segment.segment_type
            );
            return None;
        }
    }
    if first.segment_type != "LINE" || last.segment_type != "LINE" {
        *why = format!(
            "it {} on a curve, which has no point of intersection",
            if first.segment_type != "LINE" {
                "starts"
            } else {
                "ends"
            }
        );
        return None;
    }

    let mut definition = HorizontalAlignment {
        start_station,
        pis: vec![pi_at(first.start)],
    };
    let mut i = 0;
    while i + 1 < segments.len() {
        let mut j = i + 1;
        while segments[j].segment_type != "LINE" {
            j += 1;
        }
        let before = &segments[i];
        let after = &segments[j];
        let curve = &segments[i + 1..j];
        let end = before.start + unit(before.direction) * before.length;
        let mut pi = AlignmentPi::default();
        if curve.is_empty() {
            // Two tangents meeting: a kink where they turn, nothing where they
            // run on.
            if cpp::remainder(after.direction - before.direction, TWO_PI).abs() > SAME_DIRECTION {
                pi.point = end;
                definition.pis.push(pi);
            }
            i = j;
            continue;
        }
        // [CLOTHOID] CIRCULARARC [CLOTHOID]: a spiral in from the straight to
        // the arc's curvature, the arc, a spiral out back to the straight.
        let mut arc = 0;
        while arc < curve.len() && curve[arc].segment_type != "CIRCULARARC" {
            arc += 1;
        }
        let shape = arc < curve.len()
            && arc <= 1
            && curve.len() - arc <= 2
            && (arc == 0 || curve[0].segment_type == "CLOTHOID")
            && (arc + 1 == curve.len() || curve[arc + 1].segment_type == "CLOTHOID");
        if !shape {
            *why = format!(
                "the curve between its tangents {} and {} is not a spiral, an arc and a spiral (a \
                 compound or reverse curve has no single point of intersection)",
                i + 1,
                j + 1
            );
            return None;
        }
        let k = curve[arc].start_curvature;
        if k == 0.0 || !same_curvature(k, curve[arc].end_curvature) {
            *why = String::from("an arc's radius changes along it");
            return None;
        }
        if arc == 1
            && !(same_curvature(curve[0].start_curvature, 0.0)
                && same_curvature(curve[0].end_curvature, k))
        {
            *why = String::from("a spiral does not run from the straight to its arc's radius");
            return None;
        }
        if arc + 1 < curve.len()
            && !(same_curvature(curve[arc + 1].start_curvature, k)
                && same_curvature(curve[arc + 1].end_curvature, 0.0))
        {
            *why = String::from("a spiral does not run from its arc's radius to the straight");
            return None;
        }
        let u = unit(before.direction);
        let v = unit(after.direction);
        let denominator = perp_dot(u, v);
        if denominator.abs() < 1e-12 {
            *why = String::from("two tangents either side of a curve are parallel");
            return None;
        }
        let t = perp_dot(after.start - end, v) / denominator;
        pi.point = end + u * t;
        pi.radius = 1.0 / k.abs();
        pi.spiral_in = if arc == 1 { curve[0].length } else { 0.0 };
        pi.spiral_out = if arc + 1 < curve.len() {
            curve[arc + 1].length
        } else {
            0.0
        };
        definition.pis.push(pi);
        i = j;
    }
    definition
        .pis
        .push(pi_at(last.start + unit(last.direction) * last.length));
    if definition.pis.len() < 2 {
        *why = String::from("it has fewer than two points of intersection");
        return None;
    }
    Some(definition)
}

/// The largest distance between where a segment of the file starts and where
/// the solved definition puts that station.
pub(crate) fn check_horizontal(solved: &SolvedAlignment, segments: &[HorizontalSegment]) -> f64 {
    let mut worst: f64 = 0.0;
    let mut distance = 0.0;
    for segment in segments {
        let station = cpp::clamp(
            solved.start_station() + distance,
            solved.start_station(),
            solved.end_station(),
        );
        if let Some(at) = solved.point_at_station(station) {
            worst = cpp::max(worst, (at - segment.start).length());
        }
        distance += segment.length;
    }
    cpp::max(worst, (solved.length() - distance).abs())
}

/// The PVI definition whose solution is these segments (CONSTANTGRADIENT and
/// PARABOLICARC only), stations counted from `start_station` at distance 0.
pub(crate) fn reconstruct_vertical(
    all: &[VerticalSegment],
    start_station: f64,
    why: &mut String,
) -> Option<VerticalAlignment> {
    let segments: Vec<VerticalSegment> = all
        .iter()
        .filter(|segment| segment.length > NO_LENGTH)
        .cloned()
        .collect();
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        *why = String::from("it has no segment with a length");
        return None;
    };
    for segment in &segments {
        if segment.segment_type != "CONSTANTGRADIENT" && segment.segment_type != "PARABOLICARC" {
            *why = format!(
                "it has a {} vertical curve, and Katana designs vertical curves as parabolas",
                segment.segment_type
            );
            return None;
        }
    }
    let end_height =
        |s: &VerticalSegment| s.start_height + (s.start_grade + s.end_grade) / 2.0 * s.length;
    for (k, pair) in segments.windows(2).enumerate() {
        let a = &pair[0];
        let b = &pair[1];
        if (a.start_distance + a.length - b.start_distance).abs() > 1e-3
            || (end_height(a) - b.start_height).abs() > 1e-3
        {
            *why = format!("its segments {} and {} do not meet", k + 1, k + 2);
            return None;
        }
    }
    let mut definition = VerticalAlignment {
        pvis: vec![ProfilePvi {
            station: start_station + first.start_distance,
            elevation: first.start_height,
            curve_length: 0.0,
        }],
    };
    for (k, s) in segments.iter().enumerate() {
        if s.segment_type == "PARABOLICARC" {
            definition.pvis.push(ProfilePvi {
                station: start_station + s.start_distance + s.length / 2.0,
                elevation: s.start_height + s.start_grade * s.length / 2.0,
                curve_length: s.length,
            });
        } else if let Some(next) = segments.get(k + 1)
            && next.segment_type == "CONSTANTGRADIENT"
            && (next.start_grade - s.start_grade).abs() > 1e-12
        {
            definition.pvis.push(ProfilePvi {
                station: start_station + s.start_distance + s.length,
                elevation: end_height(s),
                curve_length: 0.0,
            });
        }
    }
    definition.pvis.push(ProfilePvi {
        station: start_station + last.start_distance + last.length,
        elevation: end_height(last),
        curve_length: 0.0,
    });
    Some(definition)
}

pub(crate) fn check_vertical(
    solved: &SolvedProfile,
    segments: &[VerticalSegment],
    start_station: f64,
) -> f64 {
    let mut worst: f64 = 0.0;
    for segment in segments {
        for along in [0.0, segment.length / 2.0, segment.length] {
            let station = start_station + segment.start_distance + along;
            let expected = height_at(segments, segment.start_distance + along);
            let actual = solved.elevation_at(cpp::clamp(
                station,
                solved.start_station(),
                solved.end_station(),
            ));
            if let (Some(expected), Some(actual)) = (expected, actual) {
                worst = cpp::max(worst, (expected - actual).abs());
            }
        }
    }
    worst
}

/// The horizontal as points no further than `tolerance` from the curve, each
/// with its distance along; transitions other than the clothoid are
/// integrated from their curvature, which is what defines them.
pub(crate) fn sample_horizontal(all: &[HorizontalSegment], tolerance: f64) -> Vec<(f64, Vec2)> {
    let mut out: Vec<(f64, Vec2)> = Vec::new();
    let mut distance = 0.0;
    let add = |out: &mut Vec<(f64, Vec2)>, along: f64, at: Vec2| {
        if let Some((_, last)) = out.last()
            && (*last - at).length() < 1e-9
        {
            return;
        }
        out.push((along, at));
    };
    let segments = non_zero(all);
    for segment in &segments {
        let k0 = segment.start_curvature;
        let k1 = segment.end_curvature;
        let k_max = cpp::max(k0.abs(), k1.abs());
        // Chords from the sagitta rule at the tightest curvature (as
        // geometry's chording does): a chord of length c stands off a circle
        // of curvature k by about k c^2 / 8.
        let mut count: usize = 1;
        if k_max > 0.0 {
            let chord = (8.0 * tolerance / k_max).sqrt();
            // The C++ casts the ceiling unchecked; Rust saturates an infinite
            // or NaN one.
            count = 1_usize.max((segment.length / chord).ceil() as usize);
        }
        count = count.min(100_000);
        if segment.segment_type == "LINE" {
            add(&mut out, distance, segment.start);
        } else if segment.segment_type == "CIRCULARARC" || segment.segment_type == "CLOTHOID" {
            let exact = Spiral2 {
                start: segment.start,
                start_direction: segment.direction,
                start_curvature: k0,
                end_curvature: k1,
                length: segment.length,
            };
            for n in 0..count {
                let s = segment.length * n as f64 / count as f64;
                add(&mut out, distance + s, exact.point_at(s));
            }
        } else {
            // Direction is the integral of curvature, position the integral
            // of direction: Simpson's rule on a fine grid, eight steps to
            // each chord written.
            let steps = count * 8;
            let h = segment.length / steps as f64;
            let mut at = segment.start;
            let mut direction = segment.direction;
            let curvature =
                |s: f64| transition_curvature(&segment.segment_type, k0, k1, s / segment.length);
            for n in 0..steps {
                if n.is_multiple_of(8) {
                    add(&mut out, distance + h * n as f64, at);
                }
                let s = h * n as f64;
                let d_mid = direction
                    + h / 12.0
                        * (curvature(s) + 4.0 * curvature(s + h / 4.0) + curvature(s + h / 2.0));
                let d_end = direction
                    + h / 6.0 * (curvature(s) + 4.0 * curvature(s + h / 2.0) + curvature(s + h));
                at += (unit(direction) + unit(d_mid) * 4.0 + unit(d_end)) * (h / 6.0);
                direction = d_end;
            }
        }
        distance += segment.length;
    }
    if let Some(last) = segments.last() {
        if last.segment_type == "LINE" {
            add(
                &mut out,
                distance,
                last.start + unit(last.direction) * last.length,
            );
        } else if last.segment_type == "CIRCULARARC" || last.segment_type == "CLOTHOID" {
            let exact = Spiral2 {
                start: last.start,
                start_direction: last.direction,
                start_curvature: last.start_curvature,
                end_curvature: last.end_curvature,
                length: last.length,
            };
            add(&mut out, distance, exact.end_point());
        }
    }
    out
}

/// The profile's height at `distance` along the horizontal; None outside it.
pub(crate) fn height_at(segments: &[VerticalSegment], distance: f64) -> Option<f64> {
    for s in segments {
        if s.length <= NO_LENGTH
            || distance < s.start_distance - 1e-9
            || distance > s.start_distance + s.length + 1e-9
        {
            continue;
        }
        let x = cpp::clamp(distance - s.start_distance, 0.0, s.length);
        if s.segment_type == "CIRCULARARC" {
            // Slope angle a from atan g1 to atan g2: sin a changes by x / R'
            // over a horizontal x, with R' = L / (sin a2 - sin a1).
            let a1 = s.start_grade.atan();
            let a2 = s.end_grade.atan();
            if (a2.sin() - a1.sin()).abs() > 1e-12 {
                let r = s.length / (a2.sin() - a1.sin());
                let sa = cpp::clamp(a1.sin() + x / r, -1.0, 1.0);
                return Some(s.start_height + r * (a1.cos() - (1.0 - sa * sa).sqrt()));
            }
        }
        // CONSTANTGRADIENT, PARABOLICARC; a vertical CLOTHOID taken as the
        // parabola with the same grades.
        return Some(
            s.start_height
                + s.start_grade * x
                + (s.end_grade - s.start_grade) / (2.0 * s.length) * x * x,
        );
    }
    None
}
