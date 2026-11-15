use crate::support::near;
use katana_core::ErrorCode;
use katana_geometry::profile::*;
fn pvi(station: f64, elevation: f64, curve_length: f64) -> ProfilePvi {
    ProfilePvi {
        station,
        elevation,
        curve_length,
    }
}
fn sag() -> VerticalAlignment {
    VerticalAlignment {
        pvis: vec![
            pvi(800.0, 106.0, 0.0),
            pvi(1000.0, 100.0, 200.0),
            pvi(1300.0, 106.0, 0.0),
        ],
    }
}
fn crest() -> VerticalAlignment {
    VerticalAlignment {
        pvis: vec![
            pvi(200.0, 138.0, 0.0),
            pvi(500.0, 150.0, 300.0),
            pvi(900.0, 142.0, 0.0),
        ],
    }
}
fn kinds(solved: &SolvedProfile) -> Vec<ProfileElementKind> {
    solved
        .elements()
        .iter()
        .map(|element| element.kind)
        .collect()
}
mod profile {
    use super::*;
    use ProfileElementKind::{Curve, Tangent};
    #[test]
    fn a_sag_curve_matches_the_textbook() {
        let solved = solve_profile(&sag()).unwrap();
        assert_eq!(kinds(&solved), vec![Tangent, Curve, Tangent]);
        assert_eq!(solved.start_station(), 800.0);
        assert_eq!(solved.end_station(), 1300.0);
        near(solved.elevation_at(900.0).unwrap(), 103.0, 1e-12);
        near(solved.elevation_at(1100.0).unwrap(), 102.0, 1e-12);
        near(solved.elevation_at(1020.0).unwrap(), 101.2, 1e-12);
        near(solved.grade_at(850.0).unwrap(), -0.03, 1e-15);
        near(solved.grade_at(1200.0).unwrap(), 0.02, 1e-15);
        near(solved.grade_at(1020.0).unwrap(), 0.0, 1e-15);
        let lows = solved.high_low_points();
        assert_eq!(lows.len(), 1);
        assert!(!lows[0].high);
        near(lows[0].station, 1020.0, 1e-9);
        near(lows[0].elevation, 101.2, 1e-12);
        assert_eq!(lows[0].pvi, 1);
        assert_eq!(solved.key_stations(), vec![800.0, 900.0, 1100.0, 1300.0]);
    }
    #[test]
    fn a_crest_curve_matches_the_textbook() {
        let solved = solve_profile(&crest()).unwrap();
        near(solved.elevation_at(350.0).unwrap(), 144.0, 1e-12);
        near(solved.elevation_at(650.0).unwrap(), 147.0, 1e-12);
        near(solved.elevation_at(550.0).unwrap(), 148.0, 1e-12);
        let highs = solved.high_low_points();
        assert_eq!(highs.len(), 1);
        assert!(highs[0].high);
        near(highs[0].station, 550.0, 1e-9);
        near(highs[0].elevation, 148.0, 1e-12);
    }
    #[test]
    fn every_joint_is_continuous_in_elevation_and_grade() {
        let definition = VerticalAlignment {
            pvis: vec![
                pvi(0.0, 50.0, 0.0),
                pvi(300.0, 59.0, 120.0),
                pvi(700.0, 47.0, 200.0),
                pvi(1000.0, 56.0, 100.0),
                pvi(1200.0, 52.0, 0.0),
            ],
        };
        let solved = solve_profile(&definition).unwrap();
        let joints = solved.key_stations();
        assert!(joints.len() >= 8);
        let mut checked = 0;
        for &station in &joints[1..joints.len() - 1] {
            let step = 1e-6;
            let before = solved.elevation_at(station - step).unwrap();
            let after = solved.elevation_at(station + step).unwrap();
            let before_grade = solved.grade_at(station - step).unwrap();
            let after_grade = solved.grade_at(station + step).unwrap();
            near(after - before, 2.0 * step * before_grade, 1e-9);
            near(after_grade, before_grade, 1e-7);
            checked += 1;
        }
        assert!(checked >= 6);
        let mut highs = 0;
        let mut lows = 0;
        for extremum in solved.high_low_points() {
            if extremum.high {
                highs += 1;
            } else {
                lows += 1;
            }
        }
        assert_eq!(highs, 2);
        assert_eq!(lows, 1);
    }
    #[test]
    fn a_profile_with_no_curves_is_the_grade_line_through_its_pv_is() {
        let definition = VerticalAlignment {
            pvis: vec![
                pvi(0.0, 10.0, 0.0),
                pvi(100.0, 12.0, 0.0),
                pvi(300.0, 8.0, 0.0),
            ],
        };
        let solved = solve_profile(&definition).unwrap();
        assert_eq!(solved.elements().len(), 2);
        near(solved.elevation_at(50.0).unwrap(), 11.0, 1e-12);
        near(solved.elevation_at(200.0).unwrap(), 10.0, 1e-12);
        near(solved.grade_at(50.0).unwrap(), 0.02, 1e-15);
        near(solved.grade_at(200.0).unwrap(), -0.02, 1e-15);
        assert!(solved.high_low_points().is_empty());
        assert!(solved.elevation_at(-1.0).is_none());
        assert!(solved.elevation_at(301.0).is_none());
        assert!(solved.elevation_at(300.0).is_some());
    }
    #[test]
    fn back_to_back_curves_are_allowed_when_the_tangent_is_exactly_used_up() {
        let definition = VerticalAlignment {
            pvis: vec![
                pvi(0.0, 0.0, 0.0),
                pvi(100.0, 5.0, 100.0),
                pvi(200.0, 0.0, 100.0),
                pvi(300.0, 5.0, 0.0),
            ],
        };
        let solved = solve_profile(&definition).unwrap();
        assert_eq!(kinds(&solved), vec![Tangent, Curve, Curve, Tangent]);
        for element in solved.elements() {
            assert!(element.length > 1.0);
        }
        assert_eq!(solved.high_low_points().len(), 2);
    }
    #[test]
    fn refuses_definitions_that_cannot_be_built_and_names_the_pvi() {
        assert_eq!(
            solve_profile(&VerticalAlignment {
                pvis: vec![pvi(0.0, 0.0, 0.0)]
            })
            .unwrap_err()
            .code,
            ErrorCode::InvalidGeometry
        );
        let backwards = solve_profile(&VerticalAlignment {
            pvis: vec![pvi(100.0, 0.0, 0.0), pvi(50.0, 0.0, 0.0)],
        })
        .unwrap_err();
        assert_eq!(backwards.code, ErrorCode::InvalidGeometry);
        assert!(backwards.describe().contains("PVI 0"));
        let at_end = solve_profile(&VerticalAlignment {
            pvis: vec![
                pvi(0.0, 0.0, 0.0),
                pvi(100.0, 5.0, 0.0),
                pvi(200.0, 0.0, 50.0),
            ],
        })
        .unwrap_err();
        assert_eq!(at_end.code, ErrorCode::InvalidGeometry);
        assert!(at_end.describe().contains("PVI 2"));
        let overlapping = solve_profile(&VerticalAlignment {
            pvis: vec![
                pvi(0.0, 0.0, 0.0),
                pvi(100.0, 5.0, 150.0),
                pvi(200.0, 0.0, 150.0),
                pvi(300.0, 5.0, 0.0),
            ],
        })
        .unwrap_err();
        assert_eq!(overlapping.code, ErrorCode::InvalidGeometry);
        assert!(overlapping.describe().contains("PVI 1"));
        assert!(overlapping.describe().contains("PVI 2"));
        assert!(overlapping.describe().contains("short by"));
        assert_eq!(
            solve_profile(&VerticalAlignment {
                pvis: vec![
                    pvi(0.0, 0.0, 0.0),
                    pvi(100.0, 5.0, -10.0),
                    pvi(200.0, 0.0, 0.0)
                ]
            })
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            solve_profile(&VerticalAlignment {
                pvis: vec![pvi(0.0, f64::NAN, 0.0), pvi(100.0, 5.0, 0.0)]
            })
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
    }
}
