// The cost of a large survey job's commands (tests/cad/test_survey_job_throughput.cpp),
// measured rather than asserted (a shared machine's timing is not a property);
// what IS asserted is that a job of this size still imports, re-adjusts and
// undoes as one step each.

use std::time::Instant;

use katana_cad::{
    Document, ImportSurveyJobCommand, ReadjustSurveyJobCommand, ReductionFunction,
    SurveyImportOptions, SurveyJob, SurveyJobContext, SurveyJobImport, SurveyJobReader,
    SurveyJobReadjustment,
};
use katana_core::Result;
use katana_survey::data_model::{CoordinateSource, SurveyPoint, SurveyProject};
use katana_survey::reduction::{ReductionContext, ReductionOutcome};
use katana_survey::reduction_report::ReductionReport;
use katana_survey::reduction_settings::ReductionSettings;

use crate::must;

const POINTS: usize = 50_000;

// A reduction that places point i at a grid position depending on i and on a
// shift, so a re-adjustment with another shift moves every point.
fn shifted_grid(shift: f64) -> ReductionFunction {
    Box::new(
        move |_: &SurveyProject,
              settings: &ReductionSettings,
              _: &ReductionContext|
              -> Result<ReductionOutcome> {
            let mut points = Vec::with_capacity(POINTS);
            for i in 0..POINTS {
                points.push(SurveyPoint {
                    id: format!("{}", 1000 + i),
                    northing: 6_250_000.0 + (i / 250) as f64 * 2.0 + shift,
                    easting: 300_000.0 + (i % 250) as f64 * 2.0 + shift,
                    elevation: Some(10.0 + shift),
                    code: String::from("NS"),
                    coordinate_source: CoordinateSource::Calculated,
                    ..SurveyPoint::default()
                });
            }
            Ok(ReductionOutcome {
                reduced: SurveyProject {
                    points,
                    ..SurveyProject::default()
                },
                report: ReductionReport {
                    settings: settings.clone(),
                    ..ReductionReport::default()
                },
                ..ReductionOutcome::default()
            })
        },
    )
}

fn milliseconds_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

mod survey_job_throughput {
    use super::*;

    #[test]
    fn fifty_thousand_points_import_readjust_and_undo_as_one_step_each() {
        let mut document = Document::default();
        let request = SurveyJobImport {
            job: SurveyJob {
                name: String::from("big.gsi"),
                source_bytes: vec![b'x'; 8_000_000], // about what 50k shots take
                layer: String::from("survey/big"),
                ..SurveyJob::default()
            },
            import_options: SurveyImportOptions {
                layer: String::from("survey/big"),
                ..SurveyImportOptions::default()
            },
            context: SurveyJobContext {
                created_utc: String::from("2026-09-24T00:00:00Z"),
                ..SurveyJobContext::default()
            },
            ..SurveyJobImport::default()
        };

        let start = Instant::now();
        let import = ImportSurveyJobCommand::new(&document, request, Some(shifted_grid(0.0)));
        let imported = import.results();
        must(&mut document, Box::new(import));
        let import_ms = milliseconds_since(start);
        assert_eq!(document.survey_jobs()[0].placed_points.len(), POINTS);

        let readjust = SurveyJobReadjustment {
            job_id: imported.job_id(),
            context: SurveyJobContext {
                created_utc: String::from("2026-09-25T00:00:00Z"),
                ..SurveyJobContext::default()
            },
            ..SurveyJobReadjustment::default()
        };
        let reader: SurveyJobReader =
            Box::new(|_: &SurveyJob| -> Result<SurveyProject> { Ok(SurveyProject::default()) });
        let start = Instant::now();
        let command = ReadjustSurveyJobCommand::new(
            &document,
            readjust,
            Some(reader),
            Some(shifted_grid(0.01)),
        );
        let readjusted = command.results();
        must(&mut document, Box::new(command));
        let readjust_ms = milliseconds_since(start);
        assert_eq!(readjusted.changes().moved.len(), POINTS);

        let start = Instant::now();
        assert!(document.undo().is_ok());
        let undo_ms = milliseconds_since(start);
        assert_eq!(
            document.survey_jobs()[0].placed_points[0].northing,
            6_250_000.0
        );

        let start = Instant::now();
        assert!(document.undo().is_ok());
        let undo_import_ms = milliseconds_since(start);
        assert!(document.survey_jobs().is_empty());
        assert_eq!(document.model().entities.size(), 0);

        let points = POINTS as f64;
        println!(
            "survey job of {POINTS} points: import {import_ms:.0} ms ({:.0} points/s), \
             re-adjust moving all {readjust_ms:.0} ms ({:.0} points/s), undo re-adjust \
             {undo_ms:.0} ms, undo import {undo_import_ms:.0} ms",
            points / (import_ms / 1000.0),
            points / (readjust_ms / 1000.0)
        );
    }
}
