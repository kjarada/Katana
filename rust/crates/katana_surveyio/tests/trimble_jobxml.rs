//! Twin of tests/surveyio/test_trimble_jobxml.cpp: the Trimble JobXML reader
//! (src/trimble_jobxml.rs).
//!
//! The fixture, data/trimble_jxl/tps_gnss_job.jxl, is synthetic and was built
//! by hand from the JobXML 5.72 schema. Its setup and its Reductions section
//! were worked out by hand, like this:
//!
//! ```text
//! Setup on CP1 (N 1000, E 2000, H 50), instrument height 1.500, oriented on
//! CP2 (N 1100, E 2000) with circle reading 0 00 00: CP2 is due north, so
//! the grid azimuth of the backsight is 0 and the orientation correction 0.
//!
//! Point 100, target height 1.800, prism constant 0:
//!   face 1  HA 90.0    VA 85.0            SD 50.000
//!   face 2  HA 270.0   VA 275.0 (= 85.0)  SD 50.000
//!   mean    azimuth 90, zenith 85
//!   HD = 50 sin 85 = 50 x 0.9961946981 = 49.8097349
//!   VD = 50 cos 85 = 50 x 0.0871557427 =  4.3577871
//!   N = 1000 + 49.8097 cos 90 = 1000.0000
//!   E = 2000 + 49.8097 sin 90 = 2049.8097
//!   H = 50 + 1.500 + 4.3578 - 1.800 = 54.0578
//!
//! Point 101, target height 0, prism constant -0.0344 (added to the raw EDM
//! distance, which the schema says carries none):
//!   HA 135.0  VA 90.0  SD 20.000 -> 19.9656 horizontal
//!   N = 1000 + 19.9656 cos 135 = 1000 - 19.9656 x 0.7071067812 = 985.8822
//!   E = 2000 + 19.9656 sin 135 = 2000 + 14.1178             = 2014.1178
//!   H = 50 + 1.500 + 0 - 0 = 51.5000
//! ```
//!
//! Those are the values written into the fixture's Reductions section. The
//! cross-check test radiates the PARSED observations the same way and has to
//! land on them, which tests every unit conversion and the face-2 zenith rule
//! end to end.

#![allow(
    clippy::excessive_precision,
    reason = "The expected values are the C++ test's literals, digit for digit."
)]

mod support;

use std::f64::consts::PI;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use katana_core::text::format_exact_real;
use katana_core::{ErrorCode, Result};
use katana_survey::data_model::{observation_pointing, validate_project};
use katana_survey::{
    AngularUnit, AntennaHeightMethod, ControlPoint, CoordinateSource, CorrectionState,
    DeclaredCoordinateSystem, DistanceKind, DistanceObservation, Face, GeocentricCoordinate,
    GnssCovariance3, GnssGeocentricBaselineObservation, GnssGlobalPositionObservation,
    GnssSolution, HorizontalDirectionObservation, LinearUnit, Observation, ObservationPrecision,
    SurveyPoint, SurveyProject, SurveyStation, UnpositionedPoint, ZenithAngleObservation,
};
use katana_surveyio::{
    DetectionOutcome, FormatDescriptor, Manufacturer, ReadOptions, ReadResult, describe,
    detect_format, format_registry, probe_of, read_survey,
};

use support::{Mt19937, double_eq, near};

const ID: &str = "trimble-jobxml";
const DEG: f64 = PI / 180.0;

fn data_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/surveyio/data"
    ))
}

// The file's bytes, empty when it cannot be read, as the C++'s ifstream gives.
fn read_file(path: &std::path::Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn fixture() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| read_file(&data_dir().join("trimble_jxl").join("tps_gnss_job.jxl")))
}

fn read_jxl(bytes: &[u8], name: &str) -> Result<ReadResult> {
    read_survey(format_registry(), ID, bytes, name, &ReadOptions::default())
}

fn fixture_result() -> &'static ReadResult {
    static RESULT: OnceLock<ReadResult> = OnceLock::new();
    RESULT.get_or_init(|| {
        read_jxl(fixture(), "tps_gnss_job.jxl")
            .unwrap_or_else(|error| panic!("{}", error.describe()))
    })
}

fn find_point<'p>(project: &'p SurveyProject, id: &str) -> Option<&'p SurveyPoint> {
    project.points.iter().find(|point| point.id == id)
}

fn find_named<'p>(project: &'p SurveyProject, id: &str) -> Option<&'p UnpositionedPoint> {
    project
        .unpositioned_points
        .iter()
        .find(|point| point.id == id)
}

fn any_warning_contains(result: &ReadResult, text: &str) -> bool {
    result
        .warnings
        .iter()
        .any(|warning| describe(warning).contains(text))
}

// An observation kind and the point it is to: its `to`, or its `point` for a
// kind that names one point, as the C++ template's two branches have it.
trait Observed {
    fn of(observation: &Observation) -> Option<&Self>;
    fn subject(&self) -> &str;
}

macro_rules! observed {
    ($($kind:ident => $variant:ident . $field:ident),* $(,)?) => {
        $(
            impl Observed for $kind {
                fn of(observation: &Observation) -> Option<&Self> {
                    if let Observation::$variant(typed) = observation {
                        Some(typed)
                    } else {
                        None
                    }
                }

                fn subject(&self) -> &str {
                    &self.$field
                }
            }
        )*
    };
}

observed! {
    HorizontalDirectionObservation => HorizontalDirection.to,
    ZenithAngleObservation => ZenithAngle.to,
    DistanceObservation => Distance.to,
    GnssGeocentricBaselineObservation => GnssGeocentricBaseline.to,
    GnssGlobalPositionObservation => GnssGlobalPosition.point,
}

fn observations_of<'o, T: Observed>(list: &'o [Observation], to: &str) -> Vec<&'o T> {
    list.iter()
        .filter_map(T::of)
        .filter(|typed| typed.subject() == to)
        .collect()
}

fn only_station() -> &'static SurveyStation {
    let project = &fixture_result().project;
    assert_eq!(project.stations.len(), 1);
    &project.stations[0]
}

// `bytes` with its first `from` replaced by `to`, as std::string::replace
// at find(from).
fn replaced(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(from.len())
        .position(|window| window == from)
        .expect("the text to replace is in the fixture");
    [&bytes[..at], to, &bytes[at + from.len()..]].concat()
}

// A job of setups, each with 100 shots on both faces, in the fixture's shape,
// grown to `target_bytes`.
fn synthetic_job(target_bytes: usize) -> String {
    let mut job = String::from(concat!(
        "<?xml version=\"1.0\"?>\n<JOBFile jobName=\"BIG\" version=\"5.72\" ",
        "product=\"Trimble Access\" productVersion=\"2024.10\" ",
        "TimeStamp=\"2024-03-05T09:00:00\">\n<FieldBook>\n",
        "<AtmosphereRecord ID=\"a\"><Pressure>1013.25</Pressure><Temperature>20",
        "</Temperature><PPM>0</PPM><ApplyPPMToRawDistances>true",
        "</ApplyPPMToRawDistances></AtmosphereRecord>\n",
        "<TargetRecord ID=\"t\"><PrismType>CustomPrism</PrismType><PrismConstant>0",
        "</PrismConstant><TargetHeight>1.8</TargetHeight></TargetRecord>\n",
    ));
    job.reserve(target_bytes + 4096);
    let mut setup = 0usize;
    let mut record = 0usize;
    while job.len() < target_bytes {
        setup += 1;
        let station_id = format!("s{setup}");
        job.push_str(&format!(
            "<StationRecord ID=\"{station_id}\" TimeStamp=\"2024-03-05T09:03:00\">\
             <StationName>STN{setup}</StationName><TheodoliteHeight>1.5\
             </TheodoliteHeight><AtmosphereID>a</AtmosphereID><ScaleFactor>1</ScaleFactor>\
             </StationRecord>\n"
        ));
        for shot in 0..100 {
            for face in 1..=2 {
                record += 1;
                let shot_value = f64::from(shot);
                job.push_str(&format!(
                    "<PointRecord ID=\"p{record}\" TimeStamp=\"2024-03-05T09:05:00\">\n  \
                     <Name>P{setup}_{shot}</Name>\n  <Code>KB</Code>\n  \
                     <Method>DirectReading</Method>\n  <SurveyMethod>Fix</SurveyMethod>\n  \
                     <Classification>Normal</Classification>\n  <Deleted>false</Deleted>\n  \
                     <Circle>\n    <HorizontalCircle>{}</HorizontalCircle>\n    \
                     <VerticalCircle>{}</VerticalCircle>\n    <EDMDistance>{}</EDMDistance>\n    \
                     <Face>Face{face}</Face>\n    <HorizontalCircleStandardError>\
                     0.000277777777777778</HorizontalCircleStandardError>\n    \
                     <VerticalCircleStandardError>0.000277777777777778\
                     </VerticalCircleStandardError>\n    <EDMDistanceStandardError>0.00105\
                     </EDMDistanceStandardError>\n  </Circle>\n  <StationID>{station_id}\
                     </StationID>\n  <TargetID>t</TargetID>\n  <Pressure>1013.25</Pressure>\n  \
                     <Temperature>20</Temperature>\n</PointRecord>\n",
                    format_exact_real(if face == 1 {
                        12.3456 + shot_value
                    } else {
                        192.3456 + shot_value
                    }),
                    format_exact_real(if face == 1 { 88.1234 } else { 271.8766 }),
                    format_exact_real(25.0 + shot_value * 0.5),
                ));
            }
        }
    }
    job.push_str("</FieldBook>\n<Reductions/>\n</JOBFile>\n");
    job
}

// `count` keyed-in grid points of one class, one record each: the shape of a
// list of control marks imported into a job.
fn keyed_in_job(count: usize, classification: &str) -> String {
    let mut job = String::from("<JOBFile version=\"5.72\"><FieldBook>\n");
    job.reserve(count * 240);
    for i in 1..=count {
        job.push_str(&format!(
            "<PointRecord ID=\"{i}\"><Name>CM{i}</Name><Method>Coordinates</Method>\
             <SurveyMethod>KeyedIn</SurveyMethod><Classification>{classification}\
             </Classification><Deleted>false</Deleted><Grid><North>{i}</North><East>{i}</East>\
             <Elevation>1</Elevation></Grid></PointRecord>\n"
        ));
    }
    job.push_str("</FieldBook></JOBFile>\n");
    job
}

mod trimble_job_xml {
    use super::*;

    // ---- Registration and detection ------------------------------------------------------

    #[test]
    fn is_registered_as_an_importable_trimble_format_that_says_what_it_reads() {
        let format: FormatDescriptor = format_registry().find(ID).expect("registered");
        assert_eq!(format.manufacturer, Manufacturer::Trimble);
        assert!(format.can_import);
        assert!(!format.can_export);
        assert_eq!(format.parser_version, "1.0");
        assert_eq!(format.extensions, ["jxl"]);
        assert!(
            format.reads.observations
                && format.reads.stations
                && format.reads.instrument_settings
                && format.reads.gnss
                && format.reads.points
        );
        assert!(format_registry().reader(ID).is_some());
    }

    #[test]
    fn the_fixture_is_identified_as_job_xml_with_its_root_element_as_evidence() {
        let detection = detect_format(&probe_of(fixture(), "tps_gnss_job.jxl", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().unwrap().id, ID);
        assert!(detection.candidates()[0].evidence.contains("JOBFile"));
    }

    #[test]
    fn a_job_xml_file_with_another_extension_is_still_identified_by_its_root() {
        let detection = detect_format(&probe_of(fixture(), "export.xml", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().unwrap().id, ID);
    }

    #[test]
    fn other_formats_are_not_claimed_by_the_job_xml_probe() {
        // Every fixture of another format in the suite's data folder, and
        // short samples in the shape of the other instrument formats.
        let mut others: Vec<(String, Vec<u8>)> = vec![
            (
                String::from("landxml.xml"),
                b"<?xml version=\"1.0\"?>\n<LandXML version=\"1.2\"><CgPoints/></LandXML>\n"
                    .to_vec(),
            ),
            (
                String::from("job.gsi"),
                b"*110001+0000000000000A01 21.022+0000000009000000 22.022+0000000009000000\n"
                    .to_vec(),
            ),
            (
                String::from("job.rw5"),
                b"JB,NMJOB,DT03-05-2024,TM09:00:00\nMO,AD0,UN2,SF1.00000000,EC0,EO0.0,AU0\n"
                    .to_vec(),
            ),
            (
                String::from("obs.24o"),
                b"     3.04           OBSERVATION DATA    M                   RINEX VERSION / TYPE\n"
                    .to_vec(),
            ),
            (
                String::from("job.gts"),
                b"_'CP1_(1.500)_+CP2_ ?+00100008m0892552+0000000d\n".to_vec(),
            ),
            (
                String::from("synthetic_job.dc"),
                read_file(&data_dir().join("trimble_dc").join("synthetic_job.dc")),
            ),
        ];
        for entry in std::fs::read_dir(data_dir())
            .expect("the data folder")
            .flatten()
        {
            let path = entry.path();
            if path.is_file() {
                others.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    read_file(&path),
                ));
            }
        }
        for (name, bytes) in &others {
            for result in format_registry().probe_all(&probe_of(bytes, name, false)) {
                if result.format_id == ID {
                    assert_eq!(result.signature.confidence, 0.0, "{name}");
                }
            }
        }
    }

    #[test]
    fn a_jxl_named_file_that_is_not_job_xml_is_only_weakly_suggested() {
        let bytes = b"<?xml version=\"1.0\"?><Other/>";
        let detection = detect_format(&probe_of(bytes, "odd.jxl", false));
        assert_ne!(detection.outcome(), DetectionOutcome::Identified);
    }

    // ---- The project ---------------------------------------------------------------------

    #[test]
    fn the_job_name_units_and_declared_coordinate_system_come_from_the_file() {
        let result = fixture_result();
        assert_eq!(result.format_id, ID);
        assert_eq!(result.parser_version, "1.0");
        assert_eq!(result.project.name, "SYNTH-01");
        // The schema: values are metres and decimal degrees whatever the
        // display units (this job displays feet and DMS).
        assert_eq!(result.project.units.linear, LinearUnit::Metres);
        assert_eq!(result.project.units.angular, AngularUnit::DecimalDegrees);
        assert_eq!(
            result.project.coordinate_system,
            DeclaredCoordinateSystem::named("Australia / MGA2020 Zone 56")
        );
        assert_eq!(
            result.project.metadata["jxl.environment.CoordinateSystem/DatumName"],
            "GDA2020"
        );
        assert_eq!(
            result.project.metadata["jxl.UnitsRecord.DistanceUnits"],
            "InternationalFeet"
        );
        assert_eq!(result.project.metadata["jxl.productVersion"], "2024.10");
        assert_eq!(result.project.source.format_version, "JobXML 5.72");
    }

    #[test]
    fn keyed_in_grid_points_are_positioned_and_a_control_class_point_is_control() {
        let project = &fixture_result().project;
        let cp1 = find_point(project, "CP1").expect("CP1");
        assert_eq!(cp1.northing, 1000.0);
        assert_eq!(cp1.easting, 2000.0);
        assert_eq!(cp1.elevation, Some(50.0));
        assert_eq!(cp1.coordinate_source, CoordinateSource::Entered);
        assert_eq!(cp1.code, "CONTROL");
        assert_eq!(project.control_points.len(), 1);
        assert_eq!(project.control_points[0], ControlPoint::fixed3d("CP1"));
        let cp2 = find_point(project, "CP2").expect("CP2");
        assert_eq!(cp2.northing, 1100.0);
        // Observed-only points are named, never placed at a made-up position.
        assert!(find_point(project, "100").is_none());
        let hundred = find_named(project, "100").expect("100 is named");
        assert_eq!(hundred.code, "KB&FL"); // &amp; decoded
        let tree = find_named(project, "101").expect("101 is named");
        assert_eq!(tree.description, "Gum");
        assert_eq!(tree.metadata["feature"], "Tree");
        assert_eq!(tree.metadata["attribute.Species"], "Eucalyptus");
    }

    #[test]
    fn a_setup_carries_its_instrument_and_atmosphere_with_the_correction_states_the_schema_states()
    {
        let station = only_station();
        assert_eq!(station.setup.id, "CP1#1");
        assert_eq!(station.setup.point_id, "CP1");
        assert_eq!(station.setup.instrument_height, 1.5);
        let settings = &station.instrument;
        assert_eq!(settings.make, "Trimble");
        assert_eq!(settings.model, "S7");
        assert_eq!(settings.serial_number, "37310042");
        assert_eq!(settings.pressure_hectopascals, Some(1013.25));
        assert_eq!(settings.temperature_celsius, Some(20.0));
        assert_eq!(settings.atmospheric_ppm, Some(0.0));
        // ApplyPPMToRawDistances true: the raw distances still need it.
        assert_eq!(settings.atmospheric_ppm_state, CorrectionState::NotApplied);
        assert_eq!(settings.refraction_coefficient, Some(0.142));
        assert_eq!(
            settings.curvature_refraction_state,
            CorrectionState::NotApplied
        );
        assert_eq!(settings.scale_factor, Some(1.0));
        assert_eq!(settings.scale_factor_state, CorrectionState::NotApplied);
        assert_eq!(settings.time.year, 2024);
        assert_eq!(settings.time.hour, 9);
        assert_eq!(settings.time.minute, 3);
        assert_eq!(settings.time.time_system, ""); // no zone in the file: not guessed
        assert_eq!(
            station.metadata["jxl.applyEarthCurvatureCorrection"],
            "false"
        );
    }

    #[test]
    fn the_backsight_record_orients_the_setup_with_the_face_one_circle_reading() {
        let station = only_station();
        assert_eq!(station.backsight_point_id, "CP2");
        assert_eq!(station.backsight_azimuth, Some(0.0));
        assert_eq!(station.metadata["jxl.orientationCorrection_deg"], "0.0");
    }

    #[test]
    fn one_shot_becomes_direction_zenith_and_slope_distance_sharing_one_pointing_and_face() {
        let station = only_station();
        let directions =
            observations_of::<HorizontalDirectionObservation>(&station.observations, "100");
        let zeniths = observations_of::<ZenithAngleObservation>(&station.observations, "100");
        let distances = observations_of::<DistanceObservation>(&station.observations, "100");
        assert_eq!(directions.len(), 2);
        assert_eq!(zeniths.len(), 2);
        assert_eq!(distances.len(), 2);
        // Face 1: 90 degrees = pi/2 exactly as read; zenith 85 degrees.
        double_eq(directions[0].direction, PI / 2.0);
        double_eq(zeniths[0].angle, 85.0 * DEG);
        assert_eq!(distances[0].distance, 50.0);
        assert_eq!(distances[0].kind, DistanceKind::Slope);
        assert_eq!(directions[0].pointing, zeniths[0].pointing);
        assert_eq!(directions[0].pointing, distances[0].pointing);
        assert_eq!(directions[0].pointing.face, Face::Left);
        assert_ne!(directions[0].pointing.index, 0);
        // Face 2 is a separate pointing.
        assert_eq!(directions[1].pointing.face, Face::Right);
        assert_ne!(directions[1].pointing.index, directions[0].pointing.index);
        assert_eq!(zeniths[0].instrument_height, 1.5);
        assert_eq!(zeniths[0].target_height, 1.8);
        assert_eq!(distances[0].target_height, 1.8);
    }

    #[test]
    fn a_face_two_circle_reading_is_kept_raw_and_its_zenith_is_the_angle_it_measures() {
        let observations = &only_station().observations;
        let directions = observations_of::<HorizontalDirectionObservation>(observations, "100");
        let zeniths = observations_of::<ZenithAngleObservation>(observations, "100");
        assert_eq!(directions.len(), 2);
        // 270 degrees as read (the reduction pairs the faces).
        double_eq(directions[1].direction, 1.5 * PI);
        // A reading of 275 measures a zenith angle of 360 - 275 = 85 degrees.
        double_eq(zeniths[1].angle, 85.0 * DEG);
        assert_eq!(zeniths[1].pointing.face, Face::Right);
    }

    #[test]
    fn standard_errors_in_degrees_become_radians_and_missing_ones_use_the_default_precision() {
        let observations = &only_station().observations;
        let directions = observations_of::<HorizontalDirectionObservation>(observations, "100");
        let distances = observations_of::<DistanceObservation>(observations, "100");
        // 0.000277777... degrees = 1" = pi / 648000 = 4.8481368e-6 rad.
        near(directions[0].sigma, PI / 648000.0, 1e-15);
        assert_eq!(distances[0].sigma, 0.00105);
        // Face 2 states none: the reader's precision (3", and 2 mm + 2 ppm at
        // 50 m = hypot(0.002, 0.0001) = 0.0020025 m).
        let precision = ObservationPrecision::default();
        assert_eq!(directions[1].sigma, precision.direction);
        near(distances[1].sigma, 0.0020024984, 1e-9);
    }

    #[test]
    fn prism_constant_and_target_height_come_from_the_target_record_the_shot_names() {
        let distances = observations_of::<DistanceObservation>(&only_station().observations, "101");
        assert_eq!(distances.len(), 1);
        assert_eq!(distances[0].target.prism_constant, Some(-0.0344));
        assert_eq!(
            distances[0].target.prism_constant_state,
            CorrectionState::NotApplied
        );
        assert_eq!(distances[0].target.target_type, "CustomPrism");
        assert_eq!(distances[0].target_height, 0.0);
        assert_eq!(distances[0].distance, 20.0); // raw: the constant is not added here
    }

    #[test]
    fn deleted_records_are_not_imported_and_a_warning_names_them() {
        let result = fixture_result();
        assert!(find_named(&result.project, "102").is_none());
        assert!(any_warning_contains(
            result,
            "1 record(s) the surveyor deleted"
        ));
        assert!(any_warning_contains(result, "0000000e"));
    }

    #[test]
    fn a_mean_turned_angle_is_not_read_as_another_observation() {
        let result = fixture_result();
        // Two shots to 100 (faces 1 and 2), not three.
        assert_eq!(
            observations_of::<DistanceObservation>(&only_station().observations, "100").len(),
            2
        );
        assert!(any_warning_contains(
            result,
            "1 mean turned angle record(s)"
        ));
    }

    #[test]
    fn record_kinds_and_elements_it_does_not_read_are_counted_in_warnings() {
        let result = fixture_result();
        assert!(any_warning_contains(
            result,
            "1 SurveyEventRecord record(s) were not read"
        ));
        assert!(any_warning_contains(
            result,
            "PointRecord ComputedGrid in 1 record(s)"
        ));
        assert!(any_warning_contains(
            result,
            "PointRecord QualityControl1 in 1 record(s)"
        ));
        assert!(any_warning_contains(
            result,
            "PointRecord Precision in 1 record(s)"
        ));
        // Every warning has the file name in it, so a person can find it.
        for warning in &result.warnings {
            assert_eq!(warning.file_name, "tps_gnss_job.jxl");
        }
        // The FieldBook holds 26 records. Skipped: the SurveyEventRecord (a
        // kind this reader does not import), point 102 (deleted) and the mean
        // turned angle to 100 (the mean of shots read from their own records)
        // = 3. Read: the other 23 - 13 setup, equipment, line, note and units
        // records and 10 point records (CP1, CP2, the backsight shot to CP2,
        // two shots to 100, the shot to 101, BASE, G100, G200, G300). Each
        // record once.
        assert_eq!(result.records_skipped, 3);
        assert_eq!(result.records_read, 23);
    }

    #[test]
    fn an_rtk_vector_becomes_a_geocentric_baseline_with_covariance_and_both_antennas() {
        let vectors = observations_of::<GnssGeocentricBaselineObservation>(
            &fixture_result().project.observations,
            "G100",
        );
        assert_eq!(vectors.len(), 1);
        let vector = vectors[0];
        assert_eq!(vector.from, "BASE");
        assert_eq!(
            vector.delta,
            GeocentricCoordinate {
                x: 10.123,
                y: -5.456,
                z: 7.789
            }
        );
        assert_eq!(
            vector.covariance,
            GnssCovariance3 {
                xx: 4.0e-5,
                yy: 9.0e-5,
                zz: 1.6e-4,
                xy: 1.0e-6,
                xz: -2.0e-6,
                yz: 3.0e-6
            }
        );
        assert_eq!(vector.solution, GnssSolution::Fixed);
        assert_eq!(vector.reference_frame, "WGS 84");
        // The base antenna record states a ReducedHeight (1.720, measured
        // 1.650): the controller's height to the phase centre, carried as such.
        assert_eq!(vector.from_antenna.height, 1.72);
        assert_eq!(vector.from_antenna.method, AntennaHeightMethod::PhaseCentre);
        assert!(
            vector.from_antenna.measured_to.contains("measured 1.65 m"),
            "{}",
            vector.from_antenna.measured_to
        );
        // The rover's states none: the measured height and the method's words.
        assert_eq!(vector.to_antenna.height, 2.0);
        assert_eq!(vector.to_antenna.r#type, "R12i Internal");
        assert_eq!(vector.to_antenna.method, AntennaHeightMethod::Vertical);
        assert_eq!(vector.to_antenna.measured_to, "BottomOfAntennaMount");
    }

    #[test]
    fn a_network_rtk_position_is_geocentric_and_an_autonomous_one_geodetic_in_radians() {
        let project = &fixture_result().project;
        let network =
            observations_of::<GnssGlobalPositionObservation>(&project.observations, "G200");
        assert_eq!(network.len(), 1);
        assert_eq!(
            network[0].geocentric.expect("geocentric"),
            GeocentricCoordinate {
                x: -4646000.125,
                y: 2553000.25,
                z: -3534000.5
            }
        );
        assert_eq!(network[0].solution, GnssSolution::Fixed);

        let autonomous =
            observations_of::<GnssGlobalPositionObservation>(&project.observations, "G300");
        assert_eq!(autonomous.len(), 1);
        let geodetic = autonomous[0].geodetic.expect("geodetic");
        // -33.865 deg x pi / 180 = -0.5910557512 rad; 151.2094 deg = 2.6391018900 rad.
        near(geodetic.latitude, -0.5910557511878798, 1e-15);
        near(geodetic.longitude, 2.6391018899651137, 1e-15);
        assert_eq!(geodetic.ellipsoidal_height, 45.0);
        assert_eq!(autonomous[0].solution, GnssSolution::Autonomous);
        // QualityControl3 sigmas 2, 3, 5 m -> variances 4, 9, 25 m^2
        // (north/east/up).
        assert_eq!(
            autonomous[0].covariance,
            GnssCovariance3 {
                xx: 4.0,
                yy: 9.0,
                zz: 25.0,
                xy: 0.5,
                xz: 0.0,
                yz: 0.0
            }
        );
    }

    #[test]
    fn a_keyed_in_latitude_longitude_is_named_with_its_values_in_metadata_not_placed() {
        let result = fixture_result();
        let base = find_named(&result.project, "BASE").expect("BASE is named");
        // The value as the fixture keys it in, written back exactly - not as
        // std::to_string would have it, which libc++ still pads to
        // "-33.865000".
        assert_eq!(base.metadata["wgs84.latitude_deg"], "-33.865");
        assert!(any_warning_contains(
            result,
            "keyed-in or local latitude and longitude"
        ));
    }

    #[test]
    fn line_records_become_features_and_notes_project_metadata() {
        let project = &fixture_result().project;
        assert_eq!(project.features.len(), 1);
        assert_eq!(project.features[0].name, "L1");
        assert_eq!(project.features[0].code, "BOUNDARY");
        assert_eq!(project.features[0].point_ids, ["CP1", "CP2"]);
        // Text and CDATA join into one note, markup characters intact.
        assert_eq!(
            project.metadata["jxl.note.0000001a"],
            "Wind gusting; shots <after> 10:00 repeated"
        );
    }

    // ---- The controller's reductions, as a cross-check ---------------------------------------

    #[test]
    fn the_controllers_reduced_coordinates_are_kept_in_point_metadata_as_written() {
        let result = fixture_result();
        let hundred = find_named(&result.project, "100").expect("100 is named");
        assert_eq!(hundred.metadata["trimble.reductions.north"], "1000.0000");
        assert_eq!(hundred.metadata["trimble.reductions.east"], "2049.8097");
        assert_eq!(hundred.metadata["trimble.reductions.elevation"], "54.0578");
        assert_eq!(hundred.metadata["trimble.reductions.recordId"], "0000000a");
        assert_eq!(
            find_named(&result.project, "G100").unwrap().metadata["trimble.reductions.height"],
            "46.1"
        );
        // ZZZ is in Reductions only: said, not invented.
        assert!(any_warning_contains(
            result,
            "1 point(s) in the controller's Reductions section"
        ));
    }

    #[test]
    fn radiating_the_parsed_shots_reproduces_the_controllers_reduced_coordinates() {
        // See the working at the top of this file: this repeats it with the
        // values the reader produced, so a wrong unit, a wrong face-2 rule or a
        // prism constant of the wrong sign lands away from the controller's
        // answer.
        let project = &fixture_result().project;
        let station = only_station();
        let at = find_point(project, "CP1").expect("CP1");
        let backsight = find_point(project, "CP2").expect("CP2");
        let backsight_azimuth =
            (backsight.easting - at.easting).atan2(backsight.northing - at.northing);
        let orientation = backsight_azimuth - station.backsight_azimuth.expect("a circle");

        for target in ["100", "101"] {
            let directions =
                observations_of::<HorizontalDirectionObservation>(&station.observations, target);
            let zeniths = observations_of::<ZenithAngleObservation>(&station.observations, target);
            let distances = observations_of::<DistanceObservation>(&station.observations, target);
            let mut sum_direction = 0.0_f64;
            let mut sum_zenith = 0.0_f64;
            let mut sum_distance = 0.0_f64;
            for ((direction, zenith), distance) in directions.iter().zip(&zeniths).zip(&distances) {
                let face = if direction.pointing.face == Face::Right {
                    PI
                } else {
                    0.0
                };
                sum_direction += katana_math::cpp::remainder(direction.direction - face, 2.0 * PI);
                sum_zenith += zenith.angle;
                sum_distance += distance.distance + distance.target.prism_constant.unwrap_or(0.0);
            }
            let n = directions.len() as f64;
            let azimuth = sum_direction / n + orientation;
            let zenith: f64 = sum_zenith / n;
            let slope = sum_distance / n;
            let horizontal = slope * zenith.sin();
            let north = at.northing + horizontal * azimuth.cos();
            let east = at.easting + horizontal * azimuth.sin();
            let height = at.elevation.expect("a height")
                + station.setup.instrument_height
                + slope * zenith.cos()
                - zeniths[0].target_height;

            let metadata = &find_named(project, target).expect("named").metadata;
            let stated = |key: &str| -> f64 { metadata[key].parse().expect("a number") };
            // The controller wrote four decimals: agreement to half the last
            // digit.
            near(north, stated("trimble.reductions.north"), 5.1e-5);
            near(east, stated("trimble.reductions.east"), 5.1e-5);
            near(height, stated("trimble.reductions.elevation"), 5.1e-5);
        }
    }

    // ---- Failure and edge cases -------------------------------------------------------------

    #[test]
    fn a_file_whose_root_is_not_job_file_is_refused_naming_what_it_found() {
        let error = read_jxl(b"<?xml version=\"1.0\"?><LandXML/>", "tps_gnss_job.jxl")
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(error.message.contains("<LandXML>"));
    }

    #[test]
    fn a_truncated_job_is_refused_as_incomplete_rather_than_half_imported() {
        let cut = &fixture()[..fixture().len() / 2];
        let error = read_jxl(cut, "tps_gnss_job.jxl").expect_err("refused");
        assert!(error.message.contains("incomplete"), "{}", error.message);
    }

    #[test]
    fn a_document_type_declaration_is_refused() {
        let error = read_jxl(
            b"<?xml version=\"1.0\"?><!DOCTYPE JOBFile [<!ENTITY x \"y\">]><JOBFile/>",
            "tps_gnss_job.jxl",
        )
        .expect_err("refused");
        assert!(error.message.contains("DOCTYPE"));
    }

    #[test]
    fn an_undefined_entity_in_a_value_is_refused() {
        let error = read_jxl(
            concat!(
                "<JOBFile version=\"5.72\"><FieldBook><NoteRecord ID=\"1\"><Notes><Note>&evil;</Note>",
                "</Notes></NoteRecord></FieldBook></JOBFile>"
            )
            .as_bytes(),
            "tps_gnss_job.jxl",
        )
        .expect_err("refused");
        assert!(error.message.contains("entity"), "{}", error.message);
    }

    #[test]
    fn nesting_deeper_than_the_limit_is_refused_not_recursed_into() {
        let mut deep = String::from("<JOBFile><FieldBook><NoteRecord ID=\"1\">");
        for _ in 0..200 {
            deep.push_str("<a>");
        }
        let error = read_jxl(deep.as_bytes(), "tps_gnss_job.jxl").expect_err("refused");
        assert!(error.message.contains("nested deeper"), "{}", error.message);
    }

    #[test]
    fn an_end_tag_that_does_not_match_is_refused_with_its_line() {
        let error = read_jxl(
            b"<JOBFile>\n<FieldBook>\n</Reductions>\n</JOBFile>",
            "tps_gnss_job.jxl",
        )
        .expect_err("refused");
        assert!(error.message.contains("line 3"), "{}", error.message);
    }

    #[test]
    fn a_number_that_is_not_a_number_is_a_warning_and_the_shot_keeps_the_rest() {
        let bytes = replaced(
            fixture(),
            b"<EDMDistance>20.000</EDMDistance>",
            b"<EDMDistance>twenty</EDMDistance>",
        );
        let read = read_jxl(&bytes, "tps_gnss_job.jxl")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(any_warning_contains(
            &read,
            "Circle/EDMDistance 'twenty' is not a number"
        ));
        let observations = &read.project.stations[0].observations;
        assert!(observations_of::<DistanceObservation>(observations, "101").is_empty());
        assert_eq!(
            observations_of::<HorizontalDirectionObservation>(observations, "101").len(),
            1
        );
    }

    #[test]
    fn two_coordinate_records_for_one_name_keep_the_control_one_and_warn_with_the_other() {
        let job = concat!(
            "<JOBFile version=\"5.72\"><FieldBook>",
            "<PointRecord ID=\"1\"><Name>A</Name><Method>Coordinates</Method><SurveyMethod>KeyedIn</SurveyMethod>",
            "<Classification>Normal</Classification><Deleted>false</Deleted>",
            "<Grid><North>1</North><East>2</East><Elevation/></Grid></PointRecord>",
            "<PointRecord ID=\"2\"><Name>A</Name><Method>Coordinates</Method><SurveyMethod>KeyedIn</SurveyMethod>",
            "<Classification>Control</Classification><Deleted>false</Deleted>",
            "<Grid><North>5</North><East>6</East><Elevation/></Grid></PointRecord>",
            "</FieldBook><Reductions/></JOBFile>",
        );
        let read = read_jxl(job.as_bytes(), "tps_gnss_job.jxl")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.project.points.len(), 1);
        assert_eq!(read.project.points[0].northing, 5.0);
        assert!(read.project.points[0].elevation.is_none()); // empty element = no height
        assert_eq!(
            read.project.control_points,
            [ControlPoint::fixed_horizontal("A")]
        );
        assert!(any_warning_contains(
            &read,
            "kept the control record 2 and not the one at N 1 E 2"
        ));
        // Both read: record 1 was imported, then replaced by record 2.
        assert_eq!(read.records_read, 2);
        assert_eq!(read.records_skipped, 0);
    }

    #[test]
    fn a_setup_with_no_instrument_height_or_atmosphere_says_so_before_import() {
        let job = concat!(
            "<JOBFile version=\"5.72\"><FieldBook>",
            "<StationRecord ID=\"1\"><StationName>S</StationName><TheodoliteHeight/></StationRecord>",
            "<PointRecord ID=\"2\"><Name>T</Name><Method>DirectReading</Method><Deleted>false</Deleted>",
            "<Circle><HorizontalCircle>10</HorizontalCircle><VerticalCircle/><EDMDistance/>",
            "<Face>FaceNull</Face></Circle><StationID>1</StationID></PointRecord>",
            "</FieldBook></JOBFile>",
        );
        let read = read_jxl(job.as_bytes(), "tps_gnss_job.jxl")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(any_warning_contains(&read, "has no instrument height"));
        assert_eq!(read.not_carried.len(), 3);
        assert!(read.not_carried[0].contains("atmospheric correction state is unknown"));
        assert!(read.not_carried[1].contains("without an instrument height"));
        assert!(read.not_carried[2].contains("without a target height"));
        let observations = &read.project.stations[0].observations;
        assert_eq!(observations.len(), 1);
        assert_eq!(
            observation_pointing(&observations[0])
                .expect("a pointing")
                .face,
            Face::Unknown
        );
    }

    #[test]
    fn each_record_is_counted_once_as_read_only_when_something_of_it_was_imported() {
        // Fifteen records, numbered by their IDs. Read: 1 (a setup), 14 (a shot
        // whose direction is kept though its distance is refused) and 15 (a
        // shot) = 3. Skipped, each with its warning: 2 has no station name, 3
        // orients a setup that is not in the file, 4 has no point name, 5 is a
        // shot from a setup that is not in the file, 6 a shot from S to S, 7 a
        // line from S to S, 8 a GNSS vector with no base, 9 is deleted, 10 is a
        // mean turned angle, 11 a shot with no reading, 12 a method this reader
        // does not turn into anything, 13 a shot whose only value (a negative
        // distance) is refused = 12. 3 + 12 = 15: no record counted twice or
        // not at all.
        let job = concat!(
            "<JOBFile version=\"5.72\"><FieldBook>\n",
            "<StationRecord ID=\"1\"><StationName>S</StationName><TheodoliteHeight>1.5",
            "</TheodoliteHeight></StationRecord>\n",
            "<StationRecord ID=\"2\"><StationName/><TheodoliteHeight>1.5</TheodoliteHeight>",
            "</StationRecord>\n",
            "<BackBearingRecord ID=\"3\"><StationRecordID>99</StationRecordID><BackSight>B</BackSight>",
            "</BackBearingRecord>\n",
            "<PointRecord ID=\"4\"><Name/><Deleted>false</Deleted><Grid><North>1</North><East>2</East>",
            "</Grid></PointRecord>\n",
            "<PointRecord ID=\"5\"><Name>T</Name><Deleted>false</Deleted><Circle><HorizontalCircle>10",
            "</HorizontalCircle></Circle><StationID>99</StationID></PointRecord>\n",
            "<PointRecord ID=\"6\"><Name>S</Name><Deleted>false</Deleted><Circle><HorizontalCircle>10",
            "</HorizontalCircle></Circle><StationID>1</StationID></PointRecord>\n",
            "<LineRecord ID=\"7\"><Name>L</Name><Method>TwoPoints</Method><StartPoint>S</StartPoint>",
            "<EndPoint>S</EndPoint><Deleted>false</Deleted></LineRecord>\n",
            "<PointRecord ID=\"8\"><Name>G</Name><Deleted>false</Deleted><ECEFDeltas><DeltaX>1</DeltaX>",
            "<DeltaY>2</DeltaY><DeltaZ>3</DeltaZ></ECEFDeltas></PointRecord>\n",
            "<PointRecord ID=\"9\"><Name>T</Name><Deleted>true</Deleted><Circle><HorizontalCircle>10",
            "</HorizontalCircle></Circle><StationID>1</StationID></PointRecord>\n",
            "<PointRecord ID=\"10\"><Name>T</Name><Method>MeanTurnedAngle</Method><Deleted>false",
            "</Deleted><MTA><HorizontalAngle>10</HorizontalAngle></MTA></PointRecord>\n",
            "<PointRecord ID=\"11\"><Name>U</Name><Deleted>false</Deleted><Circle><Face>Face1</Face>",
            "</Circle><StationID>1</StationID></PointRecord>\n",
            "<PointRecord ID=\"12\"><Name>V</Name><Method>LaserOffset</Method><Deleted>false</Deleted>",
            "</PointRecord>\n",
            "<PointRecord ID=\"13\"><Name>W</Name><Deleted>false</Deleted><Circle><EDMDistance>-5",
            "</EDMDistance></Circle><StationID>1</StationID></PointRecord>\n",
            "<PointRecord ID=\"14\"><Name>X</Name><Deleted>false</Deleted><Circle><HorizontalCircle>20",
            "</HorizontalCircle><EDMDistance>-5</EDMDistance></Circle><StationID>1</StationID>",
            "</PointRecord>\n",
            "<PointRecord ID=\"15\"><Name>T</Name><Deleted>false</Deleted><Circle><HorizontalCircle>30",
            "</HorizontalCircle><VerticalCircle>90</VerticalCircle></Circle><StationID>1</StationID>",
            "</PointRecord>\n",
            "</FieldBook></JOBFile>\n",
        );
        let read = read_jxl(job.as_bytes(), "counts.jxl")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.records_read, 3);
        assert_eq!(read.records_skipped, 12);
        // Records 14 and 15: HA to X; HA and ZA to T.
        assert_eq!(read.project.stations.len(), 1);
        assert_eq!(read.project.stations[0].observations.len(), 3);
        for said in [
            "counts.jxl record 3: a station record with no station name",
            "counts.jxl record 4: backsight record 3 names setup record 99",
            "counts.jxl record 5: a point record with no name",
            "counts.jxl record 6: observation to T names setup record 99",
            "counts.jxl record 7: observation from S to itself",
            "counts.jxl record 8: line L does not join two different points",
            "counts.jxl record 9: the GNSS vector to G has no base point",
            "1 record(s) the surveyor deleted",
            "1 mean turned angle record(s)",
            "counts.jxl record 12: observation to U records no circle reading",
            "counts.jxl record 13: point V was recorded by the method LaserOffset",
            "counts.jxl record 14: slope distance not imported",
            "counts.jxl record 15: slope distance not imported",
        ] {
            assert!(any_warning_contains(&read, said), "{said}");
        }
    }

    // ---- Robustness ---------------------------------------------------------------------------

    #[test]
    fn each_shot_records_the_line_its_record_starts_on_whatever_the_line_endings() {
        // one_setup.jxl, counted by hand in an editor: the StationRecord
        // starts on line 6, the shot to B (PointRecord 5) on line 8, the shot
        // to C (PointRecord 7) on line 10. Most of its lines are well over the
        // eight bytes the line counter takes at a time. With CR LF line endings
        // the lines are the same: a CR is not a new line.
        let lf = read_file(&data_dir().join("trimble_jxl").join("one_setup.jxl"));
        let mut crlf = Vec::with_capacity(lf.len() * 2);
        for &c in &lf {
            if c == b'\n' {
                crlf.push(b'\r');
            }
            crlf.push(c);
        }
        for bytes in [&lf, &crlf] {
            let read = read_jxl(bytes, "one_setup.jxl")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(read.project.stations.len(), 1);
            let station = &read.project.stations[0];
            assert_eq!(station.source.record_number, 6);
            let to_b = observations_of::<DistanceObservation>(&station.observations, "B");
            let to_c = observations_of::<ZenithAngleObservation>(&station.observations, "C");
            assert_eq!(to_b.len(), 1);
            assert_eq!(to_c.len(), 1);
            assert_eq!(to_b[0].source.record_number, 8);
            assert_eq!(to_c[0].source.record_number, 10);
            assert_eq!(to_c[0].source.file_name, "one_setup.jxl");
        }
    }

    #[test]
    fn a_small_job_reads_its_applied_ppm_face_two_zenith_and_float_vector() {
        let read = read_jxl(
            &read_file(&data_dir().join("trimble_jxl").join("one_setup.jxl")),
            "one_setup.jxl",
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.project.stations.len(), 1);
        let station = &read.project.stations[0];
        // ApplyPPMToRawDistances false: the ppm is already in the distances.
        assert_eq!(
            station.instrument.atmospheric_ppm_state,
            CorrectionState::Applied
        );
        assert_eq!(station.instrument.atmospheric_ppm, Some(1.5));
        let zeniths = observations_of::<ZenithAngleObservation>(&station.observations, "C");
        assert_eq!(zeniths.len(), 1);
        // Face 2 reading 268.25 -> zenith 360 - 268.25 = 91.75 degrees.
        double_eq(zeniths[0].angle, 91.75 * DEG);
        let distances = observations_of::<DistanceObservation>(&station.observations, "C");
        assert_eq!(distances.len(), 1);
        assert_eq!(distances[0].target.prism_constant, Some(0.0175));
        assert_eq!(distances[0].target_height, 1.3);
        let vectors =
            observations_of::<GnssGeocentricBaselineObservation>(&read.project.observations, "G");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].solution, GnssSolution::Float);
        assert!(!vectors[0].covariance.stated());
        assert_eq!(
            read.project.coordinate_system,
            DeclaredCoordinateSystem::named("Local")
        );
        assert_eq!(read.project.points[0].code, "C&D");
    }

    #[test]
    fn every_truncation_of_a_small_job_ends_in_an_error_or_a_project_never_a_crash() {
        let bytes = read_file(&data_dir().join("trimble_jxl").join("one_setup.jxl"));
        assert!(bytes.len() > 1000);
        let mut refused = 0usize;
        for length in 0..=bytes.len() {
            if let Err(error) = read_jxl(&bytes[..length], "tps_gnss_job.jxl") {
                refused += 1;
                assert!(!error.message.is_empty());
            }
        }
        // Only the whole file (and the whole file less trailing white space)
        // reads.
        assert!(refused >= bytes.len() - 2);
    }

    #[test]
    fn random_bytes_and_randomly_damaged_jobs_end_in_an_error_or_a_project_never_a_crash() {
        let mut random = Mt19937::new(20240305); // fixed seed: a failure must be repeatable
        for round in 0..300 {
            let length = random.next_u32() as usize % 2048;
            let mut noise = vec![0u8; length];
            for c in &mut noise {
                *c = random.uniform(0, 255) as u8;
            }
            if round % 3 == 0 {
                noise = [b"<JOBFile><FieldBook>".as_slice(), &noise[..]].concat(); // get past the root check
            }
            let _ = read_jxl(&noise, "tps_gnss_job.jxl");
        }
        let bytes = fixture();
        let alphabet = b"<>/&;\"'=!?[]- \nabc0123456789.";
        for _ in 0..400 {
            let mut damaged = bytes.to_vec();
            let edits = 1 + random.next_u32() as usize % 8;
            for _ in 0..edits {
                // `damaged[random() % size] = alphabet[random() % size]`: the
                // right of a built-in assignment is drawn first (C++17).
                let value = alphabet[random.next_u32() as usize % alphabet.len()];
                let at = random.next_u32() as usize % damaged.len();
                damaged[at] = value;
            }
            if let Ok(read) = read_jxl(&damaged, "tps_gnss_job.jxl") {
                assert!(validate_project(&read.project).is_ok());
            }
        }
    }

    #[test]
    fn a_record_that_would_hold_far_more_than_its_own_size_is_skipped_with_a_warning_and_the_rest_read()
     {
        // A crafted record: an element with a 32 KiB name, holding 16,384
        // empty elements. Every one of those is a value whose path repeats the
        // long name, so holding them all would take 16,384 x 32 KiB = 512 MiB
        // for a file of about 128 KiB - and twice the file, four times the
        // memory. The reader stops holding a record past its limit (16 MiB),
        // skips it with a warning on the line it starts on (2), and reads the
        // point after it.
        let long_name = "A".repeat(32 * 1024);
        let mut job =
            format!("<JOBFile version=\"5.72\"><FieldBook>\n<NoteRecord ID=\"1\"><{long_name}>");
        for _ in 0..16384 {
            job.push_str("<b/>");
        }
        job.push_str(&format!("</{long_name}></NoteRecord>\n"));
        job.push_str(concat!(
            "<PointRecord ID=\"2\"><Name>P</Name><Method>Coordinates</Method><Deleted>false</Deleted>",
            "<Grid><North>1</North><East>2</East><Elevation>3</Elevation></Grid></PointRecord>\n",
            "</FieldBook></JOBFile>\n",
        ));
        let start = Instant::now();
        let read = read_jxl(job.as_bytes(), "crafted.jxl");
        let seconds = start.elapsed().as_secs_f64();
        let read = read.unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            any_warning_contains(
                &read,
                "crafted.jxl record 2: the NoteRecord record holds more"
            ),
            "{}",
            read.warnings
                .first()
                .map_or_else(|| String::from("no warning"), describe)
        );
        assert_eq!(read.records_skipped, 1);
        assert_eq!(read.records_read, 1);
        assert_eq!(read.project.points.len(), 1);
        assert_eq!(read.project.points[0].id, "P");
        assert!(!read.project.metadata.contains_key("jxl.note.1")); // the note was not half read
        // It reads no more than the 16 MiB before it stops: well under a second
        // even in a Debug build; the bound is loose for a busy machine.
        assert!(seconds < 10.0, "{seconds} s");
    }

    // ---- Throughput ----------------------------------------------------------------------------

    // Correctness only in the suite; the speed is printed.
    // KATANA_SURVEY_THROUGHPUT_MB sets the size (default 4 MB, so the suite
    // stays quick in a Debug build).
    #[test]
    fn a_large_synthetic_job_reads_every_shot_and_reports_its_throughput() {
        let mut megabytes = 4usize;
        if let Ok(requested) = std::env::var("KATANA_SURVEY_THROUGHPUT_MB") {
            // strtoul: the leading digits, 0 when there are none.
            let digits: String = requested
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            megabytes = digits.parse().unwrap_or(0);
        }
        let job = synthetic_job(megabytes * 1024 * 1024);
        // The reader alone first (the parser's own speed), then through
        // read_survey, which adds the project validation every import pays.
        let reader = format_registry().reader(ID).expect("a reader");
        let reader_start = Instant::now();
        let direct = reader(job.as_bytes(), "big.jxl", &ReadOptions::default());
        let reader_seconds = reader_start.elapsed().as_secs_f64();
        assert!(
            direct.is_ok(),
            "{}",
            direct
                .as_ref()
                .map_or_else(|error| error.describe(), |_| String::new())
        );
        drop(direct); // not timed: the next read should not share the heap with this one
        let start = Instant::now();
        let read = read_jxl(job.as_bytes(), "big.jxl");
        let seconds = start.elapsed().as_secs_f64();
        let read = read.unwrap_or_else(|error| panic!("{}", error.describe()));
        let observations: usize = read
            .project
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum();
        // Every setup has 100 shots x 2 faces x (HA, ZA, SD).
        assert_eq!(observations, read.project.stations.len() * 600);
        assert!(read.warnings.is_empty());
        let mb = job.len() as f64 / (1024.0 * 1024.0);
        println!(
            "[ throughput ] JobXML: {mb} MB, {observations} observations; reader {reader_seconds} \
             s = {} MB/s; read_survey (validation included) {seconds} s = {} MB/s",
            mb / reader_seconds,
            mb / seconds
        );
    }

    #[test]
    fn a_control_list_reads_in_about_the_time_of_the_same_list_of_ordinary_points() {
        // 12,000 control marks. Were each checked against every control point
        // listed before it, the read would make 12,000 x 11,999 / 2 = 72
        // million name comparisons - several times the whole of the rest of
        // the read, and growing with the square of the list. Listed once each,
        // a control list is the same work as the same list classed Normal plus
        // one entry per point. Best of three, taken in turn, so a busy machine
        // slows both; a factor of 3 leaves room for the rest of its noise.
        const COUNT: usize = 12000;
        let control = keyed_in_job(COUNT, "Control");
        let normal = keyed_in_job(COUNT, "Normal");
        let reader = format_registry().reader(ID).expect("a reader");
        let mut best = [1e9_f64; 2];
        for _ in 0..3 {
            for (kind, fastest) in best.iter_mut().enumerate() {
                let start = Instant::now();
                let read = reader(
                    if kind == 0 {
                        control.as_bytes()
                    } else {
                        normal.as_bytes()
                    },
                    "list.jxl",
                    &ReadOptions::default(),
                );
                let seconds = start.elapsed().as_secs_f64();
                let read = read.unwrap_or_else(|error| panic!("{}", error.describe()));
                assert_eq!(read.project.points.len(), COUNT);
                assert_eq!(
                    read.project.control_points.len(),
                    if kind == 0 { COUNT } else { 0 }
                );
                *fastest = fastest.min(seconds);
            }
        }
        println!(
            "[ scaling ] JobXML: {COUNT} control points {} s, as many ordinary points {} s",
            best[0], best[1]
        );
        assert!(best[0] < 3.0 * best[1]);
    }

    #[test]
    fn a_control_mark_recorded_twice_is_listed_as_control_once() {
        // A re-imported control list repeats its marks: A twice (the first
        // kept, with its height; the second skipped, its values in a warning),
        // then B with no height.
        let job = concat!(
            "<JOBFile version=\"5.72\"><FieldBook>",
            "<PointRecord ID=\"1\"><Name>A</Name><Method>Coordinates</Method>",
            "<Classification>Control</Classification><Deleted>false</Deleted>",
            "<Grid><North>1</North><East>2</East><Elevation>3</Elevation></Grid></PointRecord>",
            "<PointRecord ID=\"2\"><Name>A</Name><Method>Coordinates</Method>",
            "<Classification>Control</Classification><Deleted>false</Deleted>",
            "<Grid><North>1</North><East>2</East><Elevation>3</Elevation></Grid></PointRecord>",
            "<PointRecord ID=\"3\"><Name>B</Name><Method>Coordinates</Method>",
            "<Classification>Control</Classification><Deleted>false</Deleted>",
            "<Grid><North>5</North><East>6</East><Elevation/></Grid></PointRecord>",
            "</FieldBook></JOBFile>",
        );
        let read = read_jxl(job.as_bytes(), "tps_gnss_job.jxl")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            read.project.control_points,
            [
                ControlPoint::fixed3d("A"),
                ControlPoint::fixed_horizontal("B")
            ]
        );
        assert_eq!(read.records_read, 2);
        assert_eq!(read.records_skipped, 1);
    }
}
