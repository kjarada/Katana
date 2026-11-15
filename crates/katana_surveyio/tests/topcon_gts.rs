//! Twin of tests/surveyio/test_topcon_gts.cpp: the Topcon GTS readers
//! (src/topcon_gts.rs), GTS-7 read, GTS-6 recognised and refused.
//!
//! data/gts/setup.gt7 is hand-built from the record list in the Topcon Link
//! Reference Manual (P/N 7010-0522, appendix C, "GTS-7 Raw Format"); the
//! northing-first test uses that manual's own worked sample. Every expected
//! value is worked below from the record text.

mod topcon_support;

use std::f64::consts::PI;
use std::time::Instant;

use katana_core::ErrorCode;
use katana_survey::{
    AngularUnit, CoordinateSource, CorrectionState, DistanceKind, DistanceObservation, Face,
    HorizontalDirectionObservation, LevelDifferenceObservation, LinearUnit, Pointing,
    ZenithAngleObservation,
};
use katana_surveyio::{
    DetectionOutcome, Manufacturer, ReadOptions, ReadResult, detect_format, format_registry,
    probe_of,
};

use topcon_support::{
    FOREIGN_SAMPLES, any_not_carried_contains, any_warning_contains, dms, double_eq, feature,
    fixture, near, observations_to, ok, point, read, read_every_truncation_and_noise, unpositioned,
};

const GTS7: &str = "topcon-gts7";
const GTS6: &str = "topcon-gts6";
const ANGLE_TOLERANCE: f64 = 1e-12;

fn read_setup() -> ReadResult {
    let bytes = fixture("gts/setup.gt7");
    ok(read(GTS7, bytes, "setup.gt7"))
}

// The first lines of the GTS-6 sample printed in the Topcon Link manual (C-5).
const GTS6_SAMPLE: &str = concat!(
    "_'MARK_(STAT_)1.52000_+ST1_ W+000049020m09757060-03726440d+0000138560+0000070470-",
    "0000002580***+0000+000000_*STAT_,1.60000_+1_\r\n",
    "?+00003448m0781803+0910223d+00003376***+00+00000_*TREE_,1.60000\r\n"
);

mod topcon_gts {
    use super::*;

    #[test]
    fn gts7_is_registered_as_an_importable_format_with_the_link_manuals_extensions() {
        let format = format_registry().find(GTS7).expect("registered");
        assert!(format.can_import);
        assert_eq!(format.manufacturer, Manufacturer::Topcon);
        assert_eq!(format.parser_version, "1.0");
        assert_eq!(format.extensions, ["gt7", "gts7", "gts", "raw", "dat"]);
        assert!(format.reads.observations && format.reads.stations && format.reads.features);
        assert!(!format.reads.gnss);
        assert!(format_registry().reader(GTS7).is_some());
    }

    #[test]
    fn a_station_record_becomes_a_setup_with_its_height_code_coordinates_and_backsight() {
        let result = read_setup();
        let project = &result.project;
        // JOB C:\Jobs\site.raw: only the name part of a path survives.
        assert_eq!(project.name, "site.raw");
        assert_eq!(project.metadata["job description"], "Test job");
        assert_eq!(project.metadata["header"], "TTools v1.0");
        assert_eq!(project.metadata["surveyor"], "Katana");
        assert_eq!(project.metadata["date"], "24/09/26 10:15");
        assert_eq!(project.units.linear, LinearUnit::Metres);
        assert_eq!(project.units.angular, AngularUnit::DegreesMinutesSeconds);
        assert_eq!(project.stations.len(), 1);
        let station = &project.stations[0];
        assert_eq!(station.setup.id, "A");
        double_eq(station.setup.instrument_height, 1.5);
        assert_eq!(station.instrument.model, "GTS-7");
        assert_eq!(station.backsight_point_id, "B");
        // 45.00000
        near(
            station.backsight_azimuth.expect("a backsight circle"),
            PI / 4.0,
            ANGLE_TOLERANCE,
        );
        assert_eq!(station.metadata["backsight bearing (radians)"], "0");
        assert_eq!(
            station.metadata["temperature, pressure (units not stated)"],
            "20.0, 1013"
        );
        assert_eq!(station.metadata["notes"], "end, of setup");

        // XYZ after STN is the station's; after BKB the backsight's.
        let a = point(project, "A").expect("A");
        double_eq(a.northing, 1000.0);
        double_eq(a.easting, 2000.0);
        double_eq(a.elevation.expect("an elevation"), 50.0);
        assert_eq!(a.code, "STAT");
        let b = point(project, "B").expect("B");
        double_eq(b.northing, 1100.0);
        double_eq(b.easting, 2100.0);
        assert!(unpositioned(project, "C").is_some());
    }

    #[test]
    fn shots_give_direction_zenith_and_distance_with_their_pointing_and_heights() {
        let result = read_setup();
        let station = &result.project.stations[0];
        // 3 + 3 + 3 + 3 + 2, worked per shot below
        assert_eq!(station.observations.len(), 14);

        // BS B,1.60000 / SD 45.00000,89.30000,141.42600: pointing 1, face left.
        let b_direction = observations_to::<HorizontalDirectionObservation>(station, "B");
        let b_zenith = observations_to::<ZenithAngleObservation>(station, "B");
        let b_distance = observations_to::<DistanceObservation>(station, "B");
        assert_eq!(b_direction.len(), 1);
        assert_eq!(b_zenith.len(), 1);
        assert_eq!(b_distance.len(), 1);
        near(b_direction[0].direction, PI / 4.0, ANGLE_TOLERANCE);
        near(b_zenith[0].angle, dms(89, 30, 0.0), ANGLE_TOLERANCE);
        double_eq(b_distance[0].distance, 141.426);
        double_eq(b_distance[0].instrument_height, 1.5);
        double_eq(b_distance[0].target_height, 1.6);
        assert_eq!(
            b_distance[0].pointing,
            Pointing {
                index: 1,
                face: Face::Left
            }
        );
        assert_eq!(b_distance[0].source.record_number, 14);

        // SD -90.30300: a negative circle reading, 360 - 90 30' 30" = 269 29' 30".
        let c_direction = observations_to::<HorizontalDirectionObservation>(station, "C");
        assert_eq!(c_direction.len(), 1);
        near(
            c_direction[0].direction,
            dms(269, 29, 30.0),
            ANGLE_TOLERANCE,
        );
        assert_eq!(
            c_direction[0].pointing,
            Pointing {
                index: 2,
                face: Face::Left
            }
        );
        double_eq(
            observations_to::<DistanceObservation>(station, "C")[0].target_height,
            1.7,
        );

        // SD 100.00000,268.59450: face right; 360 - 268 59' 45" = 91 00' 15".
        let d_zenith = observations_to::<ZenithAngleObservation>(station, "D");
        assert_eq!(d_zenith.len(), 1);
        near(d_zenith[0].angle, dms(91, 0, 15.0), ANGLE_TOLERANCE);
        assert_eq!(
            d_zenith[0].pointing,
            Pointing {
                index: 3,
                face: Face::Right
            }
        );

        // HV 200.00000,90.00000: no distance; a zenith of exactly 90 is face left.
        let f_zenith = observations_to::<ZenithAngleObservation>(station, "F");
        assert_eq!(f_zenith.len(), 1);
        near(f_zenith[0].angle, PI / 2.0, ANGLE_TOLERANCE);
        assert!(observations_to::<DistanceObservation>(station, "F").is_empty());
        near(
            observations_to::<HorizontalDirectionObservation>(station, "F")[0].direction,
            dms(200, 0, 0.0),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn a_horizontal_distance_record_gives_a_horizontal_distance_and_a_ground_level_difference() {
        let result = read_setup();
        let station = &result.project.stations[0];
        // FS E,1.80000 / HD 180.00000,50.00000,-1.25000 with HI 1.50000.
        let flat = observations_to::<DistanceObservation>(station, "E");
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].kind, DistanceKind::Horizontal);
        double_eq(flat[0].distance, 50.0);
        let rise = observations_to::<LevelDifferenceObservation>(station, "E");
        assert_eq!(rise.len(), 1);
        // VD + HI - HT = -1.25 + 1.5 - 1.8 = -1.55 (to the double: -1.25 + 1.5 is
        // exact, 0.25 - 1.8 rounds as the reader's own expression does).
        double_eq(rise[0].height_difference, -1.25 + 1.5 - 1.8);
        double_eq(rise[0].length, 50.0);
        assert_eq!(
            observations_to::<HorizontalDirectionObservation>(station, "E")[0].pointing,
            Pointing {
                index: 4,
                face: Face::Unknown
            }
        );
    }

    #[test]
    fn codes_and_string_numbers_become_features_and_offsets_are_kept_not_applied() {
        let result = read_setup();
        let project = &result.project;
        let kerb = feature(project, "KB").expect("KB");
        assert_eq!(kerb.name, "1");
        assert_eq!(kerb.point_ids, ["C", "D"]);
        let stations = feature(project, "STAT").expect("STAT");
        assert_eq!(stations.point_ids, ["A", "E"]);
        let e = unpositioned(project, "E").expect("E");
        assert_eq!(
            e.metadata["offset radial, tangential, vertical (m)"],
            "0.1, 0, 0"
        );
        assert_eq!(e.metadata["traverse foresight"], "record 19");
        assert!(any_warning_contains(&result, 21, "not applied"));
        assert_eq!(result.records_read, 24);
        assert_eq!(result.records_skipped, 0);
    }

    #[test]
    fn what_the_format_cannot_say_is_listed_as_not_carried() {
        let result = read_setup();
        assert!(any_not_carried_contains(&result, "no prism constant"));
        assert!(any_not_carried_contains(&result, "without units"));
        assert_eq!(
            result.project.stations[0].instrument.atmospheric_ppm_state,
            CorrectionState::Unknown
        );
        assert!(
            result.project.stations[0]
                .instrument
                .temperature_celsius
                .is_none()
        );
    }

    // The record list labels XYZ "X(easting), Y(northing)"; the manual's own
    // sample says otherwise. Station MARK is (N 10, E 10, Z 0.5) in the manual's
    // coordinate listing; the raw sample gives ST1 as "13.85600,7.04700,-0.25800",
    // observes it at the BKB circle reading, then radiates ST2 and prints ST2's
    // coordinates. Radiating ST2 from what the reader returns reproduces the
    // printed "14.87000,10.67900,-0.20400" to 2 mm only if XYZ is northing first.
    #[test]
    fn the_xyz_record_is_northing_first_as_the_manuals_own_sample_geometry_shows() {
        let bytes = concat!(
            "UNITS         M,D\n",
            "STN           MARK,1.52000,STAT\n",
            "XYZ           10.00000,10.00000,0.50000\n",
            "BKB           ST1,0.0000,322.33160\n",
            "XYZ           13.85600,7.04700,-0.25800\n",
            "BS            ST1,1.60000\n",
            "SD            -37.26440,97.57060,4.90200\n",
            "SS            ST2,1.60000,STAT\n",
            "SD            7.56170,97.13460,4.95600\n",
            "XYZ           14.87000,10.67900,-0.20400\n"
        );
        let result = ok(read(GTS7, bytes, "sample.gt7"));
        let project = &result.project;
        let mark = point(project, "MARK").expect("MARK");
        let st1 = point(project, "ST1").expect("ST1");
        let st2 = point(project, "ST2").expect("ST2");
        let station = &project.stations[0];
        // Orientation: the grid bearing MARK -> ST1 minus the circle reading on it.
        let bearing = (st1.easting - mark.easting).atan2(st1.northing - mark.northing);
        let orientation = bearing - station.backsight_azimuth.expect("a circle");
        let direction = observations_to::<HorizontalDirectionObservation>(station, "ST2");
        let zenith = observations_to::<ZenithAngleObservation>(station, "ST2");
        let slope = observations_to::<DistanceObservation>(station, "ST2");
        assert!(direction.len() == 1 && zenith.len() == 1 && slope.len() == 1);
        let azimuth = direction[0].direction + orientation;
        let horizontal = slope[0].distance * zenith[0].angle.sin();
        let northing = mark.northing + horizontal * azimuth.cos();
        let easting = mark.easting + horizontal * azimuth.sin();
        let elevation = mark.elevation.expect("MARK's elevation")
            + slope[0].distance * zenith[0].angle.cos()
            + station.setup.instrument_height
            - slope[0].target_height;
        near(northing, st2.northing, 0.002);
        near(easting, st2.easting, 0.002);
        near(elevation, st2.elevation.expect("ST2's elevation"), 0.002);
        assert_eq!(st2.coordinate_source, CoordinateSource::Calculated);
    }

    #[test]
    fn feet_and_gons_are_converted_and_the_unstated_foot_is_a_warning() {
        let bytes = concat!(
            "UNITS F,G\n",
            "STN P1,5.00000,\n",
            "SS P2,6.00000,EP\n",
            "SD 100.0000,100.0000,100.000\n"
        );
        let result = ok(read(GTS7, bytes, "feet.gt7"));
        let station = &result.project.stations[0];
        double_eq(station.setup.instrument_height, 5.0 * 0.3048);
        let slope = observations_to::<DistanceObservation>(station, "P2");
        assert_eq!(slope.len(), 1);
        double_eq(slope[0].distance, 100.0 * 0.3048);
        double_eq(slope[0].target_height, 6.0 * 0.3048);
        // 100 gon is a quarter circle, for the direction and the zenith alike.
        near(
            observations_to::<HorizontalDirectionObservation>(station, "P2")[0].direction,
            PI / 2.0,
            ANGLE_TOLERANCE,
        );
        near(
            observations_to::<ZenithAngleObservation>(station, "P2")[0].angle,
            PI / 2.0,
            ANGLE_TOLERANCE,
        );
        assert!(any_warning_contains(&result, 1, "which foot"));
        assert_eq!(result.project.units.angular, AngularUnit::Gons);
    }

    #[test]
    fn a_measurement_before_any_units_record_is_refused_rather_than_guessed() {
        let error = read(
            GTS7,
            "STN A,1.5,STAT\nSS B,1.6,X\nSD 1,90,10\n",
            "nounits.gt7",
        )
        .expect_err("refused");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(error.message.contains("record 1"));
    }

    #[test]
    fn a_measurement_with_no_shot_header_is_a_skipped_record_naming_its_line() {
        let result = ok(read(
            GTS7,
            "UNITS M,D\nSTN A,1.5,STAT\nSD 1.0000,90.0000,10.0\nBOGUS 1,2\n",
            "orphan.gt7",
        ));
        assert!(any_warning_contains(&result, 3, "no BS, FS or SS header"));
        assert!(any_warning_contains(&result, 4, "not a GTS-7 record"));
        assert_eq!(result.records_skipped, 2);
    }

    #[test]
    fn fields_past_the_ones_the_format_defines_are_warned_at_the_first_and_counted() {
        let result = ok(read(
            GTS7,
            concat!(
                "UNITS M,D\n",                  // 1
                "STN A,1.5,STAT,X9\n",          // 2: STN has 3
                "SS B,1.6,KB,1\n",              // 3
                "SD 10.0000,90.0000,5.000,7\n", // 4: SD has 3
                "SS C,1.6,KB,1\n",              // 5
                "SD 20.0000,90.0000,6.000,8\n"  // 6
            ),
            "extra.gt7",
        ));
        assert_eq!(result.records_read, 6);
        assert!(any_warning_contains(
            &result,
            2,
            "past the 3 the GTS-7 format defines ('X9')"
        ));
        assert!(any_warning_contains(&result, 4, "('7')"));
        assert!(!any_warning_contains(&result, 6, "past the"));
        assert!(any_warning_contains(
            &result,
            0,
            "2 SD records carry fields"
        ));
        assert!(any_warning_contains(&result, 0, "the first is record 4"));
        assert!(!any_warning_contains(&result, 0, "STN records carry"));
        // The measurements themselves are read in full.
        assert_eq!(result.project.stations[0].observations.len(), 6);
    }

    #[test]
    fn gts6_is_recognised_and_refused_with_what_to_export_instead() {
        let detection = detect_format(&probe_of(GTS6_SAMPLE.as_bytes(), "job.raw", false));
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(detection.format().expect("identified").id, GTS6);
        let error = read(GTS6, GTS6_SAMPLE, "job.raw").expect_err("refused");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.contains("GTS-7 raw"));
        assert!(error.message.contains("RW5"));
    }

    #[test]
    fn detection_identifies_gts7_by_its_control_words_whatever_the_extension() {
        let bytes = fixture("gts/setup.gt7");
        for name in ["setup.gt7", "setup.raw", "setup.dat", "setup.txt"] {
            let detection = detect_format(&probe_of(&bytes, name, false));
            assert_eq!(
                detection.outcome(),
                DetectionOutcome::Identified,
                "{name}: {}",
                detection.summary()
            );
            assert_eq!(detection.format().expect("identified").id, GTS7, "{name}");
        }
    }

    #[test]
    fn the_gts_probes_rule_out_rw5_and_coordinate_files() {
        for name in [
            "rw5/setup_metres.rw5",
            "rw5/feet_grads.rw5",
            "rw5/gnss.rw5",
            "rw5/journal.rw5",
            "survey_points_pnezd.csv",
            "survey_points_headerless.txt",
            "survey_points_headerless_penz.txt",
            "survey_points_pnezd_utm30s.csv",
        ] {
            let bytes = fixture(name);
            for probed in format_registry().probe_all(&probe_of(&bytes, name, false)) {
                if probed.format_id == GTS7 || probed.format_id == GTS6 {
                    assert!(
                        probed.signature.confidence < 0.5,
                        "{name} as {}: {}",
                        probed.format_id,
                        probed.signature.evidence
                    );
                }
            }
        }
        // And GTS-6 is not taken for GTS-7, nor the other way round.
        for probed in
            format_registry().probe_all(&probe_of(GTS6_SAMPLE.as_bytes(), "job.raw", false))
        {
            if probed.format_id == GTS7 {
                assert!(probed.signature.confidence < 0.5);
            }
        }
    }

    #[test]
    fn the_gts_probes_do_not_claim_leica_trimble_rinex_or_land_xml_files() {
        for sample in &FOREIGN_SAMPLES {
            let input = probe_of(sample.bytes.as_bytes(), sample.name, false);
            for probed in format_registry().probe_all(&input) {
                if probed.format_id == GTS7 || probed.format_id == GTS6 {
                    assert!(
                        probed.signature.confidence < 0.5,
                        "{} as {}: {}",
                        sample.name,
                        probed.format_id,
                        probed.signature.evidence
                    );
                }
            }
            let detection = detect_format(&input);
            if let Ok(format) = detection.format() {
                assert_ne!(format.id, GTS7, "{}", sample.name);
                assert_ne!(format.id, GTS6, "{}", sample.name);
            }
        }
    }

    #[test]
    fn every_truncation_and_random_bytes_give_an_error_or_a_result_never_a_crash() {
        let reads = read_every_truncation_and_noise(GTS7, &fixture("gts/setup.gt7"), "setup.gt7")
            + read_every_truncation_and_noise(GTS6, GTS6_SAMPLE.as_bytes(), "job.raw");
        assert!(reads > 1500);
    }

    #[test]
    fn a_read_of_a_large_synthetic_job_reads_every_record_and_reports_its_rate() {
        let mut megabytes = 2usize;
        if let Ok(wanted) = std::env::var("KATANA_SURVEYIO_THROUGHPUT_MB") {
            // strtoul: the leading digits, 0 for none.
            let digits: String = wanted.chars().take_while(char::is_ascii_digit).collect();
            megabytes = digits.parse().unwrap_or(0);
        }
        let mut bytes = String::from("JOB           throughput,synthetic\nUNITS         M,D\n");
        let mut setups = 0usize;
        let mut shots = 0usize;
        while bytes.len() < megabytes * 1024 * 1024 {
            setups += 1;
            let station = format!("S{setups}");
            bytes.push_str(&format!("STN           {station},1.52000,STAT\n"));
            bytes.push_str("XYZ           5000.00000,5000.00000,100.00000\n");
            bytes.push_str(&format!("BKB           B{setups},0.0000,45.00000\n"));
            for shot in 0..100 {
                shots += 1;
                bytes.push_str(&format!("SS            {shots},1.60000,KB,{}\n", shot % 7));
                bytes.push_str(&format!(
                    "SD            {}.15300,91.00150,{}.23450\n",
                    shot * 3,
                    10 + shot
                ));
            }
        }
        // The reader on its own, then the whole door (read_survey adds the size
        // caps and validate_project, which is not this reader's cost).
        let reader = format_registry().reader(GTS7).expect("a reader");
        let reader_seconds = {
            let start = Instant::now();
            let alone = reader(bytes.as_bytes(), "throughput.gt7", &ReadOptions::default());
            let seconds = start.elapsed().as_secs_f64();
            assert!(alone.is_ok());
            seconds
        }; // freed here, outside both timings
        let start = Instant::now();
        let result = ok(read(GTS7, &bytes, "throughput.gt7"));
        let seconds = start.elapsed().as_secs_f64();
        assert_eq!(result.records_read, 2 + setups * 3 + shots * 2);
        assert_eq!(result.records_skipped, 0);
        let mb = bytes.len() as f64 / (1024.0 * 1024.0);
        println!(
            "GTS-7 throughput: {mb} MB, {shots} shots; reader alone {reader_seconds} s, {} MB/s; \
             read_survey (with validate_project) {seconds} s, {} MB/s",
            mb / reader_seconds,
            mb / seconds
        );
    }

    #[test]
    fn a_point_occupied_many_times_gives_numbered_setups_in_linear_time() {
        // Numbering by search from 2 each time would make this 200 million
        // string comparisons; counted per point it is 20,000 steps.
        let mut bytes = String::from("UNITS M,D\n");
        for _ in 0..20000 {
            bytes.push_str("STN A,1.50000,STAT\n");
        }
        let result = ok(read(GTS7, bytes, "many.gt7"));
        assert_eq!(result.project.stations.len(), 20000);
        assert_eq!(result.project.stations[0].setup.id, "A");
        assert_eq!(result.project.stations[1].setup.id, "A (2)");
        assert_eq!(result.project.stations[19999].setup.id, "A (20000)");
    }
}
