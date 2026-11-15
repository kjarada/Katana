//! Twin of tests/survey/test_data_model.cpp.

mod survey_support;

use katana_survey::{
    DistanceObservation, Station, SurveyNetwork, SurveyPoint, SurveyProject, SurveyStation,
};

fn make_point(id: &str, northing: f64, easting: f64, elevation: f64) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation: Some(elevation),
        ..Default::default()
    }
}

fn triangle_network() -> SurveyNetwork {
    let mut network = SurveyNetwork::default();
    assert!(network.add_point(make_point("A", 0.0, 0.0, 0.0)).is_ok());
    assert!(network.add_point(make_point("B", 100.0, 0.0, 0.0)).is_ok());
    assert!(network.add_point(make_point("C", 0.0, 100.0, 0.0)).is_ok());
    network
}

fn distance(from: &str, to: &str, value: f64, sigma: f64) -> DistanceObservation {
    DistanceObservation {
        from: String::from(from),
        to: String::from(to),
        distance: value,
        sigma,
        ..Default::default()
    }
}

fn station(id: &str, point_id: &str, instrument_height: f64) -> Station {
    Station {
        id: String::from(id),
        point_id: String::from(point_id),
        instrument_height,
    }
}

// Two points and one setup over the first of them, backsighting the second.
fn two_point_project() -> SurveyProject {
    SurveyProject {
        points: vec![
            make_point("A", 0.0, 0.0, 10.0),
            make_point("B", 100.0, 0.0, 12.0),
        ],
        stations: vec![SurveyStation {
            setup: station("setup-1", "A", 1.532),
            backsight_point_id: String::from("B"),
            backsight_azimuth: Some(0.0),
            observations: vec![distance("A", "B", 100.0, 0.005).into()],
            ..Default::default()
        }],
        ..Default::default()
    }
}

// ---- points ----------------------------------------------------------------

mod survey_network_points {
    use super::*;
    use crate::survey_support::double_eq;
    use katana_core::ErrorCode;
    use katana_survey::Coordinate2;

    #[test]
    fn add_find_and_keep_insertion_order() {
        let mut network = SurveyNetwork::default();
        let mut point = make_point("STN-7", 5000123.456, 500987.654, 312.5);
        point.code = String::from("IP");
        point.description = String::from("iron pin");
        point
            .metadata
            .insert(String::from("instrument"), String::from("TS16"));
        assert!(network.add_point(point.clone()).is_ok());
        assert!(network.add_point(make_point("A", 1.0, 2.0, 0.0)).is_ok());
        assert!(network.add_point(make_point("M", 3.0, 4.0, 0.0)).is_ok());

        // Insertion order, not alphabetical and not hash order.
        assert_eq!(network.points().len(), 3);
        assert_eq!(network.points()[0].id, "STN-7");
        assert_eq!(network.points()[1].id, "A");
        assert_eq!(network.points()[2].id, "M");

        let found = network.point("STN-7").unwrap();
        // every field survives, including metadata
        assert_eq!(found, point);
        assert_eq!(
            found.position(),
            Coordinate2 {
                northing: 5000123.456,
                easting: 500987.654
            }
        );
        assert_eq!(network.point_index("M"), Some(2));
        assert!(network.contains_point("A"));
        // ids are case sensitive
        assert!(!network.contains_point("a"));
        assert_eq!(
            network.point("missing").unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(network.point_index("missing").is_none());
    }

    #[test]
    fn rejects_duplicate_empty_and_non_finite() {
        let mut network = SurveyNetwork::default();
        assert!(network.add_point(make_point("A", 1.0, 2.0, 0.0)).is_ok());
        assert_eq!(
            network
                .add_point(make_point("A", 9.0, 9.0, 0.0))
                .unwrap_err()
                .code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            network
                .add_point(make_point("", 1.0, 2.0, 0.0))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            network
                .add_point(make_point("B", f64::NAN, 2.0, 0.0))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            network
                .add_point(make_point("B", 1.0, f64::INFINITY, 0.0))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            network
                .add_point(make_point("B", 1.0, 2.0, f64::NAN))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );

        // A rejected call leaves the network as it was.
        assert_eq!(network.points().len(), 1);
        double_eq(network.point("A").unwrap().northing, 1.0);
    }

    #[test]
    fn update_replaces_and_validates() {
        let mut network = triangle_network();
        assert!(
            network
                .update_point(make_point("B", 100.25, -0.5, 7.0))
                .is_ok()
        );
        double_eq(network.point("B").unwrap().northing, 100.25);
        let elevation = network.point("B").unwrap().elevation;
        assert!(elevation.is_some());
        double_eq(elevation.unwrap(), 7.0);
        // position kept
        assert_eq!(network.point_index("B"), Some(1));

        assert_eq!(
            network
                .update_point(make_point("Z", 0.0, 0.0, 0.0))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            network
                .update_point(make_point("B", f64::NAN, 0.0, 0.0))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        double_eq(network.point("B").unwrap().northing, 100.25);
    }

    #[test]
    fn remove_rebuilds_the_index_and_refuses_referenced_points() {
        let mut network = triangle_network();
        assert!(
            network
                .add_observation(distance("B", "C", 141.42, 0.005))
                .is_ok()
        );

        let referenced = network.remove_point("B");
        assert_eq!(referenced.unwrap_err().code, ErrorCode::InvalidState);
        assert_eq!(network.points().len(), 3);

        // "A" is unreferenced; the points after it must still be found afterwards.
        assert!(network.remove_point("A").is_ok());
        assert!(!network.contains_point("A"));
        assert_eq!(network.point_index("B"), Some(0));
        assert_eq!(network.point_index("C"), Some(1));
        double_eq(network.point("C").unwrap().easting, 100.0);

        assert_eq!(
            network.remove_point("A").unwrap_err().code,
            ErrorCode::NotFound
        );

        assert!(network.remove_observation(0).is_ok());
        assert!(network.remove_point("B").is_ok());
    }
}

// ---- observations ----------------------------------------------------------
//
// The C++ spells every field of these observations, the SourceRecord as `{}`:
// they were written here, not imported from anything, so they have no
// provenance. Here the fields not named are their defaults.

mod survey_network_observations {
    use super::*;
    use crate::survey_support::double_eq;
    use crate::survey_support::near;
    use katana_core::ErrorCode;
    use katana_math::numerics::{HALF_PI, PI, TWO_PI};
    use katana_survey::data_model::{
        observation_kind_name, referenced_points, validate_observation,
    };
    use katana_survey::{
        AzimuthObservation, GnssBaselineObservation, GnssPositionObservation,
        HorizontalAngleObservation, LevelDifferenceObservation, Observation,
        VerticalAngleObservation, ZenithAngleObservation,
    };

    fn angle(at: &str, from: &str, to: &str, value: f64, sigma: f64) -> Observation {
        HorizontalAngleObservation {
            at: String::from(at),
            from: String::from(from),
            to: String::from(to),
            angle: value,
            sigma,
            ..Default::default()
        }
        .into()
    }

    fn vertical(from: &str, to: &str, value: f64, sigma: f64, ih: f64, th: f64) -> Observation {
        VerticalAngleObservation {
            from: String::from(from),
            to: String::from(to),
            angle: value,
            sigma,
            instrument_height: ih,
            target_height: th,
            ..Default::default()
        }
        .into()
    }

    fn zenith(from: &str, to: &str, value: f64, sigma: f64, ih: f64, th: f64) -> Observation {
        ZenithAngleObservation {
            from: String::from(from),
            to: String::from(to),
            angle: value,
            sigma,
            instrument_height: ih,
            target_height: th,
            ..Default::default()
        }
        .into()
    }

    fn azimuth(from: &str, to: &str, value: f64, sigma: f64) -> Observation {
        AzimuthObservation {
            from: String::from(from),
            to: String::from(to),
            azimuth: value,
            sigma,
            ..Default::default()
        }
        .into()
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The fields of the C++ brace initialisation, in its order."
    )]
    fn baseline(
        from: &str,
        to: &str,
        d_n: f64,
        d_e: f64,
        d_u: f64,
        s_n: f64,
        s_e: f64,
        s_u: f64,
    ) -> Observation {
        GnssBaselineObservation {
            from: String::from(from),
            to: String::from(to),
            delta_northing: d_n,
            delta_easting: d_e,
            delta_up: d_u,
            sigma_northing: s_n,
            sigma_easting: s_e,
            sigma_up: s_u,
            ..Default::default()
        }
        .into()
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The fields of the C++ brace initialisation, in its order."
    )]
    fn position(
        point: &str,
        northing: f64,
        easting: f64,
        elevation: f64,
        s_n: f64,
        s_e: f64,
        s_h: f64,
    ) -> Observation {
        GnssPositionObservation {
            point: String::from(point),
            northing,
            easting,
            elevation,
            sigma_northing: s_n,
            sigma_easting: s_e,
            sigma_elevation: s_h,
            ..Default::default()
        }
        .into()
    }

    fn level(from: &str, to: &str, rise: f64, sigma: f64, length: f64) -> Observation {
        LevelDifferenceObservation {
            from: String::from(from),
            to: String::from(to),
            height_difference: rise,
            sigma,
            length,
            ..Default::default()
        }
        .into()
    }

    #[test]
    fn add_returns_index_and_stores_every_kind() {
        let mut network = triangle_network();
        let observations: [Observation; 8] = [
            distance("A", "B", 100.0, 0.003).into(),
            angle("A", "B", "C", HALF_PI, 1e-5),
            vertical("A", "B", 0.1, 1e-5, 1.5, 1.8),
            zenith("A", "B", 1.5, 1e-5, 1.5, 1.8),
            azimuth("A", "C", HALF_PI, 2e-5),
            baseline("A", "B", 100.0, 0.0, 1.0, 0.005, 0.005, 0.01),
            position("C", 0.0, 100.0, 5.0, 0.01, 0.01, 0.02),
            level("A", "B", 1.234, 0.001, 250.0),
        ];
        for (i, observation) in observations.iter().enumerate() {
            let index = network.add_observation(observation.clone());
            assert!(index.is_ok(), "{}", observation_kind_name(observation));
            assert_eq!(index.unwrap(), i);
        }
        assert_eq!(network.observations().len(), observations.len());
        assert_eq!(network.observations(), observations.as_slice());

        assert_eq!(
            observation_kind_name(&observations[0]),
            "horizontal distance"
        );
        assert_eq!(observation_kind_name(&observations[7]), "level difference");
        assert_eq!(referenced_points(&observations[1]), ["A", "B", "C"]);
        assert_eq!(referenced_points(&observations[6]), ["C"]);
    }

    #[test]
    fn rejects_unknown_stations() {
        let mut network = triangle_network();
        let result = network.add_observation(distance("A", "NOPE", 10.0, 0.01));
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        // names the culprit
        assert!(error.context.contains("NOPE"));

        assert_eq!(
            network
                .add_observation(angle("A", "B", "X", 1.0, 1e-5))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert!(network.observations().is_empty());
    }

    #[test]
    fn rejects_non_positive_and_non_finite_sigma() {
        let mut network = triangle_network();
        for sigma in [0.0, -0.01, f64::NAN, f64::INFINITY] {
            let result = network.add_observation(distance("A", "B", 100.0, sigma));
            assert!(result.is_err(), "{sigma}");
            assert_eq!(
                result.unwrap_err().code,
                ErrorCode::InvalidSurveyObservation,
                "{sigma}"
            );
        }
        // Each of the three GNSS sigmas is checked on its own.
        assert!(validate_observation(&baseline("A", "B", 1.0, 1.0, 1.0, 0.01, 0.0, 0.01)).is_err());
        assert!(validate_observation(&position("A", 1.0, 1.0, 1.0, 0.01, 0.01, -1.0)).is_err());
        assert!(network.observations().is_empty());
    }

    #[test]
    fn rejects_bad_values() {
        let code =
            |observation: &Observation| validate_observation(observation).err().map(|e| e.code);
        let invalid = Some(ErrorCode::InvalidSurveyObservation);

        // negative distance
        assert_eq!(code(&distance("A", "B", -5.0, 0.01).into()), invalid);
        // zero length line
        assert_eq!(code(&distance("A", "B", 0.0, 0.01).into()), invalid);
        assert_eq!(code(&distance("A", "B", f64::NAN, 0.01).into()), invalid);
        assert_eq!(
            code(&distance("A", "B", f64::INFINITY, 0.01).into()),
            invalid
        );
        // observes itself
        assert_eq!(code(&distance("A", "A", 5.0, 0.01).into()), invalid);
        // empty id
        assert_eq!(code(&distance("", "B", 5.0, 0.01).into()), invalid);

        assert_eq!(code(&angle("A", "B", "B", 1.0, 1e-5)), invalid);
        assert_eq!(code(&angle("A", "A", "B", 1.0, 1e-5)), invalid);
        assert_eq!(code(&angle("A", "B", "C", f64::NAN, 1e-5)), invalid);

        assert_eq!(code(&zenith("A", "B", -0.1, 1e-5, 0.0, 0.0)), invalid);
        assert_eq!(code(&zenith("A", "B", PI + 0.1, 1e-5, 0.0, 0.0)), invalid);
        assert_eq!(code(&zenith("A", "B", HALF_PI, 1e-5, 0.0, 0.0)), None);

        assert_eq!(
            code(&vertical("A", "B", HALF_PI + 0.1, 1e-5, 0.0, 0.0)),
            invalid
        );
        assert_eq!(code(&vertical("A", "B", -HALF_PI, 1e-5, 0.0, 0.0)), None);
        assert_eq!(code(&vertical("A", "B", 0.1, 1e-5, f64::NAN, 0.0)), invalid);

        assert_eq!(code(&azimuth("A", "B", f64::INFINITY, 1e-5)), invalid);
        assert_eq!(code(&level("A", "B", 1.0, 0.001, -1.0)), invalid);
        assert_eq!(code(&level("A", "B", f64::NAN, 0.001, 0.0)), invalid);
        assert_eq!(code(&level("A", "B", -1.0, 0.001, 0.0)), None);
    }

    #[test]
    fn normalises_azimuths_and_angles_on_insertion() {
        let mut network = triangle_network();
        assert!(
            network
                .add_observation(azimuth("A", "B", -HALF_PI, 1e-5))
                .is_ok()
        );
        assert!(
            network
                .add_observation(angle("A", "B", "C", TWO_PI, 1e-5))
                .is_ok()
        );
        assert!(
            network
                .add_observation(angle("A", "B", "C", 5.0 * PI, 1e-5))
                .is_ok()
        );

        // -90° is 270°; a full turn is 0; 900° is 180°.
        let Observation::Azimuth(first) = &network.observations()[0] else {
            panic!("an azimuth was stored");
        };
        near(first.azimuth, 1.5 * PI, 1e-15);
        let Observation::HorizontalAngle(second) = &network.observations()[1] else {
            panic!("an angle was stored");
        };
        double_eq(second.angle, 0.0);
        let Observation::HorizontalAngle(third) = &network.observations()[2] else {
            panic!("an angle was stored");
        };
        near(third.angle, PI, 1e-14);
    }

    #[test]
    fn remove_shifts_indices_and_checks_range() {
        let mut network = triangle_network();
        assert!(
            network
                .add_observation(distance("A", "B", 100.0, 0.01))
                .is_ok()
        );
        assert!(
            network
                .add_observation(distance("A", "C", 100.0, 0.02))
                .is_ok()
        );
        assert!(
            network
                .add_observation(distance("B", "C", 141.0, 0.03))
                .is_ok()
        );

        assert!(network.remove_observation(0).is_ok());
        assert_eq!(network.observations().len(), 2);
        let Observation::Distance(first) = &network.observations()[0] else {
            panic!("a distance was stored");
        };
        double_eq(first.sigma, 0.02);
        assert_eq!(
            network.remove_observation(2).unwrap_err().code,
            ErrorCode::NotFound
        );
    }
}

// ---- control and setups ----------------------------------------------------

mod survey_network_control {
    use super::*;
    use crate::survey_support::double_eq;
    use katana_core::ErrorCode;
    use katana_survey::{ControlConstraint, ControlPoint};

    #[test]
    fn add_validate_remove() {
        let mut network = triangle_network();
        assert!(
            network
                .add_control_point(ControlPoint::fixed_horizontal("A"))
                .is_ok()
        );
        assert!(
            network
                .add_control_point(ControlPoint::weighted_horizontal("B", 0.01, 0.02))
                .is_ok()
        );

        let control = network.control_for("B");
        assert!(control.is_some());
        let control = control.unwrap();
        assert_eq!(control.northing.constraint, ControlConstraint::Weighted);
        double_eq(control.easting.sigma, 0.02);
        assert_eq!(control.elevation.constraint, ControlConstraint::Free);
        assert!(network.control_for("C").is_none());

        assert_eq!(
            network
                .add_control_point(ControlPoint::fixed3d("A"))
                .unwrap_err()
                .code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            network
                .add_control_point(ControlPoint::fixed3d("Z"))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        let nothing = ControlPoint {
            point_id: String::from("C"),
            ..Default::default()
        };
        assert_eq!(
            network.add_control_point(nothing).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        for sigma in [0.0, -0.01, f64::NAN] {
            assert_eq!(
                network
                    .add_control_point(ControlPoint::weighted_vertical("C", sigma))
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidArgument,
                "{sigma}"
            );
        }
        assert_eq!(network.control_points().len(), 2);

        // Control pins the point until it is removed.
        assert_eq!(
            network.remove_point("A").unwrap_err().code,
            ErrorCode::InvalidState
        );
        assert!(network.remove_control_point("A").is_ok());
        assert_eq!(
            network.remove_control_point("A").unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(network.remove_point("A").is_ok());
    }

    #[test]
    fn factories_constrain_the_advertised_components() {
        let vertical = ControlPoint::fixed_vertical("BM");
        assert_eq!(vertical.northing.constraint, ControlConstraint::Free);
        assert_eq!(vertical.easting.constraint, ControlConstraint::Free);
        assert_eq!(vertical.elevation.constraint, ControlConstraint::Fixed);

        let full = ControlPoint::fixed3d("P");
        assert_eq!(full.northing.constraint, ControlConstraint::Fixed);
        assert_eq!(full.easting.constraint, ControlConstraint::Fixed);
        assert_eq!(full.elevation.constraint, ControlConstraint::Fixed);
    }
}

mod survey_network_stations {
    use super::*;
    use crate::survey_support::double_eq;
    use katana_core::ErrorCode;

    #[test]
    fn add_validate_remove() {
        let mut network = triangle_network();
        assert!(network.add_station(station("setup-1", "A", 1.532)).is_ok());
        // re-occupation
        assert!(network.add_station(station("setup-2", "A", 1.601)).is_ok());

        assert_eq!(
            network
                .add_station(station("setup-1", "B", 1.5))
                .unwrap_err()
                .code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            network.add_station(station("", "B", 1.5)).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            network
                .add_station(station("s", "Z", 1.5))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            network
                .add_station(station("s", "B", f64::NAN))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(network.stations().len(), 2);
        double_eq(network.stations()[1].instrument_height, 1.601);

        assert_eq!(
            network.remove_point("A").unwrap_err().code,
            ErrorCode::InvalidState
        );
        assert!(network.remove_station("setup-1").is_ok());
        assert!(network.remove_station("setup-2").is_ok());
        assert_eq!(
            network.remove_station("setup-2").unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(network.remove_point("A").is_ok());
    }
}

mod survey_network_values {
    use super::*;

    #[test]
    fn copies_are_independent() {
        let original = triangle_network();
        let mut copy = original.clone();
        assert!(copy.add_point(make_point("D", 5.0, 5.0, 0.0)).is_ok());
        assert!(copy.remove_point("A").is_ok());

        assert_eq!(original.points().len(), 3);
        assert!(original.contains_point("A"));
        assert!(!original.contains_point("D"));
        assert_eq!(copy.point_index("D"), Some(2));
    }
}

// ---- provenance --------------------------------------------------------------

mod survey_provenance {
    use super::*;
    use crate::survey_support::double_eq;
    use katana_survey::data_model::{
        describe_source, observation_source, set_observation_source, source_file_name, to_string,
        validate_observation,
    };
    use katana_survey::{CoordinateSource, GnssPositionObservation, Observation, SourceRecord};

    #[test]
    fn a_supplied_path_is_reduced_to_its_name_so_a_file_cannot_choose_what_is_read() {
        // The property under test is the security rule stated at
        // source_file_name(): whatever a file says, what comes back is a NAME and
        // can never be walked.
        assert_eq!(source_file_name("job.gsi"), "job.gsi");
        assert_eq!(source_file_name("..\\..\\..\\Windows\\win.ini"), "win.ini");
        assert_eq!(source_file_name("../../etc/passwd"), "passwd");
        assert_eq!(source_file_name("/etc/passwd"), "passwd");
        assert_eq!(source_file_name("C:job.gsi"), "job.gsi");
        // An NTFS alternate data stream: the suffix is a plain name, and the part
        // that made it a path is gone.
        assert_eq!(source_file_name("job.gsi:hidden"), "hidden");

        // Nothing that is not a name comes back as one.
        assert_eq!(source_file_name(""), "");
        assert_eq!(source_file_name("."), "");
        assert_eq!(source_file_name(".."), "");
        assert_eq!(source_file_name("scans/"), "");
        assert_eq!(source_file_name("C:\\"), "");
    }

    #[test]
    fn a_source_record_names_the_file_and_the_record_within_it() {
        let mut source = SourceRecord {
            manufacturer: String::from("Leica"),
            format: String::from("GSI-16"),
            file_name: String::from("job.gsi"),
            record_number: 412,
            ..Default::default()
        };

        assert!(source.known());
        assert_eq!(
            describe_source(&source),
            "job.gsi record 412 (Leica GSI-16)"
        );

        source.format_version = String::from("16 byte words");
        assert_eq!(
            describe_source(&source),
            "job.gsi record 412 (Leica GSI-16 16 byte words)"
        );
    }

    #[test]
    fn a_default_source_record_says_the_value_was_not_imported_rather_than_naming_nothing() {
        let none = SourceRecord::default();
        assert!(!none.known());
        assert_eq!(describe_source(&none), "unknown source");

        // a record number alone identifies nothing
        let record_only = SourceRecord {
            record_number: 7,
            ..Default::default()
        };
        assert!(!record_only.known());
    }

    #[test]
    fn a_calculated_coordinate_is_distinguishable_from_an_observed_one_without_metadata() {
        let mut observed = make_point("STN-1", 100.0, 200.0, 10.0);
        observed.coordinate_source = CoordinateSource::FieldObserved;
        let mut calculated = observed.clone();
        calculated.coordinate_source = CoordinateSource::Calculated;

        // Same coordinates, different fact about them - and the difference
        // survives value comparison, which a metadata convention would not
        // guarantee.
        double_eq(observed.northing, calculated.northing);
        assert_ne!(observed, calculated);
        assert_ne!(
            to_string(observed.coordinate_source),
            to_string(calculated.coordinate_source)
        );
        assert_eq!(to_string(CoordinateSource::Unknown), "unknown");
        assert_eq!(
            make_point("P", 0.0, 0.0, 0.0).coordinate_source,
            CoordinateSource::Unknown
        );
    }

    #[test]
    fn an_observation_carries_its_source_whichever_kind_it_is() {
        let source = SourceRecord {
            format: String::from("GSI-16"),
            file_name: String::from("job.gsi"),
            record_number: 12,
            ..Default::default()
        };

        let mut distance_observation: Observation = distance("A", "B", 100.0, 0.005).into();
        set_observation_source(&mut distance_observation, source.clone());
        assert_eq!(observation_source(&distance_observation), &source);

        let position = GnssPositionObservation {
            point: String::from("A"),
            sigma_northing: 0.01,
            sigma_easting: 0.01,
            sigma_elevation: 0.02,
            ..Default::default()
        };
        let mut gnss: Observation = position.into();
        assert!(!observation_source(&gnss).known());
        set_observation_source(&mut gnss, source);
        assert_eq!(observation_source(&gnss).record_number, 12);

        // Provenance is not part of what makes an observation valid: an
        // observation this program computed has none, and must still validate.
        assert!(validate_observation(&distance_observation).is_ok());
    }
}

// ---- declared coordinate system and units --------------------------------------

mod survey_declared_coordinate_system {
    use katana_survey::DeclaredCoordinateSystem;

    #[test]
    fn a_system_that_the_source_did_not_state_stays_unknown() {
        let nothing_said = DeclaredCoordinateSystem::default();
        assert!(nothing_said.unknown);
        assert!(nothing_said.name.is_empty());
        assert_eq!(nothing_said.epsg_code, 0);

        // A blank declaration declares nothing, and a code of 0 is the field's
        // own "none given" marker rather than an EPSG code.
        assert!(DeclaredCoordinateSystem::named("").unknown);
        assert!(DeclaredCoordinateSystem::epsg(0, "").unknown);
        assert!(DeclaredCoordinateSystem::epsg(-1, "").unknown);
    }

    #[test]
    fn what_the_source_declared_is_recorded_and_not_resolved() {
        let by_name = DeclaredCoordinateSystem::named("MGA Zone 56");
        assert!(!by_name.unknown);
        assert_eq!(by_name.name, "MGA Zone 56");
        // no code was stated, and none is invented
        assert_eq!(by_name.epsg_code, 0);

        let by_code = DeclaredCoordinateSystem::epsg(28356, "MGA Zone 56");
        assert!(!by_code.unknown);
        assert_eq!(by_code.epsg_code, 28356);
        assert_eq!(by_code.name, "MGA Zone 56");
    }
}

mod survey_declared_units {
    use katana_survey::data_model::to_string;
    use katana_survey::{AngularUnit, DeclaredUnits, LinearUnit};

    #[test]
    fn units_are_recorded_as_declared_while_the_model_stays_metres_and_radians() {
        let nothing_said = DeclaredUnits::default();
        assert_eq!(nothing_said.linear, LinearUnit::Unknown);
        assert_eq!(nothing_said.angular, AngularUnit::Unknown);

        assert_eq!(to_string(LinearUnit::UsSurveyFeet), "US survey feet");
        assert_eq!(to_string(AngularUnit::Gons), "gons");
        assert_eq!(
            to_string(AngularUnit::DegreesMinutesSeconds),
            "degrees, minutes and seconds"
        );
    }
}

// ---- stations, features and projects -------------------------------------------

mod survey_stations {
    use super::*;

    #[test]
    fn a_setup_with_no_recorded_orientation_is_not_a_setup_oriented_on_zero() {
        let unrecorded = SurveyStation {
            setup: station("setup-1", "A", 1.532),
            ..Default::default()
        };
        let mut oriented_on_zero = unrecorded.clone();
        oriented_on_zero.backsight_azimuth = Some(0.0);

        assert!(unrecorded.backsight_azimuth.is_none());
        assert_ne!(unrecorded, oriented_on_zero);
    }
}

mod survey_projects {
    use super::*;
    use katana_core::ErrorCode;
    use katana_math::numerics::PI;
    use katana_survey::data_model::validate_project;
    use katana_survey::{SurveyFeature, Traverse, TraverseKind, TraverseSetup};

    #[test]
    fn a_project_whose_references_all_resolve_validates() {
        let mut project = two_point_project();
        project.features.push(SurveyFeature {
            code: String::from("KB"),
            point_ids: vec![String::from("A"), String::from("B")],
            ..Default::default()
        });
        project
            .observations
            .push(distance("B", "A", 100.0, 0.005).into());

        assert!(validate_project(&project).is_ok());
    }

    #[test]
    fn a_point_with_no_id_or_a_duplicate_one_is_rejected() {
        let empty = SurveyProject {
            points: vec![make_point("", 0.0, 0.0, 0.0)],
            ..Default::default()
        };
        assert_eq!(
            validate_project(&empty).unwrap_err().code,
            ErrorCode::InvalidArgument
        );

        let twice = SurveyProject {
            points: vec![
                make_point("A", 0.0, 0.0, 0.0),
                make_point("A", 1.0, 1.0, 0.0),
            ],
            ..Default::default()
        };
        assert_eq!(
            validate_project(&twice).unwrap_err().code,
            ErrorCode::AlreadyExists
        );
    }

    #[test]
    fn a_point_with_a_non_finite_coordinate_is_rejected() {
        let project = SurveyProject {
            points: vec![make_point("A", f64::NAN, 0.0, 0.0)],
            ..Default::default()
        };
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::InvalidArgument
        );

        let infinite = SurveyProject {
            points: vec![make_point("A", 0.0, 0.0, f64::INFINITY)],
            ..Default::default()
        };
        assert_eq!(
            validate_project(&infinite).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_setup_over_a_point_that_is_not_in_the_project_is_rejected() {
        let mut project = two_point_project();
        project.stations[0].setup.point_id = String::from("Z");
        let error = validate_project(&project).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.message.contains("'Z'"));
    }

    #[test]
    fn a_backsight_point_that_is_not_in_the_project_is_rejected() {
        let mut project = two_point_project();
        project.stations[0].backsight_point_id = String::from("Z");
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::NotFound
        );

        // No backsight at all is not an error: a free setup records none.
        project.stations[0].backsight_point_id.clear();
        assert!(validate_project(&project).is_ok());
    }

    #[test]
    fn a_stated_backsight_azimuth_that_is_not_finite_is_rejected_and_a_finite_one_is_kept_apart() {
        // The azimuth a file states for the backsight and the circle set on it
        // are two values: a circle zeroed on a backsight 123 degrees away is the
        // usual case, and the two must not compare equal as one field.
        let mut project = two_point_project();
        project.stations[0].stated_backsight_azimuth = Some(123.0 * PI / 180.0);
        assert!(validate_project(&project).is_ok());
        assert_ne!(project.stations[0], two_point_project().stations[0]);
        for bad in [f64::NAN, f64::INFINITY] {
            project.stations[0].stated_backsight_azimuth = Some(bad);
            let error = validate_project(&project).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(
                error.message.contains("azimuth the file states"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn duplicate_or_unnamed_setups_are_rejected() {
        let mut project = two_point_project();
        let first = project.stations[0].clone();
        project.stations.push(first);
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::AlreadyExists
        );

        project.stations[1].setup.id.clear();
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn an_observation_taken_from_a_setup_is_validated_like_any_other() {
        let mut bad_value = two_point_project();
        bad_value.stations[0]
            .observations
            .push(distance("A", "B", 100.0, 0.0).into());
        assert_eq!(
            validate_project(&bad_value).unwrap_err().code,
            ErrorCode::InvalidSurveyObservation
        );

        let mut unknown_point = two_point_project();
        unknown_point.stations[0]
            .observations
            .push(distance("A", "Z", 100.0, 0.005).into());
        assert_eq!(
            validate_project(&unknown_point).unwrap_err().code,
            ErrorCode::NotFound
        );

        let mut loose = two_point_project();
        loose
            .observations
            .push(distance("A", "Z", 100.0, 0.005).into());
        assert_eq!(
            validate_project(&loose).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_feature_must_name_at_least_one_point_and_only_points_that_exist() {
        let mut no_points = two_point_project();
        no_points.features.push(SurveyFeature {
            code: String::from("KB"),
            ..Default::default()
        });
        assert_eq!(
            validate_project(&no_points).unwrap_err().code,
            ErrorCode::InvalidArgument
        );

        let mut dangling = two_point_project();
        dangling.features.push(SurveyFeature {
            name: String::from("KB1"),
            code: String::from("KB"),
            point_ids: vec![String::from("A"), String::from("Z")],
            ..Default::default()
        });
        let error = validate_project(&dangling).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.context.contains("KB1"));
    }

    #[test]
    fn a_traverse_is_not_checked_against_the_project_points_because_it_is_self_contained() {
        // A Traverse carries its own start and end coordinates, so its station
        // ids are names of its own. Requiring them to be project points would
        // reject a perfectly good traverse read from a file that lists no
        // coordinates.
        let mut project = two_point_project();
        project.traverses.push(Traverse {
            name: String::from("TR1"),
            kind: TraverseKind::Open,
            start_azimuth: 0.0,
            setups: vec![TraverseSetup {
                station_id: String::from("not-a-project-point"),
                angle: 0.0,
                distance: 50.0,
            }],
            end_station_id: String::from("also-not-one"),
            ..Default::default()
        });

        assert!(validate_project(&project).is_ok());
    }
}
