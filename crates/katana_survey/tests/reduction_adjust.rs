//! Twin of tests/survey/test_reduction_adjust.cpp: the adjustments
//! reduce_and_adjust drives: a traverse found in the setups, a network with a
//! planted blunder, and a published level-net example.

mod reduction_support;
mod survey_support;

use katana_math::numerics::normalize_angle;
use katana_survey::data_model::{
    ControlPoint, CoordinateSource, DistanceKind, Face, LevelDifferenceObservation, Observation,
    SurveyProject,
};
use katana_survey::reduction::{ReductionContext, ReductionOutcome, reduce_and_adjust};
use katana_survey::reduction_report::{MisclosureReport, ReductionReport};
use katana_survey::reduction_settings::{
    AdjustmentMethod, ControlSelection, NetworkDimension, OutlierTest, ReductionSettings,
};
use reduction_support::{Shot, arc_seconds, bare_settings, deg, point, point_from, setup, shot};
use reduction_support::{succeeded, unpositioned};

fn reduce(project: &SurveyProject, settings: &ReductionSettings) -> ReductionOutcome {
    succeeded(reduce_and_adjust(
        project,
        settings,
        &ReductionContext::default(),
    ))
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

// A 100 m square loop A -> B -> C -> D -> A, oriented on R due north of A:
//   A (N 1000, E 1000) known   B (1000, 1100)   C (900, 1100)   D (900, 1000)
// Readings are the azimuths less each setup's circle orientation; the leg
// A -> B is measured 100.010 (a planted +10 mm), every other leg 100.000,
// every angle exact. S1 also reads D, which closes the loop on R.
fn square_loop() -> SurveyProject {
    let mut project = SurveyProject::default();
    project.points.push(point("A", 1000.0, 1000.0, Some(20.0)));
    project.points.push(point("R", 1100.0, 1000.0, Some(20.0)));
    for id in ["B", "C", "D"] {
        project.unpositioned_points.push(unpositioned(id));
    }
    // At A the circle reads azimuths; at B it reads azimuth - 270; at C
    // azimuth - 0; at D azimuth - 90.
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
    project
}

fn find_misclosure<'r>(report: &'r ReductionReport, prefix: &str) -> Option<&'r MisclosureReport> {
    report
        .misclosures
        .iter()
        .find(|misclosure| misclosure.name.starts_with(prefix))
}

mod reduction_traverse {
    use super::*;
    use crate::reduction_support::find_point;
    use crate::survey_support::near;
    use katana_survey::reduction_report::ComputationMethod;
    use katana_survey::reduction_settings::TraverseRule;

    #[test]
    fn a_closed_loop_shows_the_planted_misclosure_and_bowditch_distributes_it() {
        // Latitudes 0, -100, 0, +100 close exactly; departures +100.010, 0, -100,
        // 0 leave +0.010 m east. Linear misclosure 10 mm over 400.010 m: 1 : 40001.
        // Angles: carried round the loop the azimuth of D -> A arrives at 0, and
        // the closing angle at A (D -> R) is 180, which gives 0 against R's
        // azimuth of 0: no angular misclosure.
        // Bowditch: each leg's departure correction is -0.010 * length / 400.010:
        //   A -> B -0.00250019, the others -0.00249994 each, so
        //   B E = 1100.010 - 0.00250019                    = 1100.00749981
        //   C E = 1100.00749981 - 0.00249994               = 1100.00499988
        //   D E = 1100.00499988 - 100 - 0.00249994         = 1000.00249994
        let settings = ReductionSettings {
            method: AdjustmentMethod::Traverse,
            ..bare_settings()
        };
        let outcome = reduce(&square_loop(), &settings);
        let misclosure =
            find_misclosure(&outcome.report, "traverse from A").expect("the traverse's misclosure");
        near(
            misclosure.angular.expect("an angular misclosure"),
            0.0,
            1e-12,
        );
        near(misclosure.northing.unwrap(), 0.0, 1e-9);
        near(misclosure.easting.unwrap(), 0.010, 1e-9);
        near(misclosure.linear.unwrap(), 0.010, 1e-9);
        near(misclosure.length, 400.010, 1e-9);
        near(misclosure.precision_ratio.unwrap(), 40001.0, 0.01);

        let b = find_point(&outcome, "B").expect("B");
        let c = find_point(&outcome, "C").expect("C");
        let d = find_point(&outcome, "D").expect("D");
        near(b.easting, 1100.00749981, 1e-8);
        near(b.northing, 1000.0, 1e-9);
        near(c.easting, 1100.00499988, 1e-8);
        near(c.northing, 900.0, 1e-9);
        near(d.easting, 1000.00249994, 1e-8);
        assert_eq!(b.method, ComputationMethod::TraverseBowditch);
        assert_eq!(outcome.report.adjustments.len(), 1);
        assert_eq!(outcome.report.adjustments[0].redundancy, 3);
    }

    #[test]
    fn the_transit_rule_puts_the_whole_departure_misclosure_on_the_east_west_legs() {
        // Transit: departure corrections in proportion to |departure|: A -> B
        // takes 100.010 / 200.010 of -0.010 and C -> D 100 / 200.010; the
        // north-south legs take none. B E = 1100.010 - 0.00500025 = 1100.00499975.
        let settings = ReductionSettings {
            method: AdjustmentMethod::Traverse,
            traverse_rule: TraverseRule::Transit,
            ..bare_settings()
        };
        let outcome = reduce(&square_loop(), &settings);
        let b = find_point(&outcome, "B").expect("B");
        near(b.easting, 1100.00499975, 1e-8);
        assert_eq!(b.method, ComputationMethod::TraverseTransit);
    }

    #[test]
    fn the_least_squares_rule_reports_its_statistics() {
        let settings = ReductionSettings {
            method: AdjustmentMethod::Traverse,
            traverse_rule: TraverseRule::LeastSquares,
            ..bare_settings()
        };
        let outcome = reduce(&square_loop(), &settings);
        assert_eq!(outcome.report.adjustments.len(), 1);
        let adjustment = &outcome.report.adjustments[0];
        // 3 new stations (6 unknowns); 4 distances + 5 angles = 9 observations.
        assert_eq!(adjustment.unknowns, 6);
        assert_eq!(adjustment.redundancy, 3);
        assert!(adjustment.variance_factor.is_some());
        assert!(!adjustment.ellipses.is_empty());
        assert_eq!(
            find_point(&outcome, "B").expect("B").method,
            ComputationMethod::TraverseLeastSquares
        );
    }
}

// A braced quadrilateral: A (1000, 1000) and B (1000, 1200) held; P (1150,
// 1100) and Q (850, 1100) to be found. Every setup reads every other point
// (direction and horizontal distance), except that P does not measure back
// to B, so the line B -> P is measured once. Each value carries a small
// alternating error (+-0.5 mm, +-1"), as real observations do; B -> P
// carries 30 mm more.
fn quadrilateral(blunder: f64) -> SurveyProject {
    let marks: [(&str, f64, f64); 4] = [
        ("A", 1000.0, 1000.0),
        ("B", 1000.0, 1200.0),
        ("P", 1150.0, 1100.0),
        ("Q", 850.0, 1100.0),
    ];
    let circle = [0.0, deg(30.0), deg(100.0), deg(200.0)];
    let backsight = ["B", "A", "A", "A"];
    let mut project = SurveyProject::default();
    project.points.push(point("A", 1000.0, 1000.0, None));
    project.points.push(point("B", 1000.0, 1200.0, None));
    project.unpositioned_points.push(unpositioned("P"));
    project.unpositioned_points.push(unpositioned("Q"));
    let mut sign = 1.0;
    for (s, &(at, n, e)) in marks.iter().enumerate() {
        let mut shots = Vec::new();
        for (t, &(to, tn, te)) in marks.iter().enumerate() {
            if t == s {
                continue;
            }
            let dn = tn - n;
            let de = te - e;
            let azimuth = de.atan2(dn);
            let direction = normalize_angle(azimuth - circle[s] + sign * arc_seconds(1.0));
            let mut distance = Some(dn.hypot(de) + sign * 0.0005);
            sign = -sign;
            if at == "B" && to == "P" {
                distance = distance.map(|distance| distance + blunder);
            }
            if at == "P" && to == "B" {
                distance = None;
            }
            let mut horizontal = shot(to, 0, Face::Unknown, Some(direction), None, distance, 0.0);
            horizontal.kind = DistanceKind::Horizontal;
            shots.push(horizontal);
        }
        project
            .stations
            .push(setup(&format!("S{at}"), at, 1.5, backsight[s], &shots));
    }
    project
}

fn network_settings(test: OutlierTest, auto_reject: bool) -> ReductionSettings {
    ReductionSettings {
        method: AdjustmentMethod::Network,
        network_dimension: NetworkDimension::Horizontal,
        control: vec![
            ControlSelection {
                point: ControlPoint::fixed_horizontal("A"),
                ..Default::default()
            },
            ControlSelection {
                point: ControlPoint::fixed_horizontal("B"),
                ..Default::default()
            },
        ],
        outlier_test: test,
        auto_reject_outliers: auto_reject,
        ..bare_settings()
    }
}

mod reduction_network {
    use super::*;
    use crate::reduction_support::{find_point, find_row};
    use crate::survey_support::near;
    use katana_core::ErrorCode;
    use katana_survey::reduction_report::ComputationMethod;

    #[test]
    fn the_planted_blunder_has_the_largest_tau_and_is_flagged() {
        // 8 angles + 11 distances = 19 observations, 4 unknowns: redundancy 15.
        // Pope's critical tau at alpha 0.001 for r = 15: t(14, 0.9995) = 4.140
        // (Student's t table), tau = 4.140 sqrt(15 / (14 + 4.140^2)) = 2.873.
        let outcome = reduce(
            &quadrilateral(0.030),
            &network_settings(OutlierTest::Tau, false),
        );
        assert_eq!(outcome.report.adjustments.len(), 1);
        let adjustment = &outcome.report.adjustments[0];
        assert_eq!(adjustment.observations, 19);
        assert_eq!(adjustment.unknowns, 4);
        assert_eq!(adjustment.redundancy, 15);
        let global_test = adjustment.global_test.expect("a global test");
        assert!(!global_test.passed); // 30 mm against 2.4 mm sigmas

        let mut worst: Option<&katana_survey::reduction_report::ReportResidual> = None;
        for residual in &adjustment.residuals {
            if let Some(standardised) = residual.standardised
                && worst.is_none_or(|worst| {
                    standardised.abs() > worst.standardised.unwrap_or(0.0).abs()
                })
            {
                worst = Some(residual);
            }
        }
        let worst = worst.expect("a standardised residual");
        assert_eq!(worst.observation, "distance at SB: P");
        assert!(worst.flagged);
        assert!(worst.standardised.unwrap().abs() > 2.873);
        let named_critical = outcome
            .report
            .warnings
            .iter()
            .any(|warning| warning.text.contains("tau, critical 2.87"));
        assert!(named_critical);
    }

    #[test]
    fn auto_reject_removes_the_blunder_first_and_logs_it() {
        // Baarda's critical w at alpha 0.001 is the normal quantile 3.2905.
        let outcome = reduce(
            &quadrilateral(0.030),
            &network_settings(OutlierTest::Baarda, true),
        );
        let adjustment = &outcome.report.adjustments[0];
        assert!(!adjustment.rejected_outliers.is_empty());
        assert_eq!(adjustment.rejected_outliers[0], "distance at SB: P");
        assert_eq!(adjustment.observations, 18);
        // the second distance to P is B's
        let row = find_row(&outcome.report, "horizontal distance", "P", 1).expect("B's distance");
        assert_eq!(row.station_id, "SB");
        assert!(row.rejected);
        assert!(row.rejection_reason.contains("Baarda w, critical 3.29"));
        // Without the blunder P lands within a millimetre of where it is.
        let p = find_point(&outcome, "P").expect("P");
        near(p.northing, 1150.0, 0.001);
        near(p.easting, 1100.0, 0.001);
        assert_eq!(p.method, ComputationMethod::NetworkLeastSquares);
        assert!(p.sigma_northing.expect("a precision") > 0.0);
        assert!(!adjustment.ellipses.is_empty());
    }

    #[test]
    fn without_control_the_network_is_refused_with_the_datum_hint() {
        let mut settings = network_settings(OutlierTest::Baarda, false);
        settings.control.clear();
        let outcome =
            reduce_and_adjust(&quadrilateral(0.0), &settings, &ReductionContext::default());
        assert_eq!(
            outcome.expect_err("no control").code,
            ErrorCode::AdjustmentFailure
        );
    }

    #[test]
    fn side_shots_are_radiated_from_the_adjusted_stations_not_adjusted() {
        let mut project = quadrilateral(0.0);
        // A side shot from P: 10 m due north of P, reading = azimuth - 100 deg.
        project.unpositioned_points.push(unpositioned("X"));
        let mut side = shot(
            "X",
            0,
            Face::Unknown,
            Some(normalize_angle(-deg(100.0))),
            None,
            Some(10.0),
            0.0,
        );
        side.kind = DistanceKind::Horizontal;
        let extra = setup("tmp", "P", 1.5, "A", &[side]);
        project.stations[2].observations.extend(extra.observations);
        let outcome = reduce(&project, &network_settings(OutlierTest::Baarda, false));
        // Not in the adjustment: the observation count is unchanged.
        assert_eq!(outcome.report.adjustments[0].observations, 19);
        let p = find_point(&outcome, "P").expect("P");
        let x = find_point(&outcome, "X").expect("X");
        assert_eq!(x.method, ComputationMethod::Radiation);
        // From the adjusted P (not the radiated one), within the noise of the
        // orientation: 10 m at a few seconds is well under a millimetre.
        near(x.northing - p.northing, 10.0, 0.001);
        near(x.easting - p.easting, 0.0, 0.001);
    }

    #[test]
    fn the_level_net_of_ghilani_and_wolf_example12_1_adjusts_to_the_published_heights() {
        // Ghilani & Wolf, Adjustment Computations: Spatial Data Analysis, 4th ed.
        // (Wiley 2006), section 12.6, Example 12.1: A held at 437.596 m, six
        // levelled differences with their standard deviations. Published:
        //   B = 448.1087, C = 453.4685, D = 444.9436 (to 0.1 mm)
        //   v^T W v = 1.26976 from the residuals rounded to 0.1 mm, r = 6 - 3 = 3,
        //   so the variance factor is 1.26976 / 3 = 0.4233. (Unrounded, v^T W v
        //   is 1.27212 and the factor 0.4240; the book's printed S0 of 0.6575 is
        //   not the square root of either (sqrt(0.4233) = 0.6506) and is taken
        //   to be a misprint.)
        let mut project = SurveyProject::default();
        project.points.push(point("A", 0.0, 0.0, Some(437.596)));
        project
            .points
            .push(point_from("B", 10.0, 0.0, None, CoordinateSource::Unknown));
        project
            .points
            .push(point_from("C", 10.0, 10.0, None, CoordinateSource::Unknown));
        project
            .points
            .push(point_from("D", 0.0, 10.0, None, CoordinateSource::Unknown));
        let level = |from: &str, to: &str, difference: f64, sigma: f64| {
            Observation::LevelDifference(LevelDifferenceObservation {
                from: String::from(from),
                to: String::from(to),
                height_difference: difference,
                sigma,
                length: 0.0,
                source: Default::default(),
            })
        };
        project.observations.push(level("A", "B", 10.509, 0.006));
        project.observations.push(level("B", "C", 5.360, 0.004));
        project.observations.push(level("C", "D", -8.523, 0.005));
        project.observations.push(level("D", "A", -7.348, 0.003));
        project.observations.push(level("B", "D", -3.167, 0.004));
        project.observations.push(level("A", "C", 15.881, 0.012));

        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            network_dimension: NetworkDimension::Levels,
            control: vec![ControlSelection {
                point: ControlPoint::fixed_vertical("A"),
                ..Default::default()
            }],
            ..Default::default()
        };
        let outcome = reduce(&project, &settings);
        assert_eq!(outcome.report.adjustments.len(), 1);
        let adjustment = &outcome.report.adjustments[0];
        assert_eq!(adjustment.redundancy, 3);
        near(
            adjustment.variance_factor.expect("a variance factor"),
            1.26976 / 3.0,
            0.001,
        );

        let b = find_point(&outcome, "B").expect("B");
        let c = find_point(&outcome, "C").expect("C");
        let d = find_point(&outcome, "D").expect("D");
        near(b.elevation.unwrap(), 448.1087, 0.00006);
        near(c.elevation.unwrap(), 453.4685, 0.00006);
        near(d.elevation.unwrap(), 444.9436, 0.00006);
        assert_eq!(b.method, ComputationMethod::NetworkLeastSquares);
        // A levelled height on the file's own horizontal position.
        assert_eq!(b.northing, 10.0);
    }
}
