//! Twin of tests/survey/test_raw_observations.cpp.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod survey_support;

mod raw_observations {
    use crate::survey_support::near;
    use katana_core::ErrorCode;
    use katana_math::numerics::{HALF_PI, PI};
    use katana_survey::data_model::{
        normalized_observation, observation_kind_name, observation_pointing, referenced_points,
        to_string, validate_observation, validate_project,
    };
    use katana_survey::network_adjustment::adjust_horizontal_network;
    use katana_survey::{
        AdjustmentOptions, AntennaHeightMethod, AzimuthObservation, ControlPoint, CorrectionState,
        DistanceKind, DistanceObservation, Face, GeocentricCoordinate, GeodeticCoordinate,
        GnssCovariance3, GnssGeocentricBaselineObservation, GnssGlobalPositionObservation,
        GnssSolution, HorizontalDirectionObservation, InstrumentSettings,
        LevelDifferenceObservation, Observation, Pointing, Station, SurveyNetwork, SurveyPoint,
        SurveyProject, SurveyStation, SurveyTimestamp, TargetInfo, UnpositionedPoint,
        ZenithAngleObservation,
    };

    fn positioned(id: &str, northing: f64, easting: f64) -> SurveyPoint {
        SurveyPoint {
            id: String::from(id),
            northing,
            easting,
            ..Default::default()
        }
    }

    fn unpositioned(id: &str) -> UnpositionedPoint {
        UnpositionedPoint {
            id: String::from(id),
            ..Default::default()
        }
    }

    #[test]
    fn new_members_default_to_not_stated() {
        let pointing = Pointing::default();
        assert_eq!(pointing.index, 0);
        assert_eq!(pointing.face, Face::Unknown);

        let target = TargetInfo::default();
        assert!(target.prism_constant.is_none());
        assert_eq!(target.prism_constant_state, CorrectionState::Unknown);

        let instrument = InstrumentSettings::default();
        assert!(instrument.prism_constant.is_none());
        assert!(instrument.atmospheric_ppm.is_none());
        assert_eq!(instrument.atmospheric_ppm_state, CorrectionState::Unknown);
        assert_eq!(instrument.scale_factor_state, CorrectionState::Unknown);
        assert!(!instrument.time.known());

        let station = SurveyStation::default();
        assert_eq!(station.instrument, InstrumentSettings::default());

        let project = SurveyProject::default();
        assert!(project.control_points.is_empty());
        assert!(project.gnss_sessions.is_empty());
    }

    #[test]
    fn an_aggregate_written_before_the_pointing_existed_still_means_the_same() {
        // The C++ initialises these positionally, as every test and importer did
        // before Pointing and TargetInfo were added after `source`; here the
        // fields not named are their defaults, which is the property itself.
        let distance = DistanceObservation {
            from: String::from("A"),
            to: String::from("B"),
            distance: 10.0,
            sigma: 0.002,
            kind: DistanceKind::Slope,
            instrument_height: 1.5,
            target_height: 1.6,
            ..Default::default()
        };
        assert_eq!(distance.pointing, Pointing::default());
        assert_eq!(distance.target, TargetInfo::default());
        assert!(validate_observation(&distance.into()).is_ok());
    }

    #[test]
    fn a_horizontal_direction_is_a_raw_circle_reading_at_one_setup() {
        let mut direction = HorizontalDirectionObservation {
            at: String::from("S1"),
            to: String::from("P7"),
            // a reading given as -90 degrees
            direction: -HALF_PI,
            sigma: 1e-5,
            pointing: Pointing {
                index: 3,
                face: Face::Right,
            },
            ..Default::default()
        };
        let observation: Observation = direction.clone().into();

        assert_eq!(observation_kind_name(&observation), "horizontal direction");
        assert_eq!(referenced_points(&observation), ["S1", "P7"]);
        assert!(validate_observation(&observation).is_ok());
        // -pi/2 wrapped into [0, 2 pi) is 3 pi / 2.
        let normalised = normalized_observation(observation.clone());
        let Observation::HorizontalDirection(wrapped) = &normalised else {
            panic!("a direction stays a direction");
        };
        near(wrapped.direction, 1.5 * PI, 1e-15);
        assert!(observation_pointing(&observation).is_some());
        assert_eq!(observation_pointing(&observation).unwrap().index, 3);
        assert_eq!(
            observation_pointing(&observation).unwrap().face,
            Face::Right
        );

        direction.sigma = 0.0;
        assert_eq!(
            validate_observation(&direction.into()).unwrap_err().code,
            ErrorCode::InvalidSurveyObservation
        );
    }

    #[test]
    fn only_total_station_observations_belong_to_a_pointing() {
        let azimuth: Observation = AzimuthObservation {
            from: String::from("A"),
            to: String::from("B"),
            azimuth: 1.0,
            sigma: 1e-5,
            ..Default::default()
        }
        .into();
        assert!(observation_pointing(&azimuth).is_none());
        let level: Observation = LevelDifferenceObservation {
            from: String::from("A"),
            to: String::from("B"),
            height_difference: 1.0,
            sigma: 0.001,
            length: 0.0,
            ..Default::default()
        }
        .into();
        assert!(observation_pointing(&level).is_none());
        let zenith: Observation = ZenithAngleObservation {
            from: String::from("A"),
            to: String::from("B"),
            angle: 1.5,
            sigma: 1e-5,
            instrument_height: 1.5,
            target_height: 1.6,
            ..Default::default()
        }
        .into();
        assert!(observation_pointing(&zenith).is_some());
    }

    #[test]
    fn a_gnss_global_position_is_either_geocentric_or_geodetic_never_both() {
        let mut position = GnssGlobalPositionObservation {
            point: String::from("BASE"),
            ..Default::default()
        };
        // neither
        assert!(validate_observation(&position.clone().into()).is_err());

        position.geocentric = Some(GeocentricCoordinate {
            x: -4052051.0,
            y: 4212836.0,
            z: -2545106.0,
        });
        assert!(validate_observation(&position.clone().into()).is_ok());
        assert_eq!(
            observation_kind_name(&position.clone().into()),
            "GNSS geocentric position"
        );
        assert_eq!(referenced_points(&position.clone().into()), ["BASE"]);

        position.geodetic = Some(GeodeticCoordinate {
            latitude: -0.5,
            longitude: 2.3,
            ellipsoidal_height: 40.0,
        });
        // both
        assert!(validate_observation(&position.clone().into()).is_err());

        position.geocentric = None;
        assert!(validate_observation(&position.clone().into()).is_ok());
        // beyond a pole
        if let Some(geodetic) = position.geodetic.as_mut() {
            geodetic.latitude = HALF_PI + 0.01;
        }
        assert!(validate_observation(&position.into()).is_err());
    }

    #[test]
    fn a_gnss_covariance_is_optional_but_when_stated_its_variances_are_positive() {
        let mut baseline = GnssGeocentricBaselineObservation {
            from: String::from("BASE"),
            to: String::from("R1"),
            delta: GeocentricCoordinate {
                x: 120.0,
                y: -35.5,
                z: 8.25,
            },
            ..Default::default()
        };
        // no covariance stated
        assert!(validate_observation(&baseline.clone().into()).is_ok());
        assert_eq!(referenced_points(&baseline.clone().into()), ["BASE", "R1"]);

        // stated, with a zero diagonal
        baseline.covariance.xy = 1e-6;
        assert!(validate_observation(&baseline.clone().into()).is_err());
        baseline.covariance = GnssCovariance3 {
            xx: 4e-6,
            yy: 4e-6,
            zz: 9e-6,
            xy: 1e-6,
            xz: 0.0,
            yz: 0.0,
        };
        assert!(validate_observation(&baseline.into()).is_ok());
    }

    #[test]
    fn control_named_by_a_file_must_be_a_positioned_point_of_the_project() {
        let mut project = SurveyProject {
            points: vec![positioned("CP1", 1000.0, 2000.0)],
            unpositioned_points: vec![unpositioned("P9")],
            control_points: vec![ControlPoint::fixed_horizontal("CP1")],
            ..Default::default()
        };
        assert!(validate_project(&project).is_ok());

        project
            .control_points
            .push(ControlPoint::fixed_horizontal("P9"));
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::NotFound
        );

        project.control_points = vec![ControlPoint::weighted_vertical("CP1", 0.0)];
        assert_eq!(
            validate_project(&project).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_station_with_raw_directions_and_gnss_passes_project_validation() {
        let mut station = SurveyStation {
            setup: Station {
                id: String::from("SETUP1"),
                point_id: String::from("S1"),
                instrument_height: 1.55,
            },
            ..Default::default()
        };
        station.instrument.make = String::from("Leica");
        station.instrument.atmospheric_ppm_state = CorrectionState::Applied;
        station.observations = vec![
            HorizontalDirectionObservation {
                at: String::from("S1"),
                to: String::from("P1"),
                direction: 0.25,
                sigma: 1e-5,
                pointing: Pointing {
                    index: 1,
                    face: Face::Left,
                },
                ..Default::default()
            }
            .into(),
            ZenithAngleObservation {
                from: String::from("S1"),
                to: String::from("P1"),
                angle: 1.6,
                sigma: 1e-5,
                instrument_height: 1.55,
                target_height: 1.8,
                pointing: Pointing {
                    index: 1,
                    face: Face::Left,
                },
                ..Default::default()
            }
            .into(),
        ];
        let base = GnssGlobalPositionObservation {
            point: String::from("S1"),
            geodetic: Some(GeodeticCoordinate {
                latitude: -0.59,
                longitude: 2.64,
                ellipsoidal_height: 45.0,
            }),
            ..Default::default()
        };
        let project = SurveyProject {
            points: vec![positioned("S1", 0.0, 0.0)],
            unpositioned_points: vec![unpositioned("P1")],
            stations: vec![station],
            observations: vec![base.into()],
            ..Default::default()
        };
        let status = validate_project(&project);
        assert!(status.is_ok(), "{:?}", status.err().map(|e| e.describe()));
    }

    #[test]
    fn the_horizontal_adjustment_sets_a_global_gnss_value_aside_and_takes_a_direction() {
        // A global GNSS value needs the reduction's conversion to grid first; an
        // adjustment handed one must set it aside and say so, not crash. A
        // direction it takes, as one of the set read at A with an orientation
        // unknown of its own (network_adjustment.rs) - here that unknown and the
        // reading add nothing else. B is due north of A, so the reading 0.3
        // there turns into an orientation of 0 - 0.3 = 2 pi - 0.3.
        let mut network = SurveyNetwork::default();
        assert!(network.add_point(positioned("A", 0.0, 0.0)).is_ok());
        assert!(network.add_point(positioned("B", 100.0, 0.0)).is_ok());
        assert!(
            network
                .add_control_point(ControlPoint::fixed_horizontal("A"))
                .is_ok()
        );
        assert!(
            network
                .add_observation(DistanceObservation {
                    from: String::from("A"),
                    to: String::from("B"),
                    distance: 100.0,
                    sigma: 0.002,
                    kind: DistanceKind::Horizontal,
                    ..Default::default()
                })
                .is_ok()
        );
        assert!(
            network
                .add_observation(AzimuthObservation {
                    from: String::from("A"),
                    to: String::from("B"),
                    azimuth: 0.0,
                    sigma: 1e-5,
                    ..Default::default()
                })
                .is_ok()
        );
        assert!(
            network
                .add_observation(HorizontalDirectionObservation {
                    at: String::from("A"),
                    to: String::from("B"),
                    direction: 0.3,
                    sigma: 1e-5,
                    ..Default::default()
                })
                .is_ok()
        );
        let global = GnssGlobalPositionObservation {
            point: String::from("B"),
            geodetic: Some(GeodeticCoordinate {
                latitude: -0.59,
                longitude: 2.64,
                ellipsoidal_height: 45.0,
            }),
            ..Default::default()
        };
        assert!(network.add_observation(global).is_ok());
        let result = match adjust_horizontal_network(&network, &AdjustmentOptions::default()) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(result.unused_observations, [3]);
        assert_eq!(result.orientations.len(), 1);
        assert_eq!(result.orientations[0].point_id, "A");
        near(result.orientations[0].orientation, 2.0 * PI - 0.3, 1e-12);
    }

    #[test]
    fn timestamps_print_as_iso8601_with_their_time_system() {
        assert_eq!(to_string(&SurveyTimestamp::default()), "");
        assert_eq!(
            to_string(&SurveyTimestamp {
                year: 2024,
                month: 3,
                day: 5,
                hour: 10,
                minute: 15,
                second: 30.5,
                time_system: String::from("GPS"),
            }),
            "2024-03-05T10:15:30.5 GPS"
        );
        assert_eq!(
            to_string(&SurveyTimestamp {
                year: 1999,
                month: 12,
                day: 31,
                hour: 23,
                minute: 59,
                second: 5.25,
                time_system: String::new(),
            }),
            "1999-12-31T23:59:05.25"
        );
    }

    #[test]
    fn new_enums_have_words_for_the_report() {
        assert_eq!(to_string(Face::Left), "face left");
        assert_eq!(
            to_string(CorrectionState::Applied),
            "applied by the instrument"
        );
        assert_eq!(to_string(AntennaHeightMethod::Slant), "slant");
        assert_eq!(to_string(GnssSolution::Float), "float");
    }
}
