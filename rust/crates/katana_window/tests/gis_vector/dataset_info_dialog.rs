//! The twins of tests/qt_widgets/geo/test_dataset_info_dialog.cpp: GIS > Dataset Information
//! (dataset_info_dialog), driven by its object names: the INFO lines it builds, run through an
//! executor as the window runs them, and what it shows of their records - the summary, the
//! fields, the bands, GDAL's JSON - and what its Statistics and Check buttons run. The records
//! themselves are tested in katana_app's geo twins.
//!
//! By hand: samples/gis/terrain.asc is 120 x 90 cells, one Float32 band with no-data -9999,
//! values 24.892 to 38.819 (read from the text); tests/geo/data/lots.geojson has one layer "lots"
//! with fields kind and name. Expected values are the C++'s.

use katana_window::dataset_info_dialog::DatasetInfoDialog;
use katana_window::host::{Answer, Clipboard, Host, RecordingHost};

use super::support::{Bench, executor_host, ok};
use super::window_support::{geo_data, samples};

fn terrain() -> String {
    samples("gis")
        .join("terrain.asc")
        .to_string_lossy()
        .into_owned()
}

/// The dialog on `path`, over an executor run inline, on the harness.
fn info_bench(path: &str, scratch: &str) -> Bench<DatasetInfoDialog, RecordingHost> {
    let mut host = executor_host(scratch);
    let dialog = DatasetInfoDialog::new(path, &mut host);
    Bench::new("datasetInfoDialog", dialog, host)
}

fn number(text: &str) -> f64 {
    katana_core::text::parse_finite_double(text).unwrap_or_else(|| panic!("no number: {text}"))
}

mod dataset_info_dialog {
    use super::*;

    #[test]
    fn it_runs_the_info_lines_and_shows_the_raster_its_bands_and_gdals_json() {
        let path = terrain();
        let mut bench = info_bench(&path, "katana-info-dialog-raster");
        assert_eq!(bench.dialog.object_name(), "datasetInfoDialog");
        assert!(bench.dialog.described());
        assert_eq!(bench.host.lines().len(), 2);
        assert_eq!(bench.host.lines()[0], format!("INFO \"{path}\""));
        assert_eq!(bench.host.lines()[1], format!("INFO \"{path}\" JSON"));
        assert_eq!(bench.text("datasetInfoCommand"), bench.host.lines()[1]);

        let summary = bench.text("datasetInfoSummary");
        assert!(summary.contains("dataset file="), "{summary}");
        assert!(summary.contains("driver=AAIGrid"));
        assert!(summary.contains("raster width=120 height=90 bands=1"));

        assert!(bench.has("datasetInfoBands"));
        let bands = bench.dialog.bands();
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0][0], "1");
        assert_eq!(bands[0][1], "Float32");
        assert_eq!(bands[0][2], "-9999");
        // Not computed: absent, not zero.
        assert_eq!(bands[0][3], "");

        let json = bench.text("datasetInfoJson");
        assert!(
            json.contains("\"driverShortName\": \"AAIGrid\""),
            "{}",
            json.chars().take(400).collect::<String>()
        );
        assert!(bench.enabled("datasetInfoCopyJson"));
        bench.press("datasetInfoCopyJson");
        assert_eq!(bench.host.parts.clipboard.text(), Some(json));
    }

    #[test]
    fn statistics_and_check_are_their_lines() {
        let path = terrain();
        let mut bench = info_bench(&path, "katana-info-dialog-stats");
        bench.press("datasetInfoStats");
        assert_eq!(
            bench.host.lines().last().cloned(),
            Some(format!("INFO \"{path}\" STATS"))
        );
        {
            let bands = bench.dialog.bands();
            assert_eq!(bands.len(), 1);
            // Float32 values: within half an ulp (1.9e-6 below 64) of the text's.
            assert!((number(&bands[0][3]) - 24.892).abs() <= 2e-6, "{}", bands[0][3]);
            assert!((number(&bands[0][4]) - 38.819).abs() <= 2e-6, "{}", bands[0][4]);
        }

        bench.press("datasetInfoCheck");
        assert_eq!(
            bench.host.lines().last().cloned(),
            Some(format!("INFO \"{path}\" CHECK"))
        );
        assert_eq!(bench.text("datasetInfoReply"), "check code=0 problems=0");
        // The check's own band records carry no statistics; the ones computed are of the same
        // file and stay (they were wiped to blanks).
        let bands = bench.dialog.bands();
        assert_eq!(bands.len(), 1);
        assert!((number(&bands[0][3]) - 24.892).abs() <= 2e-6, "{}", bands[0][3]);
        assert!((number(&bands[0][4]) - 38.819).abs() <= 2e-6, "{}", bands[0][4]);
    }

    #[test]
    fn a_vector_files_fields_are_listed() {
        let path = geo_data()
            .join("lots.geojson")
            .to_string_lossy()
            .into_owned();
        let bench = info_bench(&path, "katana-info-dialog-vector");
        assert!(bench.has("datasetInfoFields"));
        let fields = bench.dialog.fields();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0][0], "lots");
        assert_eq!(fields[0][1], "kind");
        assert_eq!(fields[0][2], "String");
        assert_eq!(fields[1][1], "name");
        assert_eq!(bench.dialog.bands().len(), 0);
    }

    #[test]
    fn a_file_that_cannot_be_described_says_why() {
        let bench = info_bench("C:/no such folder/absent.tif", "katana-info-dialog-absent");
        assert!(!bench.dialog.described());
        assert!(
            bench.text("datasetInfoReply").contains("NotFound"),
            "{}",
            bench.text("datasetInfoReply")
        );
        assert!(!bench.enabled("datasetInfoCopyJson"));
    }

    // The window's runner starts a job and answers later: the dialog shows it is reading, then
    // what the job said.
    #[test]
    fn a_jobs_answer_is_taken_when_the_job_ends() {
        let path = terrain();
        let mut host = RecordingHost::new().answering(Answer::Fixed(ok(
            "job id=7 title=\"INFO terrain.asc\" state=started",
        )));
        host.awaits_jobs = true;
        let dialog = DatasetInfoDialog::new(&path, &mut host);
        let mut bench = Bench::new("datasetInfoDialog", dialog, host);
        assert!(!bench.dialog.described());
        assert!(bench.text("datasetInfoReply").starts_with("Reading"));
        {
            let waiting = bench.host.awaited();
            assert_eq!(waiting.len(), 2);
            for started in waiting {
                assert!(started.reply.starts_with("job id=7"));
            }
        }
        // What the line says when it is run for real: the first job's outcome.
        let mut direct = executor_host("katana-info-dialog-job");
        let outcome = direct.run(&format!("INFO \"{path}\""));
        assert!(bench.host.finish_awaited(&outcome));
        // The dialog takes it at its next look.
        bench.draw();
        assert!(bench.dialog.described());
        assert_eq!(bench.dialog.bands().len(), 1);
    }
}
