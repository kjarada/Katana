// Builders for the reduction tests (tests/survey/test_reduction_support.hpp):
// a raw project the way a total-station parser hands it over (the contract of
// reduce_and_adjust), written in degrees and metres so the hand working in
// each test reads like a field book.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use katana_core::Result;
use katana_math::numerics::PI;
use katana_survey::data_model::{
    CoordinateSource, DistanceKind, DistanceObservation, Face, HorizontalDirectionObservation,
    Observation, Pointing, SourceRecord, Station, SurveyPoint, SurveyStation, UnpositionedPoint,
    ZenithAngleObservation,
};
use katana_survey::reduction::{ComputedPoint, ReductionOutcome};
use katana_survey::reduction_report::{
    AppliedCorrection, CorrectionKind, ReductionReport, ReportObservation,
};
use katana_survey::reduction_settings::{
    AtmosphericCorrection, PrismConstantPolicy, ReductionSettings,
};

/// deg(degrees, minutes, seconds) of the C++, in radians.
pub fn dms(degrees: f64, minutes: f64, seconds: f64) -> f64 {
    (degrees + minutes / 60.0 + seconds / 3600.0) * PI / 180.0
}

/// deg(degrees) of the C++ (its minutes and seconds 0).
pub fn deg(degrees: f64) -> f64 {
    dms(degrees, 0.0, 0.0)
}

pub fn arc_seconds(seconds: f64) -> f64 {
    seconds * PI / 648000.0
}

/// A point the file enters (CoordinateSource::Entered, the C++ default).
pub fn point(id: &str, northing: f64, easting: f64, elevation: Option<f64>) -> SurveyPoint {
    point_from(id, northing, easting, elevation, CoordinateSource::Entered)
}

pub fn point_from(
    id: &str,
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
    source: CoordinateSource,
) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation,
        coordinate_source: source,
        ..Default::default()
    }
}

pub fn unpositioned(id: &str) -> UnpositionedPoint {
    UnpositionedPoint {
        id: String::from(id),
        ..Default::default()
    }
}

/// One pointing: the circle reading, the zenith (FL-equivalent for face right,
/// as the model stores it) and the slope distance, any of them absent.
#[derive(Clone, Debug)]
pub struct Shot {
    pub target: String,
    pub index: usize,
    pub face: Face,
    pub direction: Option<f64>,
    pub zenith: Option<f64>,
    pub distance: Option<f64>,
    pub target_height: f64,
    pub kind: DistanceKind,
}

/// Shot{target, index, face, direction, zenith, distance, targetHeight} of the
/// C++, a slope distance.
pub fn shot(
    target: &str,
    index: usize,
    face: Face,
    direction: Option<f64>,
    zenith: Option<f64>,
    distance: Option<f64>,
    target_height: f64,
) -> Shot {
    Shot {
        target: String::from(target),
        index,
        face,
        direction,
        zenith,
        distance,
        target_height,
        kind: DistanceKind::Slope,
    }
}

pub fn setup(
    id: &str,
    point_id: &str,
    instrument_height: f64,
    backsight: &str,
    shots: &[Shot],
) -> SurveyStation {
    let mut station = SurveyStation {
        setup: Station {
            id: String::from(id),
            point_id: String::from(point_id),
            instrument_height,
        },
        backsight_point_id: String::from(backsight),
        ..Default::default()
    };
    for (record, shot) in (1..).zip(shots) {
        let source = SourceRecord {
            manufacturer: String::from("Test"),
            format: String::from("synthetic"),
            format_version: String::from("1"),
            file_name: String::from("field.raw"),
            record_number: record,
        };
        let pointing = Pointing {
            index: shot.index,
            face: shot.face,
        };
        if let Some(direction) = shot.direction {
            station.observations.push(Observation::HorizontalDirection(
                HorizontalDirectionObservation {
                    at: String::from(point_id),
                    to: shot.target.clone(),
                    direction,
                    sigma: arc_seconds(3.0),
                    source: source.clone(),
                    pointing,
                },
            ));
        }
        if let Some(zenith) = shot.zenith {
            station
                .observations
                .push(Observation::ZenithAngle(ZenithAngleObservation {
                    from: String::from(point_id),
                    to: shot.target.clone(),
                    angle: zenith,
                    sigma: arc_seconds(3.0),
                    instrument_height,
                    target_height: shot.target_height,
                    source: source.clone(),
                    pointing,
                }));
        }
        if let Some(distance) = shot.distance {
            station
                .observations
                .push(Observation::Distance(DistanceObservation {
                    from: String::from(point_id),
                    to: shot.target.clone(),
                    distance,
                    sigma: 0.002,
                    kind: shot.kind,
                    instrument_height,
                    target_height: shot.target_height,
                    source: source.clone(),
                    pointing,
                    ..Default::default()
                }));
        }
    }
    station
}

/// Settings with every correction off, so a test can turn on the one it is
/// about and nothing else moves the numbers.
pub fn bare_settings() -> ReductionSettings {
    ReductionSettings {
        atmospheric: AtmosphericCorrection::None,
        prism_constant_policy: PrismConstantPolicy::None,
        curvature_and_refraction: false,
        ..Default::default()
    }
}

pub fn find_row<'r>(
    report: &'r ReductionReport,
    kind: &str,
    to: &str,
    nth: usize,
) -> Option<&'r ReportObservation> {
    report
        .observations
        .iter()
        .filter(|row| row.kind == kind && row.to == to)
        .nth(nth)
}

pub fn find_correction(
    row: &ReportObservation,
    kind: CorrectionKind,
) -> Option<&AppliedCorrection> {
    row.corrections
        .iter()
        .find(|correction| correction.kind == kind)
}

pub fn find_point<'o>(outcome: &'o ReductionOutcome, id: &str) -> Option<&'o ComputedPoint> {
    outcome.points.iter().find(|point| point.id == id)
}

/// ASSERT_TRUE(outcome.ok()) << outcome.error().describe().
pub fn succeeded(outcome: Result<ReductionOutcome>) -> ReductionOutcome {
    outcome.unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// How many of the report's warnings hold `words`.
pub fn warnings_with(report: &ReductionReport, words: &str) -> usize {
    report
        .warnings
        .iter()
        .filter(|warning| warning.text.contains(words))
        .count()
}

/// Every warning, a line each, for a failure's message.
pub fn all_warnings(report: &ReductionReport) -> String {
    let mut text = String::new();
    for warning in &report.warnings {
        text.push_str(&warning.text);
        text.push('\n');
    }
    text
}
