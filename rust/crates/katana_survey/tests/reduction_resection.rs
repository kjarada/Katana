//! Twin of tests/survey/test_reduction_resection.cpp: the resection of a free
//! station, a setup on a point nothing else positions, computed from its
//! pointings to points already placed (resect_setup, reduction_adjust.rs; tried
//! from place_setups, reduction.rs).
//!
//! Most cases share one free station R, really at N 1040, E 1030, H 52.345,
//! instrument height 1.550, its circle turned so that azimuth = reading + 20
//! degrees, and three marks 50 m from it, each at H 50.000 with a 1.700 target:
//!
//!   A  N 1000, E 1000   azimuth 180 + atan(3/4) = 216.869897646 deg, reading 196.869897646
//!   B  N 1000, E 1060   azimuth 180 - atan(3/4) = 143.130102354 deg, reading 123.130102354
//!   C  N 1090, E 1030   azimuth 0,                                   reading 340
//!
//! Each is observed on both faces with the zenith and slope distance that put
//! it 50 m out and 2.345 m below the mark: the line of sight falls
//! 50 - 52.345 - 1.550 + 1.700 = -2.195 m, so z = atan2(50, -2.195) and
//! S = hypot(50, 2.195), and S sin z = 50, S cos z + 1.550 - 1.700 = -2.345.

#![allow(
    clippy::excessive_precision,
    reason = "The expected values are the C++ tests' literals, digit for digit."
)]

mod reduction_support;
mod survey_support;

use katana_math::numerics::normalize_angle;
use katana_survey::data_model::SurveyProject;
use katana_survey::reduction::ReductionOutcome;
use reduction_support::{Shot, deg, point, setup, shot, unpositioned};

const STATION_NORTHING: f64 = 1040.0;
const STATION_EASTING: f64 = 1030.0;
const STATION_HEIGHT: f64 = 52.345;
const INSTRUMENT_HEIGHT: f64 = 1.550;
const TARGET_HEIGHT: f64 = 1.700;

#[derive(Clone, Copy, Debug)]
struct Mark {
    id: &'static str,
    northing: f64,
    easting: f64,
}

const A: Mark = Mark {
    id: "A",
    northing: 1000.0,
    easting: 1000.0,
};
const B: Mark = Mark {
    id: "B",
    northing: 1000.0,
    easting: 1060.0,
};
const C: Mark = Mark {
    id: "C",
    northing: 1090.0,
    easting: 1030.0,
};

fn mark(id: &'static str, northing: f64, easting: f64) -> Mark {
    Mark {
        id,
        northing,
        easting,
    }
}

fn azimuth_from(northing: f64, easting: f64, mark: &Mark) -> f64 {
    normalize_angle((mark.easting - easting).atan2(mark.northing - northing))
}

// Both faces of one pointing to `target`: the face-right reading half a turn
// on, the zenith as the model stores it (face-left equivalent).
fn both_faces(
    shots: &mut Vec<Shot>,
    target: &str,
    index: &mut usize,
    reading: Option<f64>,
    zenith: Option<f64>,
    slope: Option<f64>,
    target_height: f64,
) {
    use katana_survey::data_model::Face;
    shots.push(shot(
        target,
        *index,
        Face::Left,
        reading,
        zenith,
        slope,
        target_height,
    ));
    *index += 1;
    shots.push(shot(
        target,
        *index,
        Face::Right,
        reading.map(|reading| normalize_angle(reading + deg(180.0))),
        zenith,
        slope,
        target_height,
    ));
    *index += 1;
}

// What R observes of `mark`: the reading for the circle's 20 degrees, and
// (when `distance`) the zenith and slope distance of a line `horizontal` long
// that falls 2.195 m. observe(shots, index, mark, distance) of the C++ is
// observe(.., 50.0, 0.0) here.
fn observe(
    shots: &mut Vec<Shot>,
    index: &mut usize,
    mark: &Mark,
    distance: bool,
    horizontal: f64,
    reading_error: f64,
) {
    let reading = normalize_angle(
        azimuth_from(STATION_NORTHING, STATION_EASTING, mark) - deg(20.0) + reading_error,
    );
    let fall = -2.195;
    both_faces(
        shots,
        mark.id,
        index,
        Some(reading),
        distance.then(|| horizontal.atan2(fall)),
        distance.then(|| horizontal.hypot(fall)),
        TARGET_HEIGHT,
    );
}

// What a station at (n, e) observes of `mark` on both faces: the reading for a
// circle turned by `turn`, plus `reading_error`, and (when `distance`) a level
// sight (zenith 90, the target at the instrument's height, so the mark and the
// station share a height) `horizontal` long, the true length unless given.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ helper's arguments, its defaults written out."
)]
fn level(
    shots: &mut Vec<Shot>,
    index: &mut usize,
    n: f64,
    e: f64,
    mark: &Mark,
    turn: f64,
    distance: bool,
    reading_error: f64,
    horizontal: Option<f64>,
) {
    let reading = normalize_angle(azimuth_from(n, e, mark) - turn + reading_error);
    let length = horizontal.unwrap_or((mark.northing - n).hypot(mark.easting - e));
    both_faces(
        shots,
        mark.id,
        index,
        Some(reading),
        distance.then(|| deg(90.0)),
        distance.then_some(length),
        INSTRUMENT_HEIGHT,
    );
}

// The marks entered with coordinates; R and Q named only.
fn marks_and(marks: &[Mark], shots: &[Shot]) -> SurveyProject {
    let mut project = SurveyProject::default();
    for mark in marks {
        project
            .points
            .push(point(mark.id, mark.northing, mark.easting, Some(50.0)));
    }
    project.unpositioned_points.push(unpositioned("R"));
    project.unpositioned_points.push(unpositioned("Q"));
    project
        .stations
        .push(setup("S1", "R", INSTRUMENT_HEIGHT, "", shots));
    project
}

// Q from R: reading 0 (azimuth 20), 10 m level, target at the instrument's
// height: at N 1040 + 10 cos 20 = 1049.396926208, E 1030 + 10 sin 20 =
// 1033.420201433, and R's height.
fn shoot_q(shots: &mut Vec<Shot>, index: &mut usize) {
    both_faces(
        shots,
        "Q",
        index,
        Some(0.0),
        Some(deg(90.0)),
        Some(10.0),
        INSTRUMENT_HEIGHT,
    );
}

// Nothing of a refused resection's least squares is left: no warning of a
// flagged residual, no observation marked rejected.
fn expect_nothing_left_behind(outcome: &ReductionOutcome) {
    assert_eq!(
        reduction_support::warnings_with(&outcome.report, "flagged by"),
        0,
        "{}",
        reduction_support::all_warnings(&outcome.report)
    );
    for row in &outcome.report.observations {
        assert!(
            !row.rejected,
            "{} {}: {}",
            row.kind, row.to, row.rejection_reason
        );
    }
}

mod reduction_resection {
    use super::*;
    use crate::reduction_support::{
        all_warnings, arc_seconds, bare_settings, find_point, find_row, point_from, succeeded,
        warnings_with,
    };
    use crate::survey_support::near;
    use katana_math::numerics::normalize_angle_signed;
    use katana_survey::data_model::{
        ControlPoint, CoordinateSource, DistanceKind, Face, RESECTION_END_METADATA,
    };
    use katana_survey::reduction::{ReductionContext, reduce_and_adjust};
    use katana_survey::reduction_report::{ComputationMethod, MisclosureReport, ReportResidual};
    use katana_survey::reduction_settings::{
        AdjustmentMethod, ControlOrigin, ControlSelection, GridScale, HeightReduction,
        ReductionSettings,
    };

    fn reduce(project: &SurveyProject, settings: &ReductionSettings) -> ReductionOutcome {
        succeeded(reduce_and_adjust(
            project,
            settings,
            &ReductionContext::default(),
        ))
    }

    fn held(id: &str) -> ControlSelection {
        ControlSelection {
            point: ControlPoint::fixed3d(id),
            origin: ControlOrigin::File,
        }
    }

    #[test]
    fn two_placed_points_with_distances_position_and_orient_a_station_nothing_else_places() {
        // Two directions and two distances for north, east and the orientation:
        // R, its 20 degrees and Q come back as constructed. The heights are
        // 50 - (-2.345) = 52.345 from each (no curvature in these settings).
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        shoot_q(&mut shots, &mut index);
        let outcome = reduce(&marks_and(&[A, B], &shots), &bare_settings());
        let report = &outcome.report;

        let r = find_point(&outcome, "R").unwrap_or_else(|| panic!("{}", all_warnings(report)));
        assert_eq!(r.method, ComputationMethod::Resection);
        near(r.northing, STATION_NORTHING, 1e-8);
        near(r.easting, STATION_EASTING, 1e-8);
        near(r.elevation.expect("a height"), STATION_HEIGHT, 1e-9);
        near(
            report.setups[0]
                .orientation_correction
                .expect("an orientation"),
            deg(20.0),
            1e-10,
        );

        assert_eq!(report.resections.len(), 1);
        let resection = &report.resections[0];
        assert_eq!(resection.station_id, "S1");
        assert_eq!(resection.point_id, "R");
        assert_eq!(
            resection.targets,
            vec![String::from("A"), String::from("B")]
        );
        near(resection.orientation, deg(20.0), 1e-10);
        assert_eq!(resection.horizontal.redundancy, 1); // 4 observations, 3 unknowns
        assert_eq!(resection.horizontal.residuals.len(), 4);
        let height = resection.height.as_ref().expect("a height adjustment");
        assert_eq!(height.redundancy, 1); // two heights of one unknown

        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1049.396926208, 1e-8);
        near(q.easting, 1033.420201433, 1e-8);
        near(q.elevation.expect("a height"), STATION_HEIGHT, 1e-9);

        // Its targets were used, so they are not also checks of it.
        assert!(report.misclosures.is_empty());
        assert_eq!(
            warnings_with(report, "has no position"),
            0,
            "{}",
            all_warnings(report)
        );
    }

    #[test]
    fn three_directions_alone_resect_a_station_off_the_circle_through_them() {
        // Three readings for three unknowns: R is the centre of the circle
        // through A, B and C (all 50 m away), as far from the danger circle as
        // it gets. Nothing checks it, and with no distance there is no height.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, false, 50.0, 0.0);
        shoot_q(&mut shots, &mut index);
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &bare_settings());
        let report = &outcome.report;

        let r = find_point(&outcome, "R").unwrap_or_else(|| panic!("{}", all_warnings(report)));
        near(r.northing, STATION_NORTHING, 1e-8);
        near(r.easting, STATION_EASTING, 1e-8);
        assert!(r.elevation.is_none());
        near(
            report.setups[0].orientation_correction.unwrap(),
            deg(20.0),
            1e-10,
        );
        assert_eq!(report.resections.len(), 1);
        assert_eq!(report.resections[0].horizontal.redundancy, 0);
        assert!(report.resections[0].height.is_none());
        assert_eq!(
            warnings_with(report, "its resection has no redundancy"),
            1,
            "{}",
            all_warnings(report)
        );
        assert_eq!(
            warnings_with(report, "resected with no height"),
            1,
            "{}",
            all_warnings(report)
        );

        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1049.396926208, 1e-8);
        near(q.easting, 1033.420201433, 1e-8);
    }

    #[test]
    fn residuals_and_precision_match_an_independent_least_squares() {
        // A, B and C with directions and distances, the distance to A 4 mm long
        // and the reading to B 6" high on both faces. Worked apart from this
        // code by a scratch script of the textbook model (Ghilani, Adjustment
        // Computations, ch. 15): unknowns N, E and the orientation w; readings
        // atan2(dE, dN) - w weighted 1 / (s^2 + (c / D)^2) with s = 3" / sqrt 2
        // (a face pair) and c = 1 mm, the TARGET's centring (the instrument is
        // the unknown, so its own centring is common to every pointing) over
        // D = 50 m (50.004 to A); distances weighted 1 / (hypot(2 mm, 2 ppm S)^2
        // + c^2); its own normal equations, inverted by cofactors and iterated
        // to 1e-13 m:
        //   N 1040.001297101, E 1030.000631310, w 19.999589125 deg
        //   residuals (computed - observed): reading A +0.3521", distance A
        //   -2.5835 mm, reading B +0.7731", distance B +0.6589 mm, reading C
        //   -1.1253", distance C -1.2971 mm; v'Pv 1.847118723 on 3 degrees of
        //   freedom, variance factor 0.615706241;
        //   a priori sN 0.987965 mm, sE 0.734805 mm, sw 2.745902".
        // The global test passes (1.847 is inside chi-square(3) 0.216 to 9.348),
        // so the variance factor (0.62 on 3 degrees of freedom, itself uncertain
        // by 82 %) is not applied: the precision is the a-priori one. R stands
        // over a mark (instrument height 1.55), so its precision adds the
        // instrument's 1 mm centring: sN hypot(0.987965, 1) = 1.405729 mm, sE
        // hypot(0.734805, 1) = 1.240942 mm. (The round before scaled by the
        // variance factor: 1.2653 and 1.1543 mm.)
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.004, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(6.0));
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &bare_settings());
        assert_eq!(outcome.report.resections.len(), 1);
        let resection = &outcome.report.resections[0];

        near(resection.northing, 1040.001297101, 1e-8);
        near(resection.easting, 1030.000631310, 1e-8);
        near(resection.orientation, deg(19.999589125), 1e-10);
        let horizontal = &resection.horizontal;
        assert_eq!(horizontal.redundancy, 3);
        near(
            horizontal.variance_factor.expect("a variance factor"),
            0.615706241,
            1e-8,
        );
        assert_eq!(horizontal.residuals.len(), 6);
        let expected = [
            arc_seconds(0.3521),
            -0.0025835,
            arc_seconds(0.7731),
            0.0006589,
            arc_seconds(-1.1253),
            -0.0012971,
        ];
        let labels = [
            "direction at S1: A",
            "distance at S1: A",
            "direction at S1: B",
            "distance at S1: B",
            "direction at S1: C",
            "distance at S1: C",
        ];
        for (i, residual) in horizontal.residuals.iter().enumerate() {
            assert_eq!(residual.observation, labels[i]);
            assert_eq!(residual.angular, i % 2 == 0, "{}", labels[i]);
            let tolerance = if residual.angular {
                arc_seconds(0.0001)
            } else {
                1e-7
            };
            assert!(
                (residual.residual - expected[i]).abs() <= tolerance,
                "{}: {} against {}",
                labels[i],
                residual.residual,
                expected[i]
            );
        }
        let global_test = horizontal.global_test.expect("a global test");
        assert!(global_test.passed);
        near(resection.sigma_northing, 0.001405729, 1e-9);
        near(resection.sigma_easting, 0.001240942, 1e-9);
        near(
            resection.sigma_orientation,
            arc_seconds(2.745902),
            arc_seconds(0.000001),
        );

        // The station carries that precision onto the drawing.
        let r = find_point(&outcome, "R").expect("R");
        near(r.sigma_northing.expect("a precision"), 0.001405729, 1e-9);
        near(r.sigma_easting.expect("a precision"), 0.001240942, 1e-9);
    }

    #[test]
    fn the_height_is_the_weighted_mean_of_trigonometric_heights_with_curvature() {
        // The default settings: curvature and refraction on, k = 0.13,
        // R = 6 371 000 m. C's mark is 6 mm higher than the others. By hand:
        //   B = (1 - k) / (2 R) = 0.87 / 12 742 000 = 6.82781353e-8
        //   Y = S sin z = 50, B Y^2 = 0.000170695338
        //   dh = S cos z + B Y^2 + HI - HT = -2.195 + 0.000170695 + 1.550 - 1.700
        //      = -2.344829305 to each mark
        //   R's height from A and from B = 50 + 2.344829305 = 52.344829305,
        //   from C 52.350829305. The three have the same zenith and distance, so
        //   the same weight: the plain mean, 52.346829305; residuals
        //   (computed - observed) -2, -2 and +4 mm. Each height difference's
        //   a-priori sigma: S = hypot(50, 2.195) = 50.048158, cos z = -2.195 / S,
        //   sigma_S = hypot(2 mm, 2 ppm S) = 2.002503 mm, sigma_z = 3" / sqrt 2,
        //   and the target height's 2 mm: sqrt((cos z sigma_S)^2 + (50 sigma_z)^2
        //   + (2 mm)^2) = 2.066915 mm. v'Pv = 24e-6 / (2.066915 mm)^2 = 5.618,
        //   inside chi-square(2)'s 0.051 to 7.378, so the global test passes and
        //   the precision is the a-priori one: a mean of three, 2.066915 / sqrt 3
        //   = 1.193334 mm. The instrument height, measured once to the settings'
        //   2 mm, is in every height difference alike, so it is in none of their
        //   weights and is added to the mark's height after: sigma hypot(1.193334,
        //   2) = 2.328958 mm. (The round before scaled by the variance factor:
        //   the a posteriori 2 mm, and hypot(2, 2) = 2.828427 mm.)
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        let mut project = marks_and(&[A, B, C], &shots);
        project.points[2].elevation = Some(50.006);
        let outcome = reduce(&project, &ReductionSettings::default());
        assert_eq!(outcome.report.resections.len(), 1);
        let resection = &outcome.report.resections[0];
        near(resection.elevation.expect("a height"), 52.346829305, 1e-9);
        near(
            resection.sigma_elevation.expect("a precision"),
            0.002328958,
            1e-9,
        );
        let height = resection.height.as_ref().expect("a height adjustment");
        assert_eq!(height.residuals.len(), 3);
        near(height.residuals[0].residual, -0.002, 1e-9);
        near(height.residuals[1].residual, -0.002, 1e-9);
        near(height.residuals[2].residual, 0.004, 1e-9);
        assert_eq!(
            height.residuals[2].observation,
            "height difference at S1: C"
        );

        // The curvature also shortens each 50 m by A X Y = (1 - k/2) / R *
        // 2.195 * 50 = 0.016 mm, alike on all three: R moves by less than that.
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, STATION_NORTHING, 5e-5);
        near(r.easting, STATION_EASTING, 5e-5);
        near(r.elevation.unwrap(), 52.346829305, 1e-9);
    }

    #[test]
    fn too_few_placed_points_are_refused_saying_what_a_resection_needs() {
        // Two readings fix neither the station nor its orientation.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, false, 50.0, 0.0);
        shoot_q(&mut shots, &mut index);
        let outcome = reduce(&marks_and(&[A, B], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert!(find_point(&outcome, "Q").is_none());
        assert!(outcome.report.resections.is_empty());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S1 stands on R, which has no position: nothing was computed from it. It \
                 was not resected: it observes 2 placed points with a direction (A and B), 0 of \
                 them with a distance as well, where a resection needs two placed points \
                 observed with a direction and a distance, or three observed with a direction."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn points_on_one_position_count_once() {
        // D is entered at A's coordinates: three readings, but two places.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, false, 50.0, 0.0);
        let d = mark("D", A.northing, A.easting);
        let reading_to_d =
            normalize_angle(azimuth_from(STATION_NORTHING, STATION_EASTING, &d) - deg(20.0));
        both_faces(
            &mut shots,
            "D",
            &mut index,
            Some(reading_to_d),
            None,
            None,
            TARGET_HEIGHT,
        );
        let outcome = reduce(&marks_and(&[A, B, d], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "(A and D stand on one position, so count as one)"
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn marks_closer_than_their_centring_count_as_one_position() {
        // A1 1 mm east of A: less than the standard deviation of the difference
        // of two targets each centred to the settings' 1 mm (sqrt 2 x 1 mm =
        // 1.41 mm), which no pointing to them can resolve, so three readings
        // are two positions, too few, said as such rather than tried.
        let a1 = mark("A1", A.northing, A.easting + 0.001);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &a1, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, false, 50.0, 0.0);
        let outcome = reduce(&marks_and(&[A, a1, B], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "(A and A1 stand on one position, so count as one)"
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_station_on_one_line_with_its_targets_is_refused() {
        // A, B and C due south, south and north of R, on its meridian: every
        // angle between them is 0 or 180 degrees.
        let a = mark("A", 1000.0, 1030.0);
        let b = mark("B", 1020.0, 1030.0);
        let c = mark("C", 1090.0, 1030.0);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &a, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &b, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &c, false, 50.0, 0.0);
        let outcome = reduce(&marks_and(&[a, b, c], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: it stands on one line with A, B and C, where directions \
                 cannot fix it."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_station_on_the_danger_circle_is_refused() {
        // A, B and C on the circle of radius 100 about N 1000, E 1000, and R on
        // it too, at N 1000, E 900: it sees A at 45, B at 90 and C at 135
        // degrees, and so would any point of that arc: the two circles of the
        // construction are one, centre N 1000, E 1000.
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        let mut shots = Vec::new();
        let mut index = 1;
        for (target, azimuth) in [(a, 45.0), (b, 90.0), (c, 135.0)] {
            both_faces(
                &mut shots,
                target.id,
                &mut index,
                Some(deg(azimuth - 20.0)),
                None,
                None,
                TARGET_HEIGHT,
            );
        }
        let outcome = reduce(&marks_and(&[a, b, c], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: it stands on the circle through A, B and C (the danger \
                 circle), where directions to them cannot fix it."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_reading_that_is_not_a_number_is_refused_not_computed() {
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, false, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, false, 50.0, 0.0);
        both_faces(
            &mut shots,
            "C",
            &mut index,
            Some(f64::NAN),
            None,
            None,
            TARGET_HEIGHT,
        );
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: what it measured to C, or where that point is, is not a \
                 finite number."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_resection_waits_for_targets_a_later_setup_places() {
        // S1, first in the file, stands on R and observes T1 and T2, which only
        // S2 places: S2 stands on K (N 2000, E 2000, H 100), backsights L due
        // north at reading 0 (orientation 0), and radiates T1 50 m east and T2
        // 50 m south, level (HI = HT, zenith 90): T1 N 2000 E 2050, T2 N 1950
        // E 2000, both H 100. S1 is tried after S2, when nothing else is left,
        // and resects R (really N 1960, E 2060, H 100, its circle turned so
        // that azimuth = reading + 10 degrees) from T1 (azimuth atan2(-10, 40),
        // 41.231 m) and T2 (azimuth atan2(-60, -10), 60.828 m); then radiates
        // Q at reading 80 (azimuth 90), 10 m: N 1960, E 2070.
        let t1 = mark("T1", 2000.0, 2050.0);
        let t2 = mark("T2", 1950.0, 2000.0);
        let r_n = 1960.0;
        let r_e = 2060.0;
        let mut project = SurveyProject::default();
        project.points.push(point("K", 2000.0, 2000.0, Some(100.0)));
        project.points.push(point("L", 2100.0, 2000.0, Some(100.0)));
        for id in ["R", "T1", "T2", "Q"] {
            project.unpositioned_points.push(unpositioned(id));
        }
        let mut free = Vec::new();
        let mut index = 1;
        for target in [t1, t2] {
            let horizontal = (target.northing - r_n).hypot(target.easting - r_e);
            both_faces(
                &mut free,
                target.id,
                &mut index,
                Some(normalize_angle(azimuth_from(r_n, r_e, &target) - deg(10.0))),
                Some(deg(90.0)),
                Some(horizontal),
                1.5,
            );
        }
        both_faces(
            &mut free,
            "Q",
            &mut index,
            Some(deg(80.0)),
            Some(deg(90.0)),
            Some(10.0),
            1.5,
        );
        project.stations.push(setup("S1", "R", 1.5, "", &free));
        let mut known = Vec::new();
        index = 1;
        both_faces(
            &mut known,
            "L",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(100.0),
            1.5,
        );
        both_faces(
            &mut known,
            "T1",
            &mut index,
            Some(deg(90.0)),
            Some(deg(90.0)),
            Some(50.0),
            1.5,
        );
        both_faces(
            &mut known,
            "T2",
            &mut index,
            Some(deg(180.0)),
            Some(deg(90.0)),
            Some(50.0),
            1.5,
        );
        project.stations.push(setup("S2", "K", 1.5, "L", &known));

        let outcome = reduce(&project, &bare_settings());
        let r = find_point(&outcome, "R")
            .unwrap_or_else(|| panic!("{}", all_warnings(&outcome.report)));
        assert_eq!(r.method, ComputationMethod::Resection);
        near(r.northing, r_n, 1e-8);
        near(r.easting, r_e, 1e-8);
        near(r.elevation.expect("a height"), 100.0, 1e-9);
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1960.0, 1e-8);
        near(q.easting, 2070.0, 1e-8);
        assert_eq!(outcome.report.resections.len(), 1);
        assert_eq!(
            outcome.report.resections[0].targets,
            vec![String::from("T1"), String::from("T2")]
        );
    }

    #[test]
    fn a_resection_comes_before_the_files_own_coordinates_for_the_station() {
        // R's own coordinates in the file, as a controller computed them, 0.2 m
        // north: the resection from A and B places it, not the file, and the
        // file's coordinates are not dropped unseen: they are a check of the
        // resection (resection less file: -0.2 m north, 0 east, and in height
        // 52.345 - 52.345 = 0), and a warning says how far they were.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        let mut project = marks_and(&[A, B], &shots);
        project.unpositioned_points.remove(0); // R
        project.points.push(point_from(
            "R",
            STATION_NORTHING + 0.2,
            STATION_EASTING,
            Some(STATION_HEIGHT),
            CoordinateSource::Calculated,
        ));
        let outcome = reduce(&project, &bare_settings());
        let report = &outcome.report;
        let r = find_point(&outcome, "R").expect("R");
        assert_eq!(r.method, ComputationMethod::Resection);
        near(r.northing, STATION_NORTHING, 1e-8);
        assert_eq!(
            warnings_with(report, "the file's own coordinates for it were used"),
            0
        );

        assert_eq!(
            warnings_with(
                report,
                "Setup S1 stands on R: its resection from A and B places it 200.0 mm from the \
                 file's own coordinates for it, which were not used"
            ),
            1,
            "{}",
            all_warnings(report)
        );
        let check: &MisclosureReport = report
            .misclosures
            .iter()
            .rfind(|misclosure| {
                misclosure.name == "R by resection at setup S1, against the file's own coordinates"
            })
            .expect("the check");
        near(check.northing.unwrap(), -0.2, 1e-8);
        near(check.easting.unwrap(), 0.0, 1e-8);
        near(check.linear.unwrap(), 0.2, 1e-8);
        near(check.height.expect("a height check"), 0.0, 1e-9);
        assert_eq!(report.resections.len(), 1);
        near(
            report.resections[0]
                .file_northing_difference
                .expect("the file's difference"),
            -0.2,
            1e-8,
        );
        near(
            report.resections[0].file_easting_difference.unwrap(),
            0.0,
            1e-8,
        );
    }

    #[test]
    fn a_refused_resection_falls_back_on_the_files_own_coordinates_and_says_why() {
        // The danger circle again, R's coordinates in the file: they are used,
        // and the warning says why the resection was not.
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        let mut shots = Vec::new();
        let mut index = 1;
        for (target, azimuth) in [(a, 45.0), (b, 90.0), (c, 135.0)] {
            both_faces(
                &mut shots,
                target.id,
                &mut index,
                Some(deg(azimuth)),
                None,
                None,
                TARGET_HEIGHT,
            );
        }
        let mut project = marks_and(&[a, b, c], &shots);
        project.unpositioned_points.remove(0); // R
        project.points.push(point_from(
            "R",
            1000.0,
            900.0,
            Some(50.0),
            CoordinateSource::Calculated,
        ));
        let outcome = reduce(&project, &bare_settings());
        assert!(outcome.report.resections.is_empty());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S1 stands on R, which is not control and was not computed (its \
                 resection was refused: it stands on the circle through A, B and C (the danger \
                 circle), where directions to them cannot fix it); the file's own coordinates \
                 for it were used."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_station_another_setup_radiates_is_not_resected() {
        // S1 on K radiates R; S2 on R backsights K and also reads A and B with
        // distances (enough for a resection) but R already has a position, so
        // S2 is oriented on K as before and its readings to A and B are checks.
        // K is 50 m due south of R (N 990, E 1030); S1 backsights A with its
        // circle at north, S2 reads K with its circle turned 20 degrees as it
        // reads A and B.
        let mut at_k = Vec::new();
        let mut index = 1;
        let k = mark("K", 990.0, 1030.0);
        both_faces(
            &mut at_k,
            "A",
            &mut index,
            Some(azimuth_from(k.northing, k.easting, &A)),
            Some(deg(90.0)),
            Some((A.northing - k.northing).hypot(A.easting - k.easting)),
            1.5,
        );
        both_faces(
            &mut at_k,
            "R",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(50.0),
            1.5,
        );
        let mut at_r = Vec::new();
        index = 1;
        both_faces(
            &mut at_r,
            "K",
            &mut index,
            Some(normalize_angle(
                azimuth_from(STATION_NORTHING, STATION_EASTING, &k) - deg(20.0),
            )),
            Some(deg(90.0)),
            Some(50.0),
            1.5,
        );
        observe(&mut at_r, &mut index, &A, true, 50.0, 0.0);
        observe(&mut at_r, &mut index, &B, true, 50.0, 0.0);
        let mut project = SurveyProject::default();
        project
            .points
            .push(point("K", k.northing, k.easting, Some(50.0)));
        project
            .points
            .push(point("A", A.northing, A.easting, Some(50.0)));
        project
            .points
            .push(point("B", B.northing, B.easting, Some(50.0)));
        project.unpositioned_points.push(unpositioned("R"));
        project.stations.push(setup("S1", "K", 1.5, "A", &at_k));
        project.stations.push(setup("S2", "R", 1.5, "K", &at_r));
        let outcome = reduce(&project, &bare_settings());
        let radiated = find_point(&outcome, "R").expect("R");
        assert_eq!(radiated.method, ComputationMethod::Radiation);
        near(radiated.northing, STATION_NORTHING, 1e-8);
        near(radiated.easting, STATION_EASTING, 1e-8);
        assert!(outcome.report.resections.is_empty());
    }

    #[test]
    fn the_report_names_the_resection_its_targets_and_its_residuals() {
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        let outcome = reduce(&marks_and(&[A, B], &shots), &bare_settings());
        let text = katana_survey::reduction_report::render_text(&outcome.report);
        assert!(text.contains("Resections: setups positioned from the points they observe"));
        assert!(text.contains("A, B"));
        assert!(text.contains("Residuals: resection at S1 (horizontal)"));
        assert!(text.contains("direction at S1: A"));
        assert!(text.contains("Residuals: resection at S1 (heights)"));
        assert!(text.contains("resection"));
        // Its variance factor is not a network's: the Survey Jobs dialog reads
        // "Variance factor" rows out of the text, and there is no network here.
        assert!(!text.contains("Variance factor"));
        let html = katana_survey::reduction_report::render_html(&outcome.report);
        assert!(html.contains("Resections: setups positioned from the points they observe"));
    }

    #[test]
    fn a_resected_station_is_adjusted_with_the_rest_of_a_network() {
        // The resection gives the network its approximate station; the network
        // then adjusts it with everything else (here the same observations, so
        // the same place), and says it did.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        let project = marks_and(&[A, B, C], &shots);
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            control: vec![held("A"), held("B"), held("C")],
            ..bare_settings()
        };
        let outcome = reduce(&project, &settings);
        assert_eq!(outcome.report.resections.len(), 1);
        let r = find_point(&outcome, "R").expect("R");
        assert_eq!(r.method, ComputationMethod::NetworkLeastSquares);
        near(r.northing, STATION_NORTHING, 1e-7);
        near(r.easting, STATION_EASTING, 1e-7);
    }

    // A horizontal distance and a reading with no pointing or face, as the
    // traverse files give them.
    fn horizontal(target: &str, direction: f64, distance: Option<f64>) -> Shot {
        let mut horizontal = shot(
            target,
            0,
            Face::Unknown,
            Some(direction),
            None,
            distance,
            0.0,
        );
        horizontal.kind = DistanceKind::Horizontal;
        horizontal
    }

    #[test]
    fn a_traverse_adjustment_does_not_replace_a_resection_with_another_setups_radiation() {
        // A traverse job: S1 on A (N 1000, E 1000) oriented on R due north,
        // S2 to S4 round the square B, C, D back to A (Bowditch moves them).
        // And a free station: S5 on F, really at N 1050, E 950, reads A
        // (azimuth 135) and R (azimuth 45), each 50 sqrt 2 = 70.710678 m, its
        // circle at north, and radiates Q 20 m west (N 1050, E 930); S6 on Q,
        // oriented on A, measures F 20 m east as 20.005, a check 5 mm long.
        // After the traverse the side shots are radiated again from the
        // adjusted stations; F is not one of them (its resection placed it,
        // from A and R, which the traverse did not move) so it stays at
        // N 1050, E 950, where S6's radiation would put it at E 950.005.
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(20.0)));
        project.points.push(point("R", 1100.0, 1000.0, Some(20.0)));
        for id in ["B", "C", "D", "F", "Q"] {
            project.unpositioned_points.push(unpositioned(id));
        }
        project.stations.push(setup(
            "S1",
            "A",
            1.5,
            "R",
            &[
                horizontal("R", 0.0, None),
                horizontal("B", deg(90.0), Some(100.010)),
                horizontal("D", deg(180.0), None),
            ],
        ));
        project.stations.push(setup(
            "S2",
            "B",
            1.5,
            "A",
            &[
                horizontal("A", 0.0, None),
                horizontal("C", deg(270.0), Some(100.0)),
            ],
        ));
        project.stations.push(setup(
            "S3",
            "C",
            1.5,
            "B",
            &[
                horizontal("B", 0.0, None),
                horizontal("D", deg(270.0), Some(100.0)),
            ],
        ));
        project.stations.push(setup(
            "S4",
            "D",
            1.5,
            "C",
            &[
                horizontal("C", 0.0, None),
                horizontal("A", deg(270.0), Some(100.0)),
            ],
        ));
        let diagonal = 50.0 * 2.0_f64.sqrt();
        project.stations.push(setup(
            "S5",
            "F",
            1.5,
            "",
            &[
                horizontal("A", deg(135.0), Some(diagonal)),
                horizontal("R", deg(45.0), Some(diagonal)),
                horizontal("Q", deg(270.0), Some(20.0)),
            ],
        ));
        let to_a = normalize_angle((1000.0_f64 - 930.0).atan2(1000.0 - 1050.0));
        project.stations.push(setup(
            "S6",
            "Q",
            1.5,
            "A",
            &[
                horizontal("A", to_a, None),
                horizontal("F", deg(90.0), Some(20.005)),
            ],
        ));

        let settings = ReductionSettings {
            method: AdjustmentMethod::Traverse,
            ..bare_settings()
        };
        let outcome = reduce(&project, &settings);
        assert_eq!(outcome.report.resections.len(), 1);
        let f = find_point(&outcome, "F").expect("F");
        assert_eq!(f.method, ComputationMethod::Resection);
        near(f.northing, 1050.0, 1e-8);
        near(f.easting, 950.0, 1e-8);
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.easting, 930.0, 1e-8);
        // And the traverse was adjusted: B moved west by its Bowditch share.
        let b = find_point(&outcome, "B").expect("B");
        near(b.easting, 1100.00749981, 1e-8);
    }

    // ---- Geometry that does not fix the station, read with real errors -------------
    //
    // The refusals above are of exact readings, where the closed form finds the
    // geometry exactly. Readings carry their errors: at an arc second a station
    // on the danger circle, on one line with its targets, or with two targets a
    // few millimetres apart, is not exactly any of them, and a least squares
    // puts it wherever the errors do, tens of metres off. Each of these is
    // refused with its geometry named (the shape it comes nearest, with its
    // numbers at the solution) and leaves nothing behind: where its least
    // squares solves, by the station's own a-priori precision (resect_setup:
    // its ellipse at 95 % reaches past where the least squares' linear model
    // holds); where the least squares is rank deficient from every start, by
    // that. Each test has a case of the first kind (a resect_setup without the
    // precision test places those stations).

    #[test]
    fn a_station_on_the_danger_circle_read_with_unequal_errors_is_refused_not_placed() {
        // The danger circle of a_station_on_the_danger_circle_is_refused (R at
        // N 1000, E 900 on the circle of radius 100 about N 1000, E 1000
        // through A, B and C) but read with the errors field readings have:
        // -2", +0.5", +1" (which the resection placed 110 to 150 m off before
        // it judged its precision), and 0.1", 0, -0.1" (51.8 m off).
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        for errors in [[-2.0, 0.5, 1.0], [0.1, 0.0, -0.1]] {
            let mut shots = Vec::new();
            let mut index = 1;
            let readings = [(a, 45.0), (b, 90.0), (c, 135.0)];
            for (i, (target, azimuth)) in readings.iter().enumerate() {
                both_faces(
                    &mut shots,
                    target.id,
                    &mut index,
                    Some(deg(azimuth - 20.0) + arc_seconds(errors[i])),
                    None,
                    None,
                    TARGET_HEIGHT,
                );
            }
            shoot_q(&mut shots, &mut index);
            let outcome = reduce(&marks_and(&[a, b, c], &shots), &bare_settings());
            assert!(find_point(&outcome, "R").is_none(), "{}", errors[0]);
            assert!(find_point(&outcome, "Q").is_none(), "{}", errors[0]);
            assert!(outcome.report.resections.is_empty());
            // Where its least squares put it, on the circle (how near, the
            // errors decide) and the precision that refuses it.
            assert_eq!(
                warnings_with(&outcome.report, "It was not resected: it stands "),
                1,
                "{}",
                all_warnings(&outcome.report)
            );
            assert_eq!(
                warnings_with(
                    &outcome.report,
                    " from the circle through A, B and C (the danger circle, radius 100.000 m), \
                     and at the precision of its observations it is uncertain by "
                ),
                1,
                "{}",
                all_warnings(&outcome.report)
            );
            expect_nothing_left_behind(&outcome);
        }
    }

    #[test]
    fn a_station_on_one_line_with_its_targets_read_with_errors_is_refused() {
        // A 100 m and B 200 m due north of R, C 100 m due south, read +1", -1",
        // +0.5" off (its least squares is rank deficient from every start) and
        // -2", +0.5", +1" off (it solves, 304 m along the meridian, and the
        // precision refuses it); before the precision criterion, such a station
        // was placed 25 to 88 m along the line. Either way the refusal says how
        // near one line its sight lines are.
        let a = mark("A", 1140.0, 1030.0);
        let b = mark("B", 1240.0, 1030.0);
        let c = mark("C", 940.0, 1030.0);
        for errors in [[1.0, -1.0, 0.5], [-2.0, 0.5, 1.0]] {
            let mut shots = Vec::new();
            let mut index = 1;
            observe(
                &mut shots,
                &mut index,
                &a,
                false,
                100.0,
                arc_seconds(errors[0]),
            );
            observe(
                &mut shots,
                &mut index,
                &b,
                false,
                200.0,
                arc_seconds(errors[1]),
            );
            observe(
                &mut shots,
                &mut index,
                &c,
                false,
                100.0,
                arc_seconds(errors[2]),
            );
            let outcome = reduce(&marks_and(&[a, b, c], &shots), &bare_settings());
            assert!(find_point(&outcome, "R").is_none(), "{}", errors[0]);
            assert_eq!(
                warnings_with(
                    &outcome.report,
                    "It was not resected: its sight lines to A, B and C are within "
                ),
                1,
                "{}",
                all_warnings(&outcome.report)
            );
            assert_eq!(
                warnings_with(
                    &outcome.report,
                    if errors[0] == 1.0 {
                        "its least squares cannot fix it (the network is rank deficient"
                    } else {
                        "at the precision of its observations"
                    }
                ),
                1,
                "{}",
                all_warnings(&outcome.report)
            );
            expect_nothing_left_behind(&outcome);
        }
    }

    #[test]
    fn two_marks_a_few_millimetres_apart_do_not_fix_a_station_with_their_distances() {
        // M and M2 5 mm apart, 158 m from R (122 m north, 100 m east), each read
        // with a direction and an exact distance (four observations of three
        // unknowns, but the station can turn about the pair almost freely):
        // read +1" and -2" off, its least squares solves somewhere on that turn
        // and the precision refuses it (before, 29.6 m off). Then M read a
        // second time, 20" off the first: the repeated reading is redundancy
        // the turn cannot absorb, so its least squares flags both readings of M
        // before the precision refuses the station, and the refusal takes the
        // flags back. The refusal gives the pair's separation and their
        // measured distance, the longer of hypot(122, 100) = 157.747 and
        // hypot(122, 100.005) = 157.750 m.
        let m = mark("M", 1162.0, 1130.0);
        let m2 = mark("M2", 1162.0, 1130.005);
        let to_m = 122.0_f64.hypot(100.0);
        let to_m2 = 122.0_f64.hypot(100.005);
        for again in [false, true] {
            let mut shots = Vec::new();
            let mut index = 1;
            observe(
                &mut shots,
                &mut index,
                &m,
                true,
                to_m,
                if again { 0.0 } else { arc_seconds(1.0) },
            );
            observe(
                &mut shots,
                &mut index,
                &m2,
                true,
                to_m2,
                if again { 0.0 } else { arc_seconds(-2.0) },
            );
            if again {
                observe(&mut shots, &mut index, &m, true, to_m, arc_seconds(20.0));
            }
            let outcome = reduce(&marks_and(&[m, m2], &shots), &bare_settings());
            assert!(find_point(&outcome, "R").is_none(), "{again}");
            assert_eq!(
                warnings_with(
                    &outcome.report,
                    "It was not resected: M and M2 are 5.0 mm apart, 157.750 m from it, and at \
                     the precision of its observations"
                ),
                1,
                "{}",
                all_warnings(&outcome.report)
            );
            assert_eq!(
                warnings_with(&outcome.report, "at the precision of its observations"),
                1,
                "{}",
                all_warnings(&outcome.report)
            );
            expect_nothing_left_behind(&outcome);
        }
    }

    #[test]
    fn two_of_three_targets_a_few_millimetres_apart_do_not_fix_a_station_by_directions() {
        // Three readings, but A and A2 are 5 mm apart, so two directions in all:
        // read +1", -1" and +0.5" off (placed up to 156 m off before).
        let a2 = mark("A2", 1000.0, 1000.005);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, false, 50.0, arc_seconds(1.0));
        observe(&mut shots, &mut index, &a2, false, 50.0, arc_seconds(-1.0));
        observe(&mut shots, &mut index, &C, false, 50.0, arc_seconds(0.5));
        let outcome = reduce(&marks_and(&[A, a2, C], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: A and A2 are 5.0 mm apart, "
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
        assert_eq!(
            warnings_with(&outcome.report, "at the precision of its observations"),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
        expect_nothing_left_behind(&outcome);
    }

    #[test]
    fn a_weak_but_sound_geometry_is_placed_and_flagged_with_its_precision() {
        // R 20 m inside the danger circle of the test above (at N 1000, E 920)
        // read exactly by direction only. Its geometry fixes it, weakly. By a
        // separate script (normal equations of the three readings, each
        // weighted 1 / ((3"/sqrt 2)^2 + (1 mm / D)^2) with D 128.062 m to A and
        // C, 180 m to B): the a-priori standard ellipse's semi-major axis
        // 21.893 mm along the meridian (azimuth 0), 53.588 mm at 95 % (x
        // 2.4477), far inside the 650.8 mm (128.062 sqrt(2 x 2.6635")) its
        // linear model holds within, so it is placed; the least precise line of
        // position is B's, 180 m x 2.4110" = 2.104 mm, so the dilution is
        // 21.893 / 2.104 = 10.405, above WEAK_RESECTION_DILUTION: flagged weak,
        // and a warning says why.
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        let r_n = 1000.0;
        let r_e = 920.0;
        let mut shots = Vec::new();
        let mut index = 1;
        for target in [a, b, c] {
            both_faces(
                &mut shots,
                target.id,
                &mut index,
                Some(normalize_angle(azimuth_from(r_n, r_e, &target) - deg(20.0))),
                None,
                None,
                TARGET_HEIGHT,
            );
        }
        let outcome = reduce(&marks_and(&[a, b, c], &shots), &bare_settings());
        let r = find_point(&outcome, "R")
            .unwrap_or_else(|| panic!("{}", all_warnings(&outcome.report)));
        near(r.northing, r_n, 1e-8);
        near(r.easting, r_e, 1e-8);
        assert_eq!(outcome.report.resections.len(), 1);
        let resection = &outcome.report.resections[0];
        near(
            resection.apriori_ellipse.standard.semi_major,
            0.021893,
            1e-6,
        );
        near(resection.apriori_ellipse.confidence_scale, 2.4477, 1e-4);
        near(resection.dilution, 10.405, 1e-3);
        assert!(resection.weak_geometry);
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S1: its resection from A, B and C has a weak geometry - at the precision \
                 of its observations its station is uncertain by 53.6 mm (95 %) along azimuth 0 \
                 deg, 10.4 times the standard deviation of its least precise observation"
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
        // A sound geometry is not flagged: the shared R of the tests above, its
        // three marks all round it with distances (the same script: semi-major
        // axis 0.988 mm, least precise line a distance's hypot(2 mm, 2 ppm S,
        // 1 mm) = 2.238 mm, dilution 0.441).
        let mut round = Vec::new();
        index = 1;
        observe(&mut round, &mut index, &A, true, 50.0, 0.0);
        observe(&mut round, &mut index, &B, true, 50.0, 0.0);
        observe(&mut round, &mut index, &C, true, 50.0, 0.0);
        let sound = reduce(&marks_and(&[A, B, C], &round), &bare_settings());
        assert_eq!(sound.report.resections.len(), 1);
        assert!(!sound.report.resections[0].weak_geometry);
        near(sound.report.resections[0].dilution, 0.441388, 1e-6);
        assert_eq!(warnings_with(&sound.report, "weak geometry"), 0);
    }

    #[test]
    fn a_distance_that_is_not_positive_is_refused() {
        // C's horizontal distance recorded as 0: nothing it can be fitted to.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        let mut zero = shot(
            "C",
            index,
            Face::Left,
            Some(normalize_angle(
                azimuth_from(STATION_NORTHING, STATION_EASTING, &C) - deg(20.0),
            )),
            None,
            Some(0.0),
            TARGET_HEIGHT,
        );
        zero.kind = DistanceKind::Horizontal;
        shots.push(zero);
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: its distance to C is not a positive number."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_least_squares_that_fails_is_refused_with_its_reason() {
        // The perturbed resection above, allowed one Gauss-Newton iteration:
        // the closed-form start is millimetres off, so one correction does not
        // reach the adjustment's convergence threshold, and the adjustment
        // refuses.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.004, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(6.0));
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        let settings = ReductionSettings {
            max_iterations: 1,
            ..bare_settings()
        };
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &settings);
        assert!(find_point(&outcome, "R").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: its least squares cannot fix it (the adjustment did not \
                 converge"
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
        expect_nothing_left_behind(&outcome);
    }

    // ---- The distances' factors ----------------------------------------------------
    //
    // The resection fits horizontal distances to grid coordinates, so each takes
    // phase B's factors, at the approximate station, before it is fitted. Here
    // the marks are 50 m from R on the grid and the distances were measured on
    // the ground, 50 m divided by the factor; with the factor applied R comes
    // back where the readings were made from, and every distance fits.

    fn expect_resected_exactly(outcome: &ReductionOutcome) {
        let r =
            find_point(outcome, "R").unwrap_or_else(|| panic!("{}", all_warnings(&outcome.report)));
        near(r.northing, STATION_NORTHING, 1e-8);
        near(r.easting, STATION_EASTING, 1e-8);
        assert_eq!(outcome.report.resections.len(), 1);
        for residual in &outcome.report.resections[0].horizontal.residuals {
            let tolerance = if residual.angular { 1e-10 } else { 1e-8 };
            assert!(
                residual.residual.abs() <= tolerance,
                "{}: {}",
                residual.observation,
                residual.residual
            );
        }
    }

    fn ground_distances(factor: f64) -> SurveyProject {
        let mut shots = Vec::new();
        let mut index = 1;
        for target in [A, B, C] {
            observe(&mut shots, &mut index, &target, true, 50.0 / factor, 0.0);
        }
        marks_and(&[A, B, C], &shots)
    }

    #[test]
    fn its_distances_take_the_combined_factor() {
        // 50 / 0.9996 = 50.020008 m on the ground; x 0.9996 = 50 m on the grid.
        let settings = ReductionSettings {
            use_combined_factor: true,
            combined_factor: 0.9996,
            ..bare_settings()
        };
        let outcome = reduce(&ground_distances(0.9996), &settings);
        expect_resected_exactly(&outcome);
    }

    #[test]
    fn its_distances_take_a_fixed_grid_scale_factor() {
        // 50 / 1.0004 = 49.980008 m on the ground; x 1.0004 = 50 m on the grid.
        let settings = ReductionSettings {
            grid_scale: GridScale::Fixed,
            fixed_grid_scale_factor: 1.0004,
            ..bare_settings()
        };
        let outcome = reduce(&ground_distances(1.0004), &settings);
        expect_resected_exactly(&outcome);
    }

    #[test]
    fn its_distances_take_the_height_reduction_at_the_stations_height() {
        // To the geoid: R / (R + h), h the line's mean height: the station's
        // 52.345 (from its targets' heights less the height differences, before
        // it is placed) + the instrument's 1.550 + half the measured vertical
        // -2.195 = 52.7975 m; 6 371 000 / 6 371 052.7975 = 0.999991713, so the
        // ground distance is 50 / 0.999991713 = 50.000414358 m.
        let settings = ReductionSettings {
            height_reduction: HeightReduction::Geoid,
            ..bare_settings()
        };
        let factor = 6371000.0 / (6371000.0 + 52.7975);
        let outcome = reduce(&ground_distances(factor), &settings);
        expect_resected_exactly(&outcome);
    }

    #[test]
    fn ground_distances_fitted_to_a_projected_grid_are_said() {
        // The drawing's projection gives a point scale factor of 0.9998 (a
        // context function, constant here), and the distances were measured on
        // the ground: 50 / 0.9998 = 50.010002 m. With no grid scale set (the
        // import's default) the resection fits each 10 mm too long, more than a
        // distance's standard deviation (hypot(2 mm, 2 ppm x 50 m, 1 mm) =
        // 2.24 mm): it says so, and records the factor. With the grid scale
        // from the projection the distances are reduced to the grid, R comes
        // back exactly, and there is nothing to say.
        let context = ReductionContext {
            grid_scale_factor: Some(Box::new(|_: f64, _: f64, _: f64| Some(0.9998))),
            ..Default::default()
        };
        let project = ground_distances(0.9998);
        let ground = succeeded(reduce_and_adjust(&project, &bare_settings(), &context));
        assert_eq!(
            warnings_with(
                &ground.report,
                "Setup S1: its resection fitted distances measured on the ground to its targets' \
                 grid coordinates"
            ),
            1,
            "{}",
            all_warnings(&ground.report)
        );
        assert_eq!(ground.report.resections.len(), 1);
        assert_eq!(
            ground.report.resections[0].unapplied_scale_factor,
            Some(0.9998)
        );

        let settings = ReductionSettings {
            grid_scale: GridScale::FromProjection,
            ..bare_settings()
        };
        let grid = succeeded(reduce_and_adjust(&project, &settings, &context));
        expect_resected_exactly(&grid);
        assert!(grid.report.resections[0].unapplied_scale_factor.is_none());
        assert_eq!(warnings_with(&grid.report, "measured on the ground"), 0);
    }

    // ---- The weights: orientation and height -----------------------------------------

    #[test]
    fn the_orientation_is_the_weighted_mean_of_its_directions_at_the_station() {
        // A and B 50 m from R and D 200 m due north of it (N 1240, E 1030); A's
        // distance 4 mm long, B read 6" high and D 4" low; Q shot at reading 0,
        // 100 m, level. By the separate script of
        // residuals_and_precision_match_an_independent_least_squares' model: R
        // at N 1040.001304459, E 1030.001806901 and the orientation
        // 20.000671740 deg, which is the weighted mean of azimuth less reading
        // at R, each reading weighted 1 / ((3"/sqrt 2)^2 + (1 mm / D)^2), so D's
        // 200 m line counts more than A's and B's 50 m ones (the plain mean
        // would be 20.000746554 deg). So Q = R + 100 (cos w, sin w):
        // N 1133.970165544, E 1064.204922934 (the plain mean would put it
        // 0.13 mm away, at N 1133.970120884, E 1064.205045633).
        let d = mark("D", 1240.0, 1030.0);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.004, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(6.0));
        observe(&mut shots, &mut index, &d, true, 200.0, arc_seconds(-4.0));
        both_faces(
            &mut shots,
            "Q",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(100.0),
            INSTRUMENT_HEIGHT,
        );
        let outcome = reduce(&marks_and(&[A, B, d], &shots), &bare_settings());
        let r = find_point(&outcome, "R")
            .unwrap_or_else(|| panic!("{}", all_warnings(&outcome.report)));
        near(r.northing, 1040.001304459, 1e-8);
        near(r.easting, 1030.001806901, 1e-8);
        near(
            outcome.report.setups[0]
                .orientation_correction
                .expect("an orientation"),
            deg(20.000671740),
            1e-10,
        );
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1133.970165544, 1e-8);
        near(q.easting, 1064.204922934, 1e-8);
    }

    #[test]
    fn the_height_is_a_weighted_mean_over_sights_of_different_lengths() {
        // The default settings (curvature and refraction, k = 0.13): A and B
        // 50 m from R, D 200 m due north with its mark 6 mm high (50.006). By
        // hand, as the_height_is_the_weighted_mean_of_trigonometric_heights_with_
        // curvature (B = 6.82781353e-8): dh = -2.195 + B Y^2 + 1.550 - 1.700,
        // -2.344829305 to A and B (Y = 50) and -2.342268875 to D (Y = 200); R's
        // height from A and B 52.344829305, from D 50.006 + 2.342268875 =
        // 52.348268875. Each is weighted 1 / ((S sin z s_z)^2 + (cos z s_S)^2 +
        // (2 mm)^2) (s_z = 3"/sqrt 2 for a face pair, s_S = hypot(2 mm, 2 ppm
        // S), the target's height the 2 mm, the instrument's being common to
        // all three) which the separate script puts at 2.066915 mm for A and B
        // and 2.869024 mm for D. The weighted mean: 52.345537986 (the plain
        // mean would be 52.345975828); residuals (computed - observed)
        // -0.708681, -0.708681 and +2.730889 mm; v'Pv 1.141 on 2 degrees of
        // freedom, so the global test passes and the precision is the a-priori
        // 1 / sqrt(sum p) = 1.302290 mm, and with the instrument height's 2 mm,
        // hypot = 2.386621 mm (the a posteriori 0.983700 mm the round before
        // gave 2.228826).
        let d = mark("D", 1240.0, 1030.0);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &d, true, 200.0, 0.0);
        let mut project = marks_and(&[A, B, d], &shots);
        project.points[2].elevation = Some(50.006);
        let outcome = reduce(&project, &ReductionSettings::default());
        assert_eq!(outcome.report.resections.len(), 1);
        let resection = &outcome.report.resections[0];
        near(resection.elevation.expect("a height"), 52.345537986, 1e-9);
        let height = resection.height.as_ref().expect("a height adjustment");
        assert_eq!(height.residuals.len(), 3);
        near(height.residuals[0].residual, -0.000708681, 1e-9);
        near(height.residuals[1].residual, -0.000708681, 1e-9);
        near(height.residuals[2].residual, 0.002730889, 1e-9);
        near(
            resection.sigma_elevation.expect("a precision"),
            0.002386621,
            1e-9,
        );
    }

    #[test]
    fn a_resected_setup_follows_its_station_when_a_network_moves_it() {
        // R resected exactly from A, B and C, with Q shot 10 m from it. A second
        // setup, on K 50 m due south of R (N 990, E 1030), backsights A and
        // reads R by direction alone (so it places nothing, and R is resected)
        // 20" east of R. The network, A, B, C and K held, moves R toward that
        // reading; Q, a side shot of R's setup, is radiated again from where R
        // now stands, on the orientation R's resection gives there: the
        // weighted mean of azimuth less reading over its directions, here all
        // to 50 m lines and so equally weighted, worked below from the adjusted
        // R, a property of the result.
        let k = mark("K", 990.0, 1030.0);
        let mut at_r = Vec::new();
        let mut index = 1;
        observe(&mut at_r, &mut index, &A, true, 50.0, 0.0);
        observe(&mut at_r, &mut index, &B, true, 50.0, 0.0);
        observe(&mut at_r, &mut index, &C, true, 50.0, 0.0);
        shoot_q(&mut at_r, &mut index);
        let mut at_k = Vec::new();
        index = 1;
        both_faces(
            &mut at_k,
            "A",
            &mut index,
            Some(azimuth_from(k.northing, k.easting, &A)),
            None,
            None,
            TARGET_HEIGHT,
        );
        both_faces(
            &mut at_k,
            "R",
            &mut index,
            Some(arc_seconds(20.0)),
            None,
            None,
            TARGET_HEIGHT,
        );
        let mut project = SurveyProject::default();
        for target in [A, B, C, k] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        project.unpositioned_points.push(unpositioned("R"));
        project.unpositioned_points.push(unpositioned("Q"));
        project
            .stations
            .push(setup("S1", "R", INSTRUMENT_HEIGHT, "", &at_r));
        project.stations.push(setup("S2", "K", 1.5, "A", &at_k));
        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            control: vec![held("A"), held("B"), held("C"), held("K")],
            ..bare_settings()
        };
        let outcome = reduce(&project, &settings);
        assert_eq!(
            outcome.report.resections.len(),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
        let r = find_point(&outcome, "R").expect("R");
        assert_eq!(r.method, ComputationMethod::NetworkLeastSquares);
        // It reaches the case: the network did move R.
        assert!((r.northing - STATION_NORTHING).hypot(r.easting - STATION_EASTING) > 1e-4);

        let mut first: Option<f64> = None;
        let mut offsets = 0.0;
        for target in [A, B, C] {
            let reading = normalize_angle(
                azimuth_from(STATION_NORTHING, STATION_EASTING, &target) - deg(20.0),
            );
            let value = normalize_angle(azimuth_from(r.northing, r.easting, &target) - reading);
            let reference = *first.get_or_insert(value);
            offsets += normalize_angle_signed(value - reference);
        }
        let orientation = normalize_angle_signed(first.expect("a value") + offsets / 3.0);
        near(
            outcome.report.setups[0]
                .orientation_correction
                .expect("an orientation"),
            orientation,
            1e-12,
        );
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, r.northing + 10.0 * orientation.cos(), 1e-9);
        near(q.easting, r.easting + 10.0 * orientation.sin(), 1e-9);
    }

    // ---- Automatic rejection ---------------------------------------------------------

    #[test]
    fn a_gross_reading_is_rejected_and_the_station_orientation_and_shots_come_from_the_rest() {
        // A, B and C 50 m from R and D 200 m due north, all with distances; B
        // read 60" high; automatic rejection on. B's direction is the worst
        // flagged observation and is rejected; everything else is exact, so R
        // is where it was made from (N 1040, E 1030), the orientation 20 deg
        // (the rejected reading takes no part in it) and Q, read at 0, 100 m
        // level: N 1040 + 100 cos 20 = 1133.969262079, E 1030 + 100 sin 20 =
        // 1064.202014333.
        let d = mark("D", 1240.0, 1030.0);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(60.0));
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &d, true, 200.0, 0.0);
        both_faces(
            &mut shots,
            "Q",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(100.0),
            INSTRUMENT_HEIGHT,
        );
        let settings = ReductionSettings {
            auto_reject_outliers: true,
            ..bare_settings()
        };
        let outcome = reduce(&marks_and(&[A, B, C, d], &shots), &settings);
        assert_eq!(outcome.report.resections.len(), 1);
        let resection = &outcome.report.resections[0];
        assert_eq!(
            resection.horizontal.rejected_outliers,
            vec![String::from("direction at S1: B")]
        );
        let rejected: Vec<&ReportResidual> = resection
            .horizontal
            .residuals
            .iter()
            .filter(|residual| residual.rejected)
            .collect();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].observation, "direction at S1: B");
        let row = find_row(&outcome.report, "horizontal direction", "B", 0).expect("B's row");
        assert!(row.rejected);

        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, STATION_NORTHING, 1e-8);
        near(r.easting, STATION_EASTING, 1e-8);
        near(resection.orientation, deg(20.0), 1e-10);
        near(
            outcome.report.setups[0].orientation_correction.unwrap(),
            deg(20.0),
            1e-10,
        );
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1133.969262079, 1e-8);
        near(q.easting, 1064.202014333, 1e-8);
    }

    // ---- The file's resection block --------------------------------------------------

    #[test]
    fn a_check_after_the_files_resection_block_checks_the_station_and_does_not_move_it() {
        // The first test's A, B and C, exact, both faces (records 1 to 6) in
        // the block the file marks (RESECTION_END_METADATA "record 7", its
        // 129), then a check to A (record 7) read 20" high and 10 mm long, as a
        // controller takes one after the block. The resection is of the block:
        // R exactly where the readings were made from, redundancy 3 (six
        // observations, three unknowns), one check. The check is a misclosure
        // of the station, radiated on the resection's orientation (20 deg):
        // A' = R + 50.010 (cos, sin)(216.869897646 deg + 20") = N 999.994909652,
        // E 999.990120856, so -5.090348 mm N, -9.879144 mm E, 11.113466 mm; in
        // height 0 (its dh -2.345 as the block's). Without the key the check is
        // the resection's fourth pointing, and R moves.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        shots.push(shot(
            "A",
            index,
            Face::Left,
            Some(normalize_angle(
                azimuth_from(STATION_NORTHING, STATION_EASTING, &A) - deg(20.0) + arc_seconds(20.0),
            )),
            Some(50.010_f64.atan2(-2.195)),
            Some(50.010_f64.hypot(2.195)),
            TARGET_HEIGHT,
        ));
        let mut project = marks_and(&[A, B, C], &shots);
        project.stations[0].metadata.insert(
            String::from(RESECTION_END_METADATA),
            String::from("record 7"),
        );
        let outcome = reduce(&project, &bare_settings());
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, STATION_NORTHING, 1e-8);
        near(r.easting, STATION_EASTING, 1e-8);
        assert_eq!(outcome.report.resections.len(), 1);
        assert_eq!(outcome.report.resections[0].horizontal.redundancy, 3);
        assert_eq!(outcome.report.resections[0].checks, 1);
        let check = outcome
            .report
            .misclosures
            .iter()
            .rfind(|misclosure| misclosure.name == "A from setup S1")
            .expect("the check");
        near(check.northing.unwrap(), -0.005090348, 1e-9);
        near(check.easting.unwrap(), -0.009879144, 1e-9);
        near(check.linear.unwrap(), 0.011113466, 1e-9);
        near(check.height.expect("a height check"), 0.0, 1e-9);

        project.stations[0].metadata.remove(RESECTION_END_METADATA);
        let unmarked = reduce(&project, &bare_settings());
        let moved = find_point(&unmarked, "R").expect("R");
        assert!((moved.northing - STATION_NORTHING).hypot(moved.easting - STATION_EASTING) > 1e-4);
        assert_eq!(unmarked.report.resections.len(), 1);
        assert_eq!(unmarked.report.resections[0].horizontal.redundancy, 5);
        assert_eq!(unmarked.report.resections[0].checks, 0);
    }

    #[test]
    fn a_block_end_that_names_no_record_leaves_every_pointing_to_the_resection() {
        // The block and check of the test above, the key's value "record" with
        // no number, empty, or not a record: the reduction cannot tell where
        // the block ends, so every pointing is the resection's, as for a file
        // that marks none: redundancy 5 (eight observations, three unknowns),
        // where "record 7" gives 3.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        shots.push(shot(
            "A",
            index,
            Face::Left,
            Some(normalize_angle(
                azimuth_from(STATION_NORTHING, STATION_EASTING, &A) - deg(20.0),
            )),
            Some(50.0_f64.atan2(-2.195)),
            Some(50.0_f64.hypot(2.195)),
            TARGET_HEIGHT,
        ));
        for value in ["record", "", "record -3", "record 7"] {
            let mut project = marks_and(&[A, B, C], &shots);
            project.stations[0]
                .metadata
                .insert(String::from(RESECTION_END_METADATA), String::from(value));
            let outcome = reduce(&project, &bare_settings());
            assert_eq!(outcome.report.resections.len(), 1, "{value}");
            assert_eq!(
                outcome.report.resections[0].horizontal.redundancy,
                if value == "record 7" { 3 } else { 5 },
                "{value}"
            );
        }
    }

    #[test]
    fn a_second_block_on_a_station_already_resected_says_what_it_could_be_oriented_on() {
        // S1 resects R from A and B; S2, a second block on R with no backsight,
        // reads A and B again and shoots Q2. R has a position when S2 is tried,
        // so S2 is not resected, and a setup on a placed station is not
        // oriented on the points it observes (docs/survey.md, Not done): Q2 is
        // not placed, and the warning names the placed points S2 observes, so
        // it does not read as a setup that observed nothing known.
        let mut first = Vec::new();
        let mut index = 1;
        observe(&mut first, &mut index, &A, true, 50.0, 0.0);
        observe(&mut first, &mut index, &B, true, 50.0, 0.0);
        let mut second = Vec::new();
        index = 1;
        observe(&mut second, &mut index, &A, true, 50.0, 0.0);
        observe(&mut second, &mut index, &B, true, 50.0, 0.0);
        both_faces(
            &mut second,
            "Q2",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(10.0),
            INSTRUMENT_HEIGHT,
        );
        let mut project = marks_and(&[A, B], &first);
        project.unpositioned_points.push(unpositioned("Q2"));
        project
            .stations
            .push(setup("S2", "R", INSTRUMENT_HEIGHT, "", &second));
        let outcome = reduce(&project, &bare_settings());
        assert!(find_point(&outcome, "R").is_some());
        assert!(find_point(&outcome, "Q2").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S2 has no backsight; it observes 2 placed points (A, B), but a setup on a \
                 placed station is not oriented on the points it observes, so its directions \
                 are not oriented and its targets are not radiated."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    // ---- Where the least squares starts, and a station with two answers -------------
    //
    // Gauss-Newton finds the solution nearest its start. The resection starts
    // it from every point where two of the station's loci meet, best first,
    // and refuses a station whose observations fit two positions alike
    // (resect_setup).

    #[test]
    fn two_positions_its_observations_fit_alike_are_refused_not_either_placed() {
        // A (N 1000, E 1000) and A2 5 mm east of it, each read with a direction
        // and a distance, and C 60 m east of A by direction alone: one distance
        // and one angle, two circles that meet twice. R really stands 100 m
        // from A at bearing 130 deg (N 935.721239, E 1076.604444), its circle
        // turned 20 deg, the sights level; A read 2" low and 2 mm short. By the
        // separate script of the model of
        // residuals_and_precision_match_an_independent_least_squares, its least
        // squares has two solutions: near R, N 935.723203, E 1076.604787 (v'Pv
        // 0.625), and N 906.663885, E 1035.888245 (v'Pv 0.057), 50.023 m away,
        // where the round before, started from the rigid fit on the close pair,
        // placed the station, 32 mm uncertain at 95 %. Their squares differ by
        // 0.568, under chi-square(2) at 95 % (5.99), and each lies far outside
        // the other's ellipse (Mahalanobis^2 1.3e8): the observations do not
        // tell them apart, and neither is placed. The pair's measured distances
        // are 99.998 (A, 2 mm short) and 99.996 m.
        let a = mark("A", 1000.0, 1000.0);
        let a2 = mark("A2", 1000.0, 1000.005);
        let c = mark("C", 1000.0, 1060.0);
        let r_n = 1000.0 + 100.0 * deg(130.0).cos();
        let r_e = 1000.0 + 100.0 * deg(130.0).sin();
        let mut shots = Vec::new();
        let mut index = 1;
        level(
            &mut shots,
            &mut index,
            r_n,
            r_e,
            &a,
            deg(20.0),
            true,
            arc_seconds(-2.0),
            Some((a.northing - r_n).hypot(a.easting - r_e) - 0.002),
        );
        level(
            &mut shots,
            &mut index,
            r_n,
            r_e,
            &a2,
            deg(20.0),
            true,
            0.0,
            None,
        );
        level(
            &mut shots,
            &mut index,
            r_n,
            r_e,
            &c,
            deg(20.0),
            false,
            0.0,
            None,
        );
        shoot_q(&mut shots, &mut index);
        let outcome = reduce(&marks_and(&[a, a2, c], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_none());
        assert!(find_point(&outcome, "Q").is_none());
        assert!(outcome.report.resections.is_empty());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "It was not resected: A and A2 are 5.0 mm apart, 99.998 m from it, and its \
                 observations fit two positions 50.023 m apart (N 906.664 E 1035.888 and N \
                 935.723 E 1076.605: weighted squares of residuals 0.057 and 0.625, where the \
                 station's 95 % confidence region takes in all within 5.99 of the least), so \
                 they do not fix it."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
        expect_nothing_left_behind(&outcome);
    }

    #[test]
    fn a_fourth_direction_fixes_a_station_whose_first_three_targets_are_on_its_danger_circle() {
        // A, B and C on the circle of radius 100 about N 1000, E 1000, and R on
        // it at N 1000, E 900 (a_station_on_the_danger_circle_is_refused); D
        // 100 m west of R, N 1000, E 800, off that circle. Four directions fix
        // R. The round before started from the first three read, on the danger
        // circle, and refused R about half the time, by the errors, and never
        // when D was read first. Exact readings put R where they were made
        // from, whichever is read first. A read 2" low, B 0.5" and C 1" high, D
        // exact put it, read either way round, where the separate script puts
        // it: N 999.999959041, E 900.001454430, 1.5 mm off (v'Pv 0.113, one
        // degree of freedom).
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        let d = mark("D", 1000.0, 800.0);
        let r_n = 1000.0;
        let r_e = 900.0;
        for errors in [false, true] {
            for d_first in [false, true] {
                let error = |seconds: f64| if errors { arc_seconds(seconds) } else { 0.0 };
                let mut shots = Vec::new();
                let mut index = 1;
                if d_first {
                    level(
                        &mut shots,
                        &mut index,
                        r_n,
                        r_e,
                        &d,
                        deg(20.0),
                        false,
                        0.0,
                        None,
                    );
                }
                level(
                    &mut shots,
                    &mut index,
                    r_n,
                    r_e,
                    &a,
                    deg(20.0),
                    false,
                    error(-2.0),
                    None,
                );
                level(
                    &mut shots,
                    &mut index,
                    r_n,
                    r_e,
                    &b,
                    deg(20.0),
                    false,
                    error(0.5),
                    None,
                );
                level(
                    &mut shots,
                    &mut index,
                    r_n,
                    r_e,
                    &c,
                    deg(20.0),
                    false,
                    error(1.0),
                    None,
                );
                if !d_first {
                    level(
                        &mut shots,
                        &mut index,
                        r_n,
                        r_e,
                        &d,
                        deg(20.0),
                        false,
                        0.0,
                        None,
                    );
                }
                let outcome = reduce(&marks_and(&[a, b, c, d], &shots), &bare_settings());
                let r = find_point(&outcome, "R").unwrap_or_else(|| {
                    panic!("{errors} {d_first}\n{}", all_warnings(&outcome.report))
                });
                near(r.northing, if errors { 999.999959041 } else { r_n }, 1e-8);
                near(r.easting, if errors { 900.001454430 } else { r_e }, 1e-8);
                assert_eq!(outcome.report.resections.len(), 1);
                assert_eq!(outcome.report.resections[0].horizontal.redundancy, 1);
            }
        }
    }

    #[test]
    fn a_distance_to_one_target_fixes_a_station_on_the_danger_circle_of_its_directions() {
        // R on the danger circle of A, B and C again, now with its distance to
        // A, hypot(100, 100) = 141.421356 m, level: the distance's circle about
        // A meets the danger circle at R and at B, a target, so at R alone.
        // Exact readings put R where they were made; A read 1" high and 1 mm
        // long, B 1" low and C 0.5" high put it where the separate script puts
        // it: N 999.998828191, E 899.999757600. The round before refused both:
        // the closed form of its three directions found the danger circle, and
        // it looked no further.
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        let r_n = 1000.0;
        let r_e = 900.0;
        for errors in [false, true] {
            let error = |seconds: f64| if errors { arc_seconds(seconds) } else { 0.0 };
            let mut shots = Vec::new();
            let mut index = 1;
            level(
                &mut shots,
                &mut index,
                r_n,
                r_e,
                &a,
                deg(20.0),
                true,
                error(1.0),
                Some(100.0_f64.hypot(100.0) + if errors { 0.001 } else { 0.0 }),
            );
            level(
                &mut shots,
                &mut index,
                r_n,
                r_e,
                &b,
                deg(20.0),
                false,
                error(-1.0),
                None,
            );
            level(
                &mut shots,
                &mut index,
                r_n,
                r_e,
                &c,
                deg(20.0),
                false,
                error(0.5),
                None,
            );
            let outcome = reduce(&marks_and(&[a, b, c], &shots), &bare_settings());
            let r = find_point(&outcome, "R")
                .unwrap_or_else(|| panic!("{errors}\n{}", all_warnings(&outcome.report)));
            near(r.northing, if errors { 999.998828191 } else { r_n }, 1e-8);
            near(r.easting, if errors { 899.999757600 } else { r_e }, 1e-8);
        }
    }

    // ---- A network after a resection -----------------------------------------------

    #[test]
    fn a_network_takes_no_reading_its_resection_rejected() {
        // The station of a_gross_reading_is_rejected_and_the_station_orientation_
        // and_shots_come_from_the_rest, with three gross readings: B's direction
        // 60" high, read first; D's distance 30 mm long; and C's mark 50 mm
        // higher than its sight says, so its height difference is 50 mm out.
        // Adjusted as a network of plan and heights, A, B, C and D held,
        // automatic rejection on. The resection rejects all three. The network
        // takes none of them (not B's direction as an angle, nor as the setup's
        // reference, which every angle is measured from, whether the setup
        // names B as its backsight or names none; the round before made B the
        // reference, put its 60" into every angle, rejected the right ones and
        // left R 6.8 mm off, while the report called B rejected); not D's
        // distance; not C's height difference, so the network rejects nothing
        // of its own. Everything else is exact: R stays where it was made from,
        // at A's, B's and D's height, 50 + 2.345 = 52.345, and Q, radiated at
        // reading 0, 100 m level, at N 1040 + 100 cos 20 = 1133.969262079,
        // E 1030 + 100 sin 20 = 1064.202014333, at R's height.
        let d = mark("D", 1240.0, 1030.0);
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(60.0));
        observe(&mut shots, &mut index, &A, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &d, true, 200.030, 0.0);
        both_faces(
            &mut shots,
            "Q",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(100.0),
            INSTRUMENT_HEIGHT,
        );
        let settings = ReductionSettings {
            auto_reject_outliers: true,
            method: AdjustmentMethod::Network,
            network_dimension:
                katana_survey::reduction_settings::NetworkDimension::HorizontalAndLevels,
            control: vec![held("A"), held("B"), held("C"), held("D")],
            ..bare_settings()
        };
        for backsight in ["", "B"] {
            let mut project = marks_and(&[A, B, C, d], &shots);
            project.points[2].elevation = Some(50.050); // C
            project.stations[0].backsight_point_id = String::from(backsight);
            let outcome = reduce(&project, &settings);
            let report = &outcome.report;
            assert_eq!(report.resections.len(), 1, "{}", all_warnings(report));
            let resection = &report.resections[0];
            let mut rejected = resection.horizontal.rejected_outliers.clone();
            rejected.sort();
            assert_eq!(
                rejected,
                vec![
                    String::from("direction at S1: B"),
                    String::from("distance at S1: D")
                ],
                "{backsight}"
            );
            let height = resection.height.as_ref().expect("a height adjustment");
            assert_eq!(
                height.rejected_outliers,
                vec![String::from("height difference at S1: C")],
                "{backsight}"
            );
            let row = find_row(report, "horizontal direction", "B", 0).expect("B's row");
            assert!(row.rejected);
            assert!(
                row.rejection_reason.contains("Baarda w"),
                "{}",
                row.rejection_reason
            );

            assert_eq!(report.adjustments.len(), 2, "{backsight}");
            let mut angle_from_a = false;
            for network in &report.adjustments {
                assert!(
                    network.rejected_outliers.is_empty(),
                    "{backsight} {}",
                    network.method
                );
                for residual in &network.residuals {
                    let label = &residual.observation;
                    assert!(!label.contains("B ->"), "{backsight} {label}");
                    assert!(!label.contains("-> B"), "{backsight} {label}");
                    assert_ne!(label, "distance at S1: D", "{backsight}");
                    assert_ne!(label, "height difference at S1: C", "{backsight}");
                    angle_from_a = angle_from_a || label == "angle at S1: A -> C";
                }
            }
            assert!(angle_from_a, "{backsight}");

            let r = find_point(&outcome, "R").expect("R");
            assert_eq!(r.method, ComputationMethod::NetworkLeastSquares);
            near(r.northing, STATION_NORTHING, 1e-8);
            near(r.easting, STATION_EASTING, 1e-8);
            near(r.elevation.expect("a height"), STATION_HEIGHT, 1e-9);
            let q = find_point(&outcome, "Q").expect("Q");
            near(q.northing, 1133.969262079, 1e-8);
            near(q.easting, 1064.202014333, 1e-8);
        }
    }

    // ---- When a resection is tried -------------------------------------------------

    #[test]
    fn a_free_station_on_a_mark_an_earlier_free_station_shot_is_resected_from_its_own_block() {
        // Two free stations, neither naming a backsight, their circles at
        // north. R1 (N 1050, E 1050) is resected from A (N 1000, E 1000) and B
        // (N 1000, E 1100) and reads R2's mark 100 m due north 20" high, so it
        // radiates R2 to N 1050 + 100 cos 20" = 1149.999999530, E 1050 +
        // 100 sin 20" = 1050.009696274. R2's own setup reads C (N 1200, E 1000)
        // and D (N 1200, E 1100) with distances and shoots Q2 due east 10 m.
        // Nothing orients that setup but its block, so it is resected from it
        // (R2 at N 1150, E 1050, where its readings were made) and the
        // radiation from R1 becomes a check of it: resection less radiation
        // +0.000000470 N, -0.009696274 E, 9.7 mm, in height 0 (level sights).
        // Q2 at N 1150, E 1060. The round before left R2 on the radiation and
        // its setup unoriented, and dropped Q2.
        let a = mark("A", 1000.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 1200.0, 1000.0);
        let d = mark("D", 1200.0, 1100.0);
        let r2 = mark("R2", 1150.0, 1050.0);
        let mut project = SurveyProject::default();
        for target in [a, b, c, d] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        for id in ["R1", "R2", "Q2"] {
            project.unpositioned_points.push(unpositioned(id));
        }
        let mut at_r1 = Vec::new();
        let mut index = 1;
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &a, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &b, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r1,
            &mut index,
            1050.0,
            1050.0,
            &r2,
            0.0,
            true,
            arc_seconds(20.0),
            None,
        );
        project
            .stations
            .push(setup("S1", "R1", INSTRUMENT_HEIGHT, "", &at_r1));
        let mut at_r2 = Vec::new();
        index = 1;
        level(
            &mut at_r2, &mut index, 1150.0, 1050.0, &c, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r2, &mut index, 1150.0, 1050.0, &d, 0.0, true, 0.0, None,
        );
        both_faces(
            &mut at_r2,
            "Q2",
            &mut index,
            Some(deg(90.0)),
            Some(deg(90.0)),
            Some(10.0),
            INSTRUMENT_HEIGHT,
        );
        project
            .stations
            .push(setup("S2", "R2", INSTRUMENT_HEIGHT, "", &at_r2));

        let outcome = reduce(&project, &bare_settings());
        let report = &outcome.report;
        assert_eq!(report.resections.len(), 2, "{}", all_warnings(report));
        assert_eq!(report.resections[1].station_id, "S2");
        let resected = find_point(&outcome, "R2").expect("R2");
        assert_eq!(resected.method, ComputationMethod::Resection);
        near(resected.northing, 1150.0, 1e-8);
        near(resected.easting, 1050.0, 1e-8);

        let second = &report.resections[1];
        assert_eq!(second.radiated_from, "S1");
        near(
            second
                .radiated_northing_difference
                .expect("the radiation's difference"),
            0.000000470,
            1e-9,
        );
        near(
            second.radiated_easting_difference.unwrap(),
            -0.009696274,
            1e-9,
        );
        let check = report
            .misclosures
            .iter()
            .rfind(|misclosure| {
                misclosure.name
                    == "R2 by resection at setup S2, against its radiation from setup S1"
            })
            .expect("the check");
        near(check.linear.unwrap(), 0.009696274, 1e-9);
        near(check.height.expect("a height check"), 0.0, 1e-9);
        assert_eq!(
            warnings_with(
                report,
                "Setup S2 stands on R2, which setup S1 radiated: its own resection from C and D \
                 places it 9.7 mm from there, and the radiation is a check of it"
            ),
            1,
            "{}",
            all_warnings(report)
        );
        assert_eq!(
            warnings_with(report, "has no backsight"),
            0,
            "{}",
            all_warnings(report)
        );

        let q2 = find_point(&outcome, "Q2").expect("Q2");
        near(q2.northing, 1150.0, 1e-8);
        near(q2.easting, 1060.0, 1e-8);
    }

    #[test]
    fn a_free_station_on_a_radiated_mark_with_too_few_placed_points_says_why() {
        // The test above with D left out of R2's block: C alone, one placed
        // point, does not resect R2, so its setup (naming no backsight) is not
        // oriented, and the warning says what a resection would have needed.
        let a = mark("A", 1000.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 1200.0, 1000.0);
        let r2 = mark("R2", 1150.0, 1050.0);
        let mut project = SurveyProject::default();
        for target in [a, b, c] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        for id in ["R1", "R2", "Q2"] {
            project.unpositioned_points.push(unpositioned(id));
        }
        let mut at_r1 = Vec::new();
        let mut index = 1;
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &a, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &b, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &r2, 0.0, true, 0.0, None,
        );
        project
            .stations
            .push(setup("S1", "R1", INSTRUMENT_HEIGHT, "", &at_r1));
        let mut at_r2 = Vec::new();
        index = 1;
        level(
            &mut at_r2, &mut index, 1150.0, 1050.0, &c, 0.0, true, 0.0, None,
        );
        both_faces(
            &mut at_r2,
            "Q2",
            &mut index,
            Some(deg(90.0)),
            Some(deg(90.0)),
            Some(10.0),
            INSTRUMENT_HEIGHT,
        );
        project
            .stations
            .push(setup("S2", "R2", INSTRUMENT_HEIGHT, "", &at_r2));

        let outcome = reduce(&project, &bare_settings());
        assert_eq!(outcome.report.resections.len(), 1);
        let radiated = find_point(&outcome, "R2").expect("R2");
        assert_eq!(radiated.method, ComputationMethod::Radiation);
        assert!(find_point(&outcome, "Q2").is_none());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S2 has no backsight; it observes 1 placed point (C), but a setup on a \
                 placed station is not oriented on the points it observes, so its directions \
                 are not oriented and its targets are not radiated. It was not resected: it \
                 observes 1 placed point with a direction (C), 1 of them with a distance as \
                 well, where a resection needs two placed points observed with a direction and \
                 a distance, or three observed with a direction."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_resection_comes_before_the_circle_as_set() {
        // S1 on K (N 1000, E 1000) backsights R, which nothing places; its
        // circle was set to 44 deg on R, which is really at azimuth 45 (a
        // degree slipped) and it reads R there, 70.711 m level. S2 on R
        // (N 1050, E 1050), naming no backsight, reads A (N 1100, E 1000) and B
        // (N 1000, E 1100) with distances, its circle at north, and shoots Q
        // due east 10 m. The resection comes first: R where its readings were
        // made, and S1 oriented on it, 45 - 44 = +1 deg (the slip found) where
        // the circle taken as a grid azimuth would radiate R 1.23 m off. Q at
        // N 1050, E 1060.
        let k = mark("K", 1000.0, 1000.0);
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let r = mark("R", 1050.0, 1050.0);
        let mut project = SurveyProject::default();
        for target in [k, a, b] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        project.unpositioned_points.push(unpositioned("R"));
        project.unpositioned_points.push(unpositioned("Q"));
        let mut at_k = Vec::new();
        let mut index = 1;
        level(
            &mut at_k,
            &mut index,
            k.northing,
            k.easting,
            &r,
            deg(1.0),
            true,
            0.0,
            None,
        );
        let mut first = setup("S1", "K", INSTRUMENT_HEIGHT, "R", &at_k);
        first.backsight_azimuth = Some(deg(44.0));
        project.stations.push(first);
        let mut at_r = Vec::new();
        index = 1;
        level(
            &mut at_r, &mut index, r.northing, r.easting, &a, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r, &mut index, r.northing, r.easting, &b, 0.0, true, 0.0, None,
        );
        both_faces(
            &mut at_r,
            "Q",
            &mut index,
            Some(deg(90.0)),
            Some(deg(90.0)),
            Some(10.0),
            INSTRUMENT_HEIGHT,
        );
        project
            .stations
            .push(setup("S2", "R", INSTRUMENT_HEIGHT, "", &at_r));

        let outcome = reduce(&project, &bare_settings());
        let resected = find_point(&outcome, "R").expect("R");
        assert_eq!(resected.method, ComputationMethod::Resection);
        near(resected.northing, 1050.0, 1e-8);
        near(resected.easting, 1050.0, 1e-8);
        near(
            outcome.report.setups[0]
                .orientation_correction
                .expect("an orientation"),
            deg(1.0),
            1e-10,
        );
        assert_eq!(
            warnings_with(&outcome.report, "circle"),
            0,
            "{}",
            all_warnings(&outcome.report)
        );
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1050.0, 1e-8);
        near(q.easting, 1060.0, 1e-8);
    }

    #[test]
    fn a_resection_takes_the_points_an_earlier_resection_placed() {
        // Two free stations resected one at a time, in file order. R1 (N 1050,
        // E 1050) reads A (N 1000, E 1000) and B (N 1000, E 1100) with
        // distances, its circle at north, and radiates E at reading 0, 50 m:
        // N 1100, E 1050. R2 (N 1150, E 1050) reads C (N 1200, E 1000) and D
        // (N 1200, E 1100), entered, and E, which only R1 places. R2 is
        // resected once R1 has radiated: from all three (three directions and
        // three distances, redundancy 3) not from the two entered ones it could
        // have had at the start (redundancy 1).
        let a = mark("A", 1000.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 1200.0, 1000.0);
        let d = mark("D", 1200.0, 1100.0);
        let e = mark("E", 1100.0, 1050.0);
        let mut project = SurveyProject::default();
        for target in [a, b, c, d] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        for id in ["R1", "R2", "E"] {
            project.unpositioned_points.push(unpositioned(id));
        }
        let mut at_r1 = Vec::new();
        let mut index = 1;
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &a, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &b, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r1, &mut index, 1050.0, 1050.0, &e, 0.0, true, 0.0, None,
        );
        project
            .stations
            .push(setup("S1", "R1", INSTRUMENT_HEIGHT, "", &at_r1));
        let mut at_r2 = Vec::new();
        index = 1;
        level(
            &mut at_r2, &mut index, 1150.0, 1050.0, &c, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r2, &mut index, 1150.0, 1050.0, &d, 0.0, true, 0.0, None,
        );
        level(
            &mut at_r2, &mut index, 1150.0, 1050.0, &e, 0.0, true, 0.0, None,
        );
        project
            .stations
            .push(setup("S2", "R2", INSTRUMENT_HEIGHT, "", &at_r2));

        let outcome = reduce(&project, &bare_settings());
        assert_eq!(
            outcome.report.resections.len(),
            2,
            "{}",
            all_warnings(&outcome.report)
        );
        let second = &outcome.report.resections[1];
        assert_eq!(second.station_id, "S2");
        assert_eq!(
            second.targets,
            vec![String::from("C"), String::from("D"), String::from("E")]
        );
        assert_eq!(second.horizontal.redundancy, 3);
        near(second.northing, 1150.0, 1e-8);
        near(second.easting, 1050.0, 1e-8);
    }

    #[test]
    fn a_refused_resection_is_tried_again_when_another_of_its_targets_is_placed() {
        // S1 on R reads A, B and C on its danger circle, exactly, and D
        // (N 1000, E 800), which nothing places yet: refused, the three placed
        // ones alone on the circle. Then S2 stands on M (N 1000, E 700), known
        // only by the file's own coordinates, backsights L (N 1100, E 700) due
        // north at reading 0, and radiates D due east, 100 m. With D placed, S1
        // is tried again, and resected from all four where its readings were
        // made.
        let a = mark("A", 1100.0, 1000.0);
        let b = mark("B", 1000.0, 1100.0);
        let c = mark("C", 900.0, 1000.0);
        let d = mark("D", 1000.0, 800.0);
        let l = mark("L", 1100.0, 700.0);
        let mut project = SurveyProject::default();
        for target in [a, b, c, l] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        project.points.push(point_from(
            "M",
            1000.0,
            700.0,
            Some(50.0),
            CoordinateSource::Calculated,
        ));
        project.unpositioned_points.push(unpositioned("R"));
        project.unpositioned_points.push(unpositioned("D"));
        let mut at_r = Vec::new();
        let mut index = 1;
        for target in [a, b, c, d] {
            level(
                &mut at_r,
                &mut index,
                1000.0,
                900.0,
                &target,
                deg(20.0),
                false,
                0.0,
                None,
            );
        }
        project
            .stations
            .push(setup("S1", "R", INSTRUMENT_HEIGHT, "", &at_r));
        let mut at_m = Vec::new();
        index = 1;
        level(
            &mut at_m, &mut index, 1000.0, 700.0, &l, 0.0, true, 0.0, None,
        );
        level(
            &mut at_m, &mut index, 1000.0, 700.0, &d, 0.0, true, 0.0, None,
        );
        project
            .stations
            .push(setup("S2", "M", INSTRUMENT_HEIGHT, "L", &at_m));

        let outcome = reduce(&project, &bare_settings());
        let r = find_point(&outcome, "R")
            .unwrap_or_else(|| panic!("{}", all_warnings(&outcome.report)));
        assert_eq!(r.method, ComputationMethod::Resection);
        near(r.northing, 1000.0, 1e-8);
        near(r.easting, 900.0, 1e-8);
        assert_eq!(outcome.report.resections.len(), 1);
        assert_eq!(
            outcome.report.resections[0].targets,
            vec![
                String::from("A"),
                String::from("B"),
                String::from("C"),
                String::from("D")
            ]
        );
    }

    #[test]
    fn a_station_another_setup_radiated_keeps_it_while_its_setup_waits_for_its_backsight() {
        // S1 on K (N 990, E 1030) backsights A and radiates R, read 30" high,
        // 50 m due north: N 990 + 50 cos 30" = 1039.999999471, E 1030 +
        // 50 sin 30" = 1030.007272205. S2 on R names a backsight, X, that
        // nothing places, and reads A and B with distances (enough for a
        // resection). But S2 names a backsight, so it is oriented on that or
        // not at all: R keeps the position S1 gave it, and S2 is given up for
        // want of X.
        let k = mark("K", 990.0, 1030.0);
        let mut project = SurveyProject::default();
        for target in [k, A, B] {
            project.points.push(point(
                target.id,
                target.northing,
                target.easting,
                Some(50.0),
            ));
        }
        project.unpositioned_points.push(unpositioned("R"));
        project.unpositioned_points.push(unpositioned("X"));
        let mut at_k = Vec::new();
        let mut index = 1;
        level(
            &mut at_k, &mut index, k.northing, k.easting, &A, 0.0, true, 0.0, None,
        );
        level(
            &mut at_k,
            &mut index,
            k.northing,
            k.easting,
            &mark("R", STATION_NORTHING, STATION_EASTING),
            0.0,
            true,
            arc_seconds(30.0),
            None,
        );
        project
            .stations
            .push(setup("S1", "K", INSTRUMENT_HEIGHT, "A", &at_k));
        let mut at_r = Vec::new();
        index = 1;
        level(
            &mut at_r,
            &mut index,
            STATION_NORTHING,
            STATION_EASTING,
            &A,
            deg(20.0),
            true,
            0.0,
            None,
        );
        level(
            &mut at_r,
            &mut index,
            STATION_NORTHING,
            STATION_EASTING,
            &B,
            deg(20.0),
            true,
            0.0,
            None,
        );
        project
            .stations
            .push(setup("S2", "R", INSTRUMENT_HEIGHT, "X", &at_r));

        let outcome = reduce(&project, &bare_settings());
        assert!(outcome.report.resections.is_empty());
        let r = find_point(&outcome, "R").expect("R");
        assert_eq!(r.method, ComputationMethod::Radiation);
        near(r.northing, 1039.999999471, 1e-8);
        near(r.easting, 1030.007272205, 1e-8);
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S2 cannot be oriented: its backsight X has no position"
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }

    #[test]
    fn a_resected_setup_is_oriented_by_its_resection_not_by_the_backsight_it_names() {
        // The shared R, its setup naming A as its backsight; A read 10" high on
        // both faces, B and C exact, all with distances; Q read at 0, 100 m
        // level. By the separate script: R at N 1039.998877235, E
        // 1030.000621099, its orientation 19.999216428 deg (the least squares',
        // the weighted mean of azimuth less reading over its three directions
        // at R) so Q at N 1133.968607048, E 1064.201350313. Oriented on the
        // backsight alone it would be 19.998563574 deg, and Q 1.1 mm away
        // (N 1133.968996740, E 1064.200279578).
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, arc_seconds(10.0));
        observe(&mut shots, &mut index, &B, true, 50.0, 0.0);
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        both_faces(
            &mut shots,
            "Q",
            &mut index,
            Some(0.0),
            Some(deg(90.0)),
            Some(100.0),
            INSTRUMENT_HEIGHT,
        );
        let mut project = marks_and(&[A, B, C], &shots);
        project.stations[0].backsight_point_id = String::from("A");
        let outcome = reduce(&project, &bare_settings());
        let r = find_point(&outcome, "R").expect("R");
        near(r.northing, 1039.998877235, 1e-8);
        near(r.easting, 1030.000621099, 1e-8);
        near(
            outcome.report.setups[0]
                .orientation_correction
                .expect("an orientation"),
            deg(19.999216428),
            1e-10,
        );
        let q = find_point(&outcome, "Q").expect("Q");
        near(q.northing, 1133.968607048, 1e-8);
        near(q.easting, 1064.201350313, 1e-8);
    }

    #[test]
    fn a_check_after_the_block_moves_nothing_when_a_traverse_radiates_again() {
        // The traverse job of a_traverse_adjustment_does_not_replace_a_resection
        // ...: S1 on A (N 1000, E 1000) oriented on R due north, S2 to S4 round
        // B, C and D, and S1 also radiates P 50 m due west, N 1000, E 950. A
        // free station F (N 1050, E 950) reads A (azimuth 135), P (180, 50 m)
        // and R (45) in its block, the file marking its end at record 4, and
        // then P again, 20" high and 10 mm long: a check after the block. After
        // the traverse the side shots are radiated again. P is S1's, from A,
        // which the traverse holds: N 1000, E 950 still. The check is F's and
        // places nothing: radiated from F it would put P at N 1050 + 50.010
        // cos(180 deg + 20") = 999.990000235, E 950 + 50.010 sin(180 deg + 20")
        // = 949.995150894, 11.1 mm off. (Each shot its own pointing, its
        // direction and distance one observation of it.)
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(20.0)));
        project.points.push(point("R", 1100.0, 1000.0, Some(20.0)));
        for id in ["B", "C", "D", "F", "P"] {
            project.unpositioned_points.push(unpositioned(id));
        }
        let mut pointing = 1;
        let mut horizontal = |target: &str, direction: f64, distance: Option<f64>| {
            let mut horizontal = shot(
                target,
                pointing,
                Face::Unknown,
                Some(direction),
                None,
                distance,
                0.0,
            );
            pointing += 1;
            horizontal.kind = DistanceKind::Horizontal;
            horizontal
        };
        let s1 = [
            horizontal("R", 0.0, None),
            horizontal("B", deg(90.0), Some(100.010)),
            horizontal("D", deg(180.0), None),
            horizontal("P", deg(270.0), Some(50.0)),
        ];
        project.stations.push(setup("S1", "A", 1.5, "R", &s1));
        let s2 = [
            horizontal("A", 0.0, None),
            horizontal("C", deg(270.0), Some(100.0)),
        ];
        project.stations.push(setup("S2", "B", 1.5, "A", &s2));
        let s3 = [
            horizontal("B", 0.0, None),
            horizontal("D", deg(270.0), Some(100.0)),
        ];
        project.stations.push(setup("S3", "C", 1.5, "B", &s3));
        let s4 = [
            horizontal("C", 0.0, None),
            horizontal("A", deg(270.0), Some(100.0)),
        ];
        project.stations.push(setup("S4", "D", 1.5, "C", &s4));
        let diagonal = 50.0 * 2.0_f64.sqrt();
        let s5 = [
            horizontal("A", deg(135.0), Some(diagonal)),
            horizontal("P", deg(180.0), Some(50.0)),
            horizontal("R", deg(45.0), Some(diagonal)),
            horizontal("P", deg(180.0) + arc_seconds(20.0), Some(50.010)),
        ];
        let mut free = setup("S5", "F", 1.5, "", &s5);
        free.metadata.insert(
            String::from(RESECTION_END_METADATA),
            String::from("record 4"),
        );
        project.stations.push(free);
        let settings = ReductionSettings {
            method: AdjustmentMethod::Traverse,
            ..bare_settings()
        };
        let outcome = reduce(&project, &settings);
        assert_eq!(outcome.report.resections.len(), 1);
        assert_eq!(outcome.report.resections[0].checks, 1);
        let f = find_point(&outcome, "F").expect("F");
        near(f.northing, 1050.0, 1e-8);
        near(f.easting, 950.0, 1e-8);
        let p = find_point(&outcome, "P").expect("P");
        near(p.northing, 1000.0, 1e-8);
        near(p.easting, 950.0, 1e-8);
        // And the traverse was adjusted: B moved west by its Bowditch share.
        let b = find_point(&outcome, "B").expect("B");
        near(b.easting, 1100.00749981, 1e-8);
    }

    // ---- Precision -----------------------------------------------------------------

    #[test]
    fn a_free_station_with_no_mark_under_it_has_no_marks_centring_in_its_precision() {
        // The shared R with no instrument height (the instrument itself is the
        // point, over no mark) A read 2" high, B 1" low, C exact, all with
        // distances. By the separate script: v'Pv 0.093 on 3 degrees of
        // freedom, the global test not failing above, so the precision is the
        // a-priori one (sN 0.987957 mm, sE 0.734819 mm) with no mark's centring
        // added (which would make sN hypot(0.987957, 1) = 1.405724 mm).
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, arc_seconds(2.0));
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(-1.0));
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        let mut project = marks_and(&[A, B, C], &shots);
        project.stations[0].setup.instrument_height = 0.0;
        let outcome = reduce(&project, &bare_settings());
        assert_eq!(outcome.report.resections.len(), 1);
        near(
            outcome.report.resections[0].sigma_northing,
            0.000987957,
            1e-9,
        );
        near(
            outcome.report.resections[0].sigma_easting,
            0.000734819,
            1e-9,
        );
    }

    #[test]
    fn the_precision_is_scaled_by_the_variance_factor_only_where_the_global_test_fails_above() {
        // The shared R, A read 20" high and B 20" low, C exact, all with
        // distances. By the separate script: R at N 1039.995508816, E 1030, the
        // orientation 20 deg; v'Pv 16.513 on 3 degrees of freedom, above
        // chi-square(3, 0.975) = 9.348, so the residuals are larger than the
        // a-priori weights allow, and the a-priori precision is scaled by the
        // variance factor, 5.504294: sN 2.317720 mm, sE 1.724021 mm, the
        // orientation 6.442423"; with the mark's 1 mm centring (instrument
        // height 1.55), sN 2.524247 and sE 1.993050 mm. The report says the
        // test failed.
        let mut shots = Vec::new();
        let mut index = 1;
        observe(&mut shots, &mut index, &A, true, 50.0, arc_seconds(20.0));
        observe(&mut shots, &mut index, &B, true, 50.0, arc_seconds(-20.0));
        observe(&mut shots, &mut index, &C, true, 50.0, 0.0);
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &bare_settings());
        assert_eq!(outcome.report.resections.len(), 1);
        let resection = &outcome.report.resections[0];
        near(resection.northing, 1039.995508816, 1e-8);
        near(resection.easting, 1030.0, 1e-8);
        let global_test = resection.horizontal.global_test.expect("a global test");
        assert!(!global_test.passed);
        near(
            resection
                .horizontal
                .variance_factor
                .expect("a variance factor"),
            5.504294,
            1e-6,
        );
        near(resection.sigma_northing, 0.002524247, 1e-9);
        near(resection.sigma_easting, 0.001993050, 1e-9);
        near(
            resection.sigma_orientation,
            arc_seconds(6.442423),
            arc_seconds(0.000001),
        );
        assert!(
            katana_survey::reduction_report::render_text(&outcome.report)
                .contains("plan global test FAILED, 16.513 above 9.348")
        );
    }

    #[test]
    fn a_resection_read_with_zeniths_but_no_distances_says_why_it_has_no_height() {
        // A, B and C each read on both faces with a direction and a zenith
        // angle but no distance: the marks have heights and zenith angles, but
        // a trigonometric height needs the distance too, and the warning says
        // so.
        let mut shots = Vec::new();
        let mut index = 1;
        for target in [A, B, C] {
            both_faces(
                &mut shots,
                target.id,
                &mut index,
                Some(normalize_angle(
                    azimuth_from(STATION_NORTHING, STATION_EASTING, &target) - deg(20.0),
                )),
                Some(50.0_f64.atan2(-2.195)),
                None,
                TARGET_HEIGHT,
            );
        }
        let outcome = reduce(&marks_and(&[A, B, C], &shots), &bare_settings());
        assert!(find_point(&outcome, "R").is_some());
        assert_eq!(
            warnings_with(
                &outcome.report,
                "Setup S1: resected with no height: no point it was resected from has a height \
                 and was observed with both a zenith angle and a distance, which a \
                 trigonometric height needs, so nothing radiated from it has one."
            ),
            1,
            "{}",
            all_warnings(&outcome.report)
        );
    }
}
